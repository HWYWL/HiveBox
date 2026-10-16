#!/usr/bin/env python3
"""gen_charset_gb2312.py — 生成 SD 卡字体包用的兜底字符集。

卡上那份字体要兜住屏幕上的文件名、Wi-Fi 名与接口返回的文案 —— 它们不跟着源码走，只扫
`charset-common.txt` 是兜不住的。所以用 GB2312 的全部可显字符（行 A1-A9 的符号区 + 一级
汉字 B0A1-D7F9），再并上源码里画出来的字与 ASCII 可打印区。

GB2312 收不下的字靠 `EXTRAS` 补：一级汉字区里只有简体「云」，繁体「雲」不在里面。

    python tools/gen_charset_gb2312.py            # 报差异，不落盘
    python tools/gen_charset_gb2312.py --write    # 写回字符集文件
"""

import argparse
import pathlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
SOURCE = ROOT / "ui-framework" / "pomelo-font" / "source"
COMMON = SOURCE / "charset-common.txt"
CHARSET = SOURCE / "charset-gb2312.txt"

ASCII = "".join(chr(c) for c in range(0x20, 0x7F))

# GB2312：符号区与一级汉字（按拼音排的 3755 字，最后一个码位是 D7F9）。
SYMBOLS = (0xA1A1, 0xA9FE)
HANZI = (0xB0A1, 0xD7F9)

# 显式补的字。「雲」（U+96F2）：一级汉字区里只有简体「云」，繁体「雲」不在 —— 屏幕上的
# 某个名字用了它，而 charset-common.txt 是扫源码来的，扫不到。
EXTRAS = "雲"

HEADER = [
    "# 兜底字符集：ASCII 可打印区 + 源码里画出来的字（charset-common.txt）+ GB2312 的符号区",
    "# （0xA1A1-0xA9FE）与一级汉字（0xB0A1-D7F9，3755 字），外加几个 GB2312 收不下的字。",
    "#",
    "# 这是给 **SD 卡上的字体包**用的字符集：屏幕上的文件名、Wi-Fi 名不跟着源码走，得有一个",
    "# 标准字库兜着。镜像里那份子集仍然用 charset-common.txt（体积换 OTA 槽位），两档并存。",
    "#",
    "# 由 tools/gen_charset_gb2312.py 生成，勿手改 —— 要补字就改那个脚本里的 EXTRAS 再重跑：",
    "#     python tools/gen_charset_gb2312.py --write",
]


def gb2312(start, end):
    """一段 GB2312 码位里所有能解码的字符，按码位升序。"""
    out = []
    for row in range(start >> 8, (end >> 8) + 1):
        for cell in range(0xA1, 0xFF):
            code = (row << 8) | cell
            if code < start or code > end:
                continue
            try:
                out.append(bytes([row, cell]).decode("gb2312"))
            except UnicodeDecodeError:
                continue
    return out


def read(path):
    """一个字符集文件里的字符：整行注释跳过，其余按字符收（与 bake.py 同一套读法）。"""
    if not path.exists():
        return set()
    text = path.read_text(encoding="utf-8")
    return {ch for line in text.splitlines() if not line.lstrip().startswith("#") for ch in line if ord(ch) >= 0x20}


def charset():
    chars = set(ASCII) | set(gb2312(*SYMBOLS)) | set(gb2312(*HANZI)) | read(COMMON) | set(EXTRAS)
    return "".join(sorted(chars))


def main():
    parser = argparse.ArgumentParser(description="生成卡上字体包用的兜底字符集")
    parser.add_argument("--write", action="store_true", help=f"写回 {CHARSET.name}")
    args = parser.parse_args()

    chars = charset()
    before = read(CHARSET)

    print(f"字符集：{len(chars)} 字（当前文件 {len(before)} 字）")
    added = sorted(set(chars) - before)
    dropped = sorted(before - set(chars))
    print(f"  新增 {len(added)} 字：{''.join(added)}")
    print(f"  裁掉 {len(dropped)} 字：{''.join(dropped)}")

    if not args.write:
        print("\n（--write 才会写回）")
        return 0

    CHARSET.write_text("\n".join(HEADER) + "\n\n" + chars + "\n", encoding="utf-8")
    print(f"\n写入 {CHARSET.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
