#!/usr/bin/env python3
"""
tools/bake_icons.py
这个 crate 自己的工具：把上游的 Material Symbols 烘成 `fonts/` 里的字体和 `src/table.rs`。

它不依赖 `pomelo-os`，也不读那儿的任何东西：上游文件下载到 `fonts/source/`（不进仓库），
产物写回本仓库。在一个干净的 checkout 里能把整个 crate 重建出来。

# 为什么是「字体」，不是一堆 SVG

因为这是**渲染器今天唯一走得通的一条路**。`pomelo-gfx` 能贴 8 位掩码（`blit_mask`）与
RGB565 位图，但 iced 侧对应用没有开这两扇门（`allocate_image` 回 `Unsupported`、
`geometry::draw_image` 是空的），`Canvas::fill_path` 也只是描了个轮廓 —— 所以「图标 = 位图」
或「图标 = 矢量路径」都还得先改渲染器。而**字形**这条路是完备的：字体装载、排版、查表光栅化
都在跑（思源黑体子集就是这么进来的）。

于是图标就是一种字体，画图标就是画一个私有区（PUA）码点：

    text(Icon::WIFI.glyph()).font(pomelo_material_symbols::font()).size(20.0)

顺带的好处：矢量、可缩放、一个 em 见方（advance = upem = 960，所以 `size(24.0)` 就是 24×24
的图标）、跟别的文字共用同一条绘制与缓存通路。

# 这个工具做什么

1. 把上游的**可变字体**按 `AXES` 钉死成静态字体（iced 没法给字形设 `font-variation-settings`，
   所以轴必须在构建机上定下来）；
2. 用 `fontTools.subset` 只留**有 `Icon` 常量的那些码点**；
3. 从同一份上游目录生成 `src/table.rs`：一个图标一个 `const`，所以「这个系统能画哪些图标」
   在 Rust 里是类型，不是文档。

**全量：上游目录里 4 299 个名字全都发。** 用的是别的一个都没有 —— 想用一个谁都没用过的新
图标，不需要跑这个工具。

第 2 步听起来像「裁剪」，但**它裁掉的不是图标**：上游那份字体一共映射了 4 405 个码点，
其中 406 个不是图标 —— 66 个 ASCII 占位字形（上游拿它们做「打 wifi 出图标」的连字，
而我们把 GSUB 丢了）加 339 个旧版 Material Icons 的兼容码点。这些码点没有 `Icon` 常量，
Rust 里根本够不着，留着白占 381 KB（1 160 KB → 777 KB）。

代价是那份字体占 777 KB 的 flash（数字见 `fonts/MANIFEST.md`）—— 而 PSRAM 里一分钱不花，
因为 `FontSystem::load_font` 现在是借用而不是复制；没被引用到的 `Icon` 常量也不会进二进制
（`const` 没人用就不生成）。

字体和表是**同一份产物**：`table.rs` 里带着那份字体的校验和，crate 的测试会对着 `FONT`
重算一遍，所以「只重生成了一半」是测试就红的。

# 用法

    python3 tools/bake_icons.py                      # 重烘 + 重新生成
    python3 tools/bake_icons.py --search wifi        # 上游有哪些名字含 "wifi"
    python3 tools/bake_icons.py --fill 1             # 换成实心（默认是描边）
    python3 tools/bake_icons.py --check              # 只检查产物是不是最新的（CI 用）

只依赖 `fontTools`（`pip install fonttools`）。上游文件不进仓库（10.7 MB，一条命令就能
重现，见 `.gitignore`）：

    mkdir -p fonts/source && cd fonts/source
    base=https://raw.githubusercontent.com/google/material-design-icons/master/variablefont
    curl -sSL -O "$base/MaterialSymbolsOutlined%5BFILL%2CGRAD%2Copsz%2Cwght%5D.ttf"
    curl -sSL -O "$base/MaterialSymbolsOutlined%5BFILL%2CGRAD%2Copsz%2Cwght%5D.codepoints"
    curl -sSL -o LICENSE-APACHE-2.0.txt \\
        https://raw.githubusercontent.com/google/material-design-icons/master/LICENSE

（`curl -O` 会还原出上游那个带方括号的文件名，工具就是按那个名字找的。）

生成出来的字体、表和许可证副本都**进仓库**。换轴等于给每个图标换一副样子 —— 跑完要有人
看一眼板上再提交。
"""

import argparse
import hashlib
import io
import pathlib
import shutil
import subprocess
import sys

try:
    from fontTools import subset
    from fontTools.ttLib import TTFont
    from fontTools.varLib import instancer
except ImportError:
    print("[!] 需要 fontTools：pip install -r tools/requirements.txt")
    sys.exit(1)

ROOT = pathlib.Path(__file__).resolve().parent.parent

SOURCE_DIR = ROOT / "fonts" / "source"
STYLE = "MaterialSymbolsOutlined[FILL,GRAD,opsz,wght]"
SOURCE = SOURCE_DIR / f"{STYLE}.ttf"
CODEPOINTS = SOURCE_DIR / f"{STYLE}.codepoints"
LICENCE = SOURCE_DIR / "LICENSE-APACHE-2.0.txt"

FONT_OUT = ROOT / "fonts" / "MaterialSymbolsOutlined-Subset.ttf"
TABLE_OUT = ROOT / "src" / "table.rs"
MANIFEST_OUT = ROOT / "fonts" / "MANIFEST.md"
LICENCE_OUT = ROOT / "fonts" / "LICENSE-APACHE-2.0.txt"

# 子集字体里那份 name 表保留的家族名。**与轴无关**：`FILL` 由 0 改 1 不会改这个名字，
# 所以 `icons::FAMILY` 和已经写好的调用点都不会因为调轴而失效（代价是这里只能有一份实例：
# 要同时上「描边」和「实心」两套，得先给第二份改 name 表）。
FAMILY = "Material Symbols Outlined"

# 钉死的轴值。上游四个轴：FILL 0..1、GRAD -50..200、opsz 20..48、wght 100..700。
#
# `opsz` 钉在 24（上游默认，也是这套图标的设计尺寸）；`wght` 400 是常规字重；`GRAD` 0 不变。
# `FILL` 默认 0（描边）—— 这是 Material Symbols 的标准长相，也是谷歌自己在网页上的默认值。
# 小尺寸上实心更好认，那时用 `--fill 1` 重裁一次即可。
AXES = {"FILL": 0.0, "GRAD": 0.0, "opsz": 24.0, "wght": 400.0}

# 子集里不要的东西：
#   * GSUB/GPOS/GDEF —— 上游拿它们做「连字」：打 "wifi" 就出 wifi 图标。我们一律用码点取图标，
#     用不上，而且留着等于让随便一个 `text(..)` 有可能被连字改写。丢掉更小也不会咬人。
#   * STAT —— 描述轴的表，轴都钉死了，它只剩噪音。
#   * DSIG —— 签名，改过之后本来就失效。
DROP_TABLES = ["GSUB", "GPOS", "GDEF", "STAT", "DSIG"]


def load_codepoints(path: pathlib.Path) -> dict[str, int]:
    """上游的 `.codepoints`：一行一个 `名字 16 进制码点`。"""
    codes: dict[str, int] = {}

    for line in path.read_text(encoding="utf-8").splitlines():
        parts = line.split()

        if len(parts) == 2:
            codes[parts[0]] = int(parts[1], 16)

    return codes


def bake(source: bytes, entries: list[str], codes: dict[str, int], axes: dict[str, float]) -> bytes:
    """把上游可变字体钉成静态字体并裁成子集，返回字体字节。"""
    font = TTFont(io.BytesIO(source))

    # 1. 钉轴。`updateFontNames=False`：name 表保持上游原样，家族名因此与轴值无关。
    instancer.instantiateVariableFont(font, axes, inplace=True, updateFontNames=False)

    # 2. 裁子集。
    options = subset.Options()
    options.drop_tables += DROP_TABLES
    options.layout_features = []
    # 保留全部 name 记录：`FAMILY` 是靠家族名找到这份字体的，名字被裁掉就会静默落到后备字体上
    # —— 思源黑体那边踩过同一个坑（见 vendor/iced-pomelo-winit/fonts/README.md）。
    options.name_IDs = ["*"]
    options.name_languages = ["*"]
    options.name_legacy = True
    # `post` 里不带字形名（format 3）：设备端不需要，留着白占体积。
    options.glyph_names = False
    # 保留 .notdef 的轮廓：真出现没定义的码点时，画出来的是一个明确的方框而不是空白。
    options.notdef_outline = True
    options.recalc_bounds = True
    # 重放要字节一致，所以不让它写时间戳，也不让表顺序随源码的顺序漂。
    options.recalc_timestamp = False
    options.canonical_order = True

    wanted = {codes[name] for name in entries}

    subsetter = subset.Subsetter(options=options)
    subsetter.populate(unicodes=wanted)
    subsetter.subset(font)

    # 3. 时间戳钉死：`recalc_timestamp` 只管子集化这一层，`TTFont.save` 自己还有个
    #    `recalcTimestamp`（默认开），会在写盘时把 `head.modified` 盖成当前时间 —— 那样子集
    #    每跑一次都不一样，`--check` 也就永远说产物是旧的。
    font.recalcTimestamp = False
    head = font["head"]
    head.modified = head.created

    out = io.BytesIO()
    font.save(out)
    return out.getvalue()


def const_name(name: str) -> str:
    """上游名字 -> Rust 关联常量名：`signal_wifi_4_bar` -> `SIGNAL_WIFI_4_BAR`。

    上游的名字本来就是 `[a-z0-9_]`，大写化就够；只有 `10k`、`1x_mobiledata` 这类以数字开头的
    需要加一个下划线前缀（常量名不能以数字开头）。不是这个形状的名字宁可报错也不要猜 ——
    猜错了会静默生成一个和上游对不上的名字。
    """
    if not name or not all(char.islower() or char.isdigit() or char == "_" for char in name):
        raise SystemExit(f"[!] 上游名字 {name!r} 不是 [a-z0-9_]，得人工决定它的 Rust 名字")

    return name.upper() if name[0].islower() else "_" + name.upper()


def checksum(source: bytes) -> int:
    """FNV-1a 64，必须和生成的 `table::checksum` 算得一模一样。"""
    hash = 0xCBF29CE484222325

    for byte in source:
        hash ^= byte
        hash = (hash * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF

    return hash


def rust_table(entries: list[str], codes: dict[str, int], font: bytes) -> str:
    consts: list[str] = []
    seen: set[str] = set()

    for name in entries:
        ident = const_name(name)

        if ident in seen:
            raise SystemExit(f"[!] {name} 与另一个名字都生成 {ident}")
        seen.add(ident)

        code = codes[name]
        # 常量名里还认得出上游名字（只是大写了），所以文档里写的是码点：想查「这个图标长
        # 什么样」时，U+ 号是唯一能拿去别处对照的东西。
        consts += [
            f"    /// `{name}` — U+{code:04X}",
            f'    pub const {ident}: Icon = Icon("\\u{{{code:x}}}");',
        ]

    return "\n".join(
        [
            "//! The icon table, generated by `tools/bake_icons.py`. Do not edit: run the tool.",
            "//!",
            "//! One `const` per icon in the catalogue, and the checksum of the font they were",
            "//! generated from.",
            "",
            "/// The checksum of the font this table was generated from.",
            "///",
            "/// The crate's own test recomputes it over `FONT`, so a table and a font that came from",
            "/// different bakes -- the one way to regenerate half of this crate -- is a red build.",
            "#[cfg(test)]",
            f"pub(crate) const FONT_CHECK: u64 = 0x{checksum(font):016x};",
            "",
            "/// FNV-1a over `bytes`.",
            "///",
            "/// A hash is not a guarantee, but the failure worth catching here is not an attacker:",
            "/// it is two files that were generated at different times.",
            "#[cfg(test)]",
            "pub(crate) const fn checksum(bytes: &[u8]) -> u64 {",
            "    let mut hash = 0xcbf2_9ce4_8422_2325u64;",
            "    let mut index = 0;",
            "",
            "    while index < bytes.len() {",
            "        hash ^= bytes[index] as u64;",
            "        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);",
            "        index += 1;",
            "    }",
            "",
            "    hash",
            "}",
            "",
            "/// One icon: a code point in the Private Use Area of the font beside this file.",
            "///",
            "/// The whole catalogue is here, one `const` each, so this type is the answer to",
            '/// "which icons may this system draw" -- an icon that is not one of these does not',
            "/// compile. An unused one costs nothing: a `const` that nothing references is not",
            "/// emitted.",
            "///",
            "/// A few names are aliases of each other upstream -- 216 groups covering 300 of the",
            "/// names -- and an alias draws the same picture as the name it aliases.",
            "#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]",
            "pub struct Icon(&'static str);",
            "",
            "impl Icon {",
            *consts,
            "",
            "    /// The icon as the one-character string a `Text` wants.",
            "    ///",
            "    /// A `&'static str` and not a `String`: an icon is drawn many times a frame, and",
            "    /// this is one less allocation on the way to the panel. The characters live in",
            "    /// `.rodata`, and only the ones a program names are emitted.",
            "    pub const fn glyph(self) -> &'static str {",
            "        self.0",
            "    }",
            "",
            "    /// The code point it has in the subset.",
            "    pub fn codepoint(self) -> char {",
            '        self.0.chars().next().expect("an icon is one character")',
            "    }",
            "}",
            "",
        ]
    )


def main() -> int:
    parser = argparse.ArgumentParser(description="裁 Material Symbols 子集并生成 Icon 枚举")
    parser.add_argument("--source", type=pathlib.Path, default=SOURCE)
    parser.add_argument("--codepoints", type=pathlib.Path, default=CODEPOINTS)
    parser.add_argument("--fill", type=float, default=AXES["FILL"], help="0 描边（默认），1 实心")
    parser.add_argument("--weight", type=float, default=AXES["wght"], help="100..700，默认 400")
    parser.add_argument("--grade", type=float, default=AXES["GRAD"], help="-50..200，默认 0")
    parser.add_argument("--optical", type=float, default=AXES["opsz"], help="20..48，默认 24")
    parser.add_argument("--search", metavar="NEEDLE", help="先列出上游含 NEEDLE 的名字，然后退出")
    parser.add_argument("--check", action="store_true", help="只检查产物是不是最新的，不写文件")
    args = parser.parse_args()

    if not args.codepoints.exists():
        print(f"[!] 缺少上游码点表：{args.codepoints}")
        print("    下载命令见本文件头部。")
        return 1

    codes = load_codepoints(args.codepoints)

    if args.search:
        needle = args.search.lower()
        hits = sorted(name for name in codes if needle in name)
        print(f"上游有 {len(hits)} 个名字含 {args.search!r}：")

        for name in hits:
            print(f"  {name:44s} U+{codes[name]:05X}")

        return 0

    if not args.source.exists():
        print(f"[!] 缺少上游字体：{args.source}")
        print("    下载命令见本文件头部。")
        return 1

    # 全部 4 299 个，按上游目录的顺序（`.codepoints` 是排好序的，所以生成的常量也是）。
    entries = list(codes)

    axes = {"FILL": args.fill, "GRAD": args.grade, "opsz": args.optical, "wght": args.weight}
    source = args.source.read_bytes()

    print(f"图标     {len(entries)} 个（上游目录的全部）")
    print(
        "上游     "
        f"{args.source.name}  {len(source) / 1e6:.2f} MB  "
        f"sha256 {hashlib.sha256(source).hexdigest()[:16]}…"
    )
    print(f"轴       {', '.join(f'{tag}={value:g}' for tag, value in axes.items())}")
    print()

    font = bake(source, entries, codes, axes)
    table = rust_table(entries, codes, font)

    # 语料在 Rust 里要能被 rustfmt 认下来，所以过一遍 rustfmt（不在的话原样写出去，并在
    # `--check` 里会看出来）。生成物要能被 `cargo fmt --check` 接受，这条是硬要求。
    formatted = subprocess.run(
        ["rustfmt", "--edition", "2021", "--emit", "stdout", "--quiet"],
        input=table,
        capture_output=True,
        text=True,
    )

    if formatted.returncode == 0 and formatted.stdout:
        table = formatted.stdout
    else:
        print("[!] 没有可用的 rustfmt，生成物按未格式化的样子写出")

    # 校验产物本身：裁完的字体必须真的带着这些码点，而且每个都画得出东西。
    check = TTFont(io.BytesIO(font))
    cmap = check.getBestCmap()
    upem = check["head"].unitsPerEm
    broken = []

    for name in entries:
        code = codes[name]

        if code not in cmap:
            broken.append(f"{name} (U+{code:04X}) 不在 cmap 里")
            continue

        glyph = check["glyf"][cmap[code]]
        advance, _ = check["hmtx"][cmap[code]]

        # `numberOfContours == 0` 是空字形；-1 是复合字形（由别的字形拼出来），那是有东西的。
        if glyph.numberOfContours == 0:
            broken.append(f"{name} (U+{code:04X}) 没有轮廓")
        if advance != upem:
            broken.append(f"{name} (U+{code:04X}) 的 advance 是 {advance}，不是 1 em（{upem}）")

    if broken:
        print("[!] 裁出来的子集有问题：")

        for line in broken:
            print(f"    {line}")

        return 1

    family = check["name"].getDebugName(1)

    if family != FAMILY:
        print(f"[!] 子集的家族名是 {family!r}，不是 {FAMILY!r} —— icons::FAMILY 会对不上")
        return 1

    manifest = rust_manifest(source, entries, codes, axes, font, family, upem)

    if args.check:
        stale = []

        for path, wanted in ((FONT_OUT, font), (TABLE_OUT, table.encode()), (MANIFEST_OUT, manifest.encode())):
            current = path.read_bytes() if path.exists() else b""

            if current != wanted:
                stale.append(path.relative_to(ROOT))

        if stale:
            print("[!] 这些产物不是最新的，跑 `python3 tools/bake_icons.py`：")

            for path in stale:
                print(f"    {path}")

            return 1

        print(f"产物都是最新的（{len(entries)} 个图标，{len(font)} 字节）")
        return 0

    for path, payload in ((FONT_OUT, font), (TABLE_OUT, table.encode()), (MANIFEST_OUT, manifest.encode())):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(payload)

    if LICENCE.exists():
        shutil.copyfile(LICENCE, LICENCE_OUT)

    print(f"字体     {FONT_OUT.relative_to(ROOT)}  {len(font)} 字节  sha256 {hashlib.sha256(font).hexdigest()[:16]}…")
    print(f"表       {TABLE_OUT.relative_to(ROOT)}  {len(entries)} 个常量")
    print(f"清单     {MANIFEST_OUT.relative_to(ROOT)}")
    print(f"         {len(font) / len(entries):.0f} 字节/图标，家族名 {family!r}，upem {upem}")
    print()
    print("别忘了：换了这份字体，所有用它画的图标都会换一副样子。")

    return 0


def rust_manifest(source, entries, codes, axes, font, family, upem) -> str:
    """`fonts/MANIFEST.md`：这份子集是什么、怎么来的、数字是多少。"""
    lines = [
        "# The icon subset",
        "",
        "Generated by [`tools/bake_icons.py`](../tools/bake_icons.py) — do not edit by hand;",
        "rebake with `python3 tools/bake_icons.py`.",
        "",
        "## Where it came from",
        "",
        "* catalogue: [Google Material Symbols](https://fonts.google.com/icons), from",
        "  `google/material-design-icons` (`variablefont/`)",
        f"* source file: `fonts/source/{SOURCE.name}` — {len(source)} B,",
        f"  sha256 `{hashlib.sha256(source).hexdigest()}`",
        "* licence: **Apache-2.0** — see `LICENSE-APACHE-2.0.txt` in this directory",
        "",
        "## What was baked",
        "",
        f"* family: `{family}` — the name table is left exactly as upstream wrote it, so the",
        "  family does not change when an axis does",
        f"* axes pinned: {', '.join(f'`{tag}={value:g}`' for tag, value in axes.items())}",
        "  — iced has no way to set font variations per draw, so the axes are decided here",
        f"* units per em: {upem}, and every icon advances exactly one em — `size(24.0)` draws a",
        "  24×24 icon",
        f"* icons: **{len(entries)}** — everything in upstream's catalogue for that source file,",
        "  with nothing dropped. There is deliberately no list here: a list would be a copy of",
        "  upstream's `.codepoints`, which is not committed and would go stale by itself",
        "* layout tables dropped (`GSUB`/`GPOS`/`GDEF`): upstream uses them for the",
        "  type-the-name ligatures (`wifi` → the wifi icon), and icons are addressed by code point",
        f"* subset: **{len(font)} B**, sha256 `{hashlib.sha256(font).hexdigest()}`",
        "",
        "`src/table.rs` carries this font's checksum and the crate's tests recompute it, so a font",
        "regenerated without its table — or the other way round — is a red build.",
        "",
        "## Regenerating",
        "",
        "Re-download the upstream files into `fonts/source/` (the commands are in",
        "`tools/bake_icons.py`'s header), run `python3 tools/bake_icons.py`, and commit the font,",
        "`src/table.rs` and this file together.",
        "",
        "Changing an axis is not a rebuild: it is a different set of pictures for every icon, so it",
        "wants a look at the panel before it is committed. The family name does not change with the",
        "axes, which is what keeps call sites working — and the price is that only one cut can exist",
        "at a time.",
        "",
    ]

    return "\n".join(lines)


if __name__ == "__main__":
    sys.exit(main())
