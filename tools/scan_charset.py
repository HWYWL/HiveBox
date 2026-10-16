#!/usr/bin/env python3
"""scan_charset.py — 从固件的源码里扫出真正会画到屏幕上的字符。

为什么要这么干：`pomelo-font` 的子集字符集当前是 GB2312 一级字（3755 字），而固件里
真正写过的中文只有几百个。多出来的那些字，每个都要在运行时字体（约 445 B/字）和 24 px
预烘表（约 506 B/字）里各付一次，合起来是几 MB —— 而这些 flash 正是 OTA 双槽位缺的。

所以字符集改由源码推出：扫字符串与字符字面量（**注释不算**，注释不进固件），加上
ASCII 可打印区，写成 `pomelo-font/source/charset-common.txt`。

代价：屏幕上的文字不再有"兜底字库"了。用户自己造出来的字（Wi-Fi 名、SD 卡上的文件名、
网络接口返回的文案）只要没在源码里出现过，就会画成 `.notdef`（方框）。这是刻意的取舍：
要兜底就把 3755 字塞回来。

用法：

    python tools/scan_charset.py            # 打印报告
    python tools/scan_charset.py --write    # 同时写 charset 文件
"""

import argparse
import collections
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

# 真的会进固件的代码。`ui-framework/iced` 是上游 iced 的源码树（只用来 patch），它的
# `Cargo.toml` 里有作者名之类的非 ASCII，扫进去就是白发一个字形。
ROOTS = [
    ROOT / "pomelo-apps",
    ROOT / "pomelo-widgets",
    ROOT / "pomelo-hal",
    ROOT / "pomelo-firmware",
    ROOT / "ui-framework" / "iced-pomelo-winit",
    ROOT / "ui-framework" / "iced-pomelo-gfx",
    ROOT / "ui-framework" / "pomelo-gfx",
    ROOT / "ui-framework" / "pomelo-material-symbols",
    ROOT / "ui-framework" / "iced" / "src",
]

SKIP = {"target", "build", "managed_components", ".git", "node_modules", "dist", ".codebuddy", "vendor"}
EXTS = {".rs", ".toml"}

CHARSET = ROOT / "ui-framework" / "pomelo-font" / "source" / "charset-common.txt"

# ASCII 可打印区：留着。它们是排版的基础（空格、数字、ASCII 标点），而且九个字符加起来
# 也顶不上一个汉字，删它们省不下任何东西。
ASCII = "".join(chr(c) for c in range(0x20, 0x7F))

# 除 ASCII 外唯一无条件留下的字符：全角空格。
#
# 它不是一个"文案里的字"，扫描永远扫不到它 —— 它出现在布局代码里（`text("　")` 这种对齐
# 技巧），而 `bake.py` 把它和空格、`0`、`A` 一起列为哨兵：少了它，U+3000 会落到 `.notdef`，
# 排版里每一个全角空格都会变成方框。一个字符，留着。
ALWAYS = ASCII + "\u3000"


# 打日志的宏：参数是给串口看的，不画到屏幕上。
#
# 唯一的实锤在 `rust_main` 的 panic hook 里：`eprintln!("\n🚨 [RUST PANIC] {info}\n")`。
# 那颗 emoji 的字形思源黑体里根本没有，收进字符集只换来 `bake.py` 一句「缺 1 字」——
# 而字符集每多一个字，屏幕上就少一个字的位置。
#
# 只列确定是日志的。`format!`、`text!`、`column!` 这些**不跳**：它们在 UI 里拼文案，
# 收多了只是多几个字形，漏收才是屏幕上开天窗。
LOG_MACROS = {
    "println", "print", "eprintln", "eprint", "dbg",
    "panic", "assert", "assert_eq", "assert_ne",
    "debug_assert", "debug_assert_eq", "debug_assert_ne",
    "unreachable", "todo", "unimplemented",
    "debug", "info", "warn", "error", "trace",
}


def skip_group(text: str, i: int) -> int:
    """`i` 指向 `(`，返回与之配对的 `)` 的下标 + 1。字符串与注释里的括号不算数。"""
    n = len(text)
    depth = 0
    while i < n:
        c = text[i]
        if c == "(":
            depth += 1
            i += 1
        elif c == ")":
            depth -= 1
            i += 1
            if depth == 0:
                return i
        elif c == '"':
            _, i = quoted(text, i + 1, '"')
        elif c == "'":
            _, i = quoted(text, i + 1, "'")
        elif text.startswith("//", i):
            j = text.find("\n", i)
            i = n if j < 0 else j
        elif text.startswith("/*", i):
            j = text.find("*/", i)
            i = n if j < 0 else j + 2
        else:
            i += 1
    return i


def lex(text: str):
    """产出 ('str', 内容) / ('chr', 内容) / ('comment', 内容)。

    够用的 Rust 词法：注释、普通字符串（带转义）、raw 字符串、字符字面量，以及日志宏的
    整段参数（当注释处理，也就是丢掉）。
    """
    i, n = 0, len(text)
    while i < n:
        c = text[i]
        if c == "/" and i + 1 < n and text[i + 1] == "/":
            j = text.find("\n", i)
            j = n if j < 0 else j
            yield "comment", text[i:j]
            i = j
        elif c == "/" and i + 1 < n and text[i + 1] == "*":
            depth, j = 1, i + 2
            while j < n and depth:
                if text.startswith("/*", j):
                    depth += 1
                    j += 2
                elif text.startswith("*/", j):
                    depth -= 1
                    j += 2
                else:
                    j += 1
            yield "comment", text[i:j]
            i = j
        elif c in "rb" and (m := raw_prefix(text, i)) is not None:
            body, i = m
            yield "str", body
        elif c == '"':
            body, i = quoted(text, i + 1, '"')
            yield "str", body
        elif c == "'":
            body, i = quoted(text, i + 1, "'")
            # 生命周期 `'a` 会被误当成字符字面量；只收长度 <= 2 的（`'中'`、`'\n'`）。
            if body and len(body) <= 2:
                yield "chr", body
        elif c.isalpha() or c == "_":
            j = i
            while j < n and (text[j].isalnum() or text[j] == "_"):
                j += 1
            word = text[i:j]
            # `ident!` 只有宏会这么写，所以先看标识符、再看它后面是不是 `!(`。
            k = j
            while k < n and text[k] in " \t\r\n":
                k += 1
            if word in LOG_MACROS and k < n and text[k] == "!":
                k += 1
                while k < n and text[k] in " \t\r\n":
                    k += 1
                # 整段扔掉：这里有日志的文案，而它们不上屏 —— `scan()` 只认 `str` / `chr`。
                i = skip_group(text, k) if k < n and text[k] == "(" else k
                continue
            i = j
        else:
            i += 1


def raw_prefix(text: str, i: int):
    """r"..." / br"..." / r#"..."# —— 返回 (内容, 下一个下标)；不是 raw 串就返回 None。"""
    j = i
    if text[j] == "b":
        j += 1
    if j >= len(text) or text[j] != "r":
        return None
    j += 1
    hashes = 0
    while j < len(text) and text[j] == "#":
        hashes += 1
        j += 1
    if j >= len(text) or text[j] != '"':
        return None
    terminator = '"' + "#" * hashes
    end = text.find(terminator, j + 1)
    if end < 0:
        return None
    return text[j + 1 : end], end + len(terminator)


def quoted(text: str, i: int, quote: str):
    """从 i 读到 quote 为止，处理反斜杠转义。返回 (内容, 下一个下标)。"""
    out = []
    n = len(text)
    while i < n:
        c = text[i]
        if c == "\\":
            out.append(text[i : i + 2])
            i += 2
            continue
        if c == quote:
            return "".join(out), i + 1
        if c == "\n":  # 没闭合，当普通代码处理
            return "".join(out), i
        out.append(c)
        i += 1
    return "".join(out), i


def scan():
    """返回 (字面量字符集, 每字符来源)。"""
    found = {}
    seen = set()

    for root in ROOTS:
        for path in sorted(root.rglob("*")):
            if path.suffix.lower() not in EXTS or not path.is_file():
                continue
            if any(part in SKIP for part in path.parts):
                continue
            try:
                text = path.read_text(encoding="utf-8")
            except Exception:
                continue
            rel = str(path.relative_to(ROOT))
            for kind, body in lex(text):
                if kind == "comment":
                    continue
                for ch in body:
                    # 转义序列的残余（`\"` 留下的 `"`）是 ASCII，会被 ASCII 那一档兜住。
                    if ch == "\\" or ch in seen:
                        continue
                    seen.add(ch)
                    found[ch] = rel

    return found


def charset_of(found) -> str:
    """字面量字符 + [`ALWAYS`]，按码位排序 —— 顺序无关，排序只为让 diff 稳定。"""
    chars = set(found) | set(ALWAYS)
    return "".join(sorted(chars))


def main() -> int:
    parser = argparse.ArgumentParser(description="从源码扫出字符集")
    parser.add_argument("--write", action="store_true", help=f"写回 {CHARSET}")
    args = parser.parse_args()

    found = scan()
    chars = charset_of(found)

    # 与当前字符集比一比，让人看清这一次裁掉了多少。
    before = None
    if CHARSET.exists():
        before = {
            ch
            for line in CHARSET.read_text(encoding="utf-8").splitlines()
            if not line.lstrip().startswith("#")
            for ch in line
            if ord(ch) >= 0x20
        }

    print(f"源码里出现的字符（字面量）: {len(found)}")
    print(f"新字符集（+ ASCII/全角空格）: {len(chars)}")
    if before is not None:
        dropped = before - set(chars)
        added = set(chars) - before
        print(f"当前字符集                 : {len(before)}")
        print(f"  裁掉 {len(dropped)} 字，新增 {len(added)} 字")

    if not args.write:
        print("\n（--write 才会写回字符集文件）")
        return 0

    body = [
        "# 界面实际画出来的字符，由 tools/scan_charset.py 从源码里扫出来 ——",
        "# 不要手工往里加字：先把那句话写进代码，再重跑扫描。",
        "#",
        "# 上一次的字符集是 GB2312 一级字（3755 字）。多出来的那些在运行时字体里约 445 B/字、",
        "# 在 24 px 预烘表里约 506 B/字，合计近 4 MB —— 那正是 OTA 双槽位缺的几 MB。",
        "#",
        "# 代价：没有兜底字库了。屏幕上的字如果没在源码里出现过（Wi-Fi 名、SD 卡文件名、",
        "# 网络接口返回的文案），画出来是方框（.notdef）。要兜底就把 GB2312 一级字放回来。",
        "#",
        "# 注意：任何改动都要重做子集化与烘培 —— 子集化会重排 glyph_id，表必须跟着重烘。",
        "# 见 ../README.md 的「子集化」与「烘培字形表」两节。",
        "#",
    ]

    # 一行放得下，bake.py 按行读；ASCII 与汉字放在一起，因为两者是一套。
    CHARSET.write_text("\n".join(body) + "\n" + chars + "\n", encoding="utf-8")
    print(f"\n写入 {CHARSET.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
