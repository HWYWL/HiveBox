#!/usr/bin/env python3
"""
bake.py — 把矢量字体烘成位图字形表，供 iced-pomelo-winit 在嵌入式端使用。

为什么要有这个工具：ESP32-S3 上现场光栅化一个字形要 **约 1.6 ms**（实测：打开 Settings
页 147 个字形 = 238 ms，占那一帧 400 ms 的六成）。这些字形是固定的、字号是固定的，所以
唯一合理的做法是在构建机上把它们烘好，设备上只做「查表 + 贴掩码」。

这个工具**不参与排版**：advance、kerning、断行都由 cosmic-text 在设备上做（因此字体文件
仍然要留在设备上）。烘出来的只有「像素覆盖」，也就是 `blit_mask` 需要的那个字节数组。

# 目录结构

    pomelo-font/
    ├── bake.py          ← 本脚本
    ├── requirements.txt ← pip install -r requirements.txt
    ├── source/
    │   ├── source-han-sans-sc-regular.otf  ← 源字体（不进仓库，见下方下载命令）
    │   └── charset-common.txt              ← 字符集定义
    └── dist/
        ├── SourceHanSansSC-Regular-Subset.otf         ← 子集（进固件）
        ├── SourceHanSansSC-Regular-Subset-common@14px.bin
        ├── SourceHanSansSC-Regular-Subset-common@15px.bin
        ├── SourceHanSansSC-Regular-Subset-common@18px.bin
        └── MANIFEST.md                                ← 由本脚本生成

# 首次使用：下载源字体

    curl -sSL -o source/source-han-sans-sc-regular.otf \
      https://github.com/adobe-fonts/source-han-sans/raw/release/OTF/SimplifiedChinese/SourceHanSansSC-Regular.otf

# 子集化（仅在字符集变更时需要；需要 fontTools）

    pyftsubset source/source-han-sans-sc-regular.otf \
        --text-file=source/charset-common.txt \
        --drop-tables+=DSIG \
        --output-file=dist/SourceHanSansSC-Regular-Subset.otf

# 烘培（子集化之后、或字号变更时需要重烘）

    python3 bake.py                        # 默认字号：18、20、24 px
    python3 bake.py --sizes 14 15 18 21   # 指定字号

字号清单要和设备端实际用到的字号一致：多烘只浪费 flash，少烘只是让那些字号退回在线绘制
（正确性不受影响）。`dist/MANIFEST.md` 记录当前烘了哪些。

# 输出格式（PGLY v1，小端）

表头 40 字节：
    0   magic           4    b"PGLY"
    4   version         u16  1
    6   flags           u16  0
    8   font_size_bits  u32  字号 f32 的位模式，与 cosmic-text 的 CacheKey::font_size_bits 逐位比较
    12  font_hash       16   源字体 SHA-256 前 16 字节
    28  glyph_count     u32
    32  coverage_len    u32
    36  reserved        u32  0

字形记录（12 字节 × glyph_count，按 glyph_id 升序）：
    0   glyph_id    u16
    2   width       u8
    3   height      u8
    4   left        i8
    5   top         i8
    6   reserved    u8
    7   reserved    u8
    8   offset      u32  在覆盖池里的字节偏移

覆盖池：每个字形 width × height 字节，行主序，8 位 alpha。
"""

import argparse
import hashlib
import pathlib
import struct
import sys

try:
    import freetype
except ImportError:
    print("[!] 需要 freetype-py：pip install freetype-py")
    sys.exit(1)

ROOT = pathlib.Path(__file__).resolve().parent

# **从运行时那份子集烘**，不从 16.5 MB 的源字体烘。子集化会重排 glyph_id，所以表里的编号
# 必须与设备真正加载的那份字体一致；直接从子集烘就结构上不会错（也顺带让表头的哈希指的就是
# 运行时字体）。重新子集化之后必须重烘。
FONT = ROOT / "dist/SourceHanSansSC-Regular-Subset.otf"
CHARSET = ROOT / "source/charset-common.txt"
OUTPUT = ROOT / "dist"

# 要烘的字号。app 里实际出现的字号共 13 档（12/13/14/15/16/17/18/20/22/32/34/38/96），
# 全烘不可能：满字符集（4008 字）在 96px 一档就要 34 MB，六档加起来 48 MB。
#
# 所以分两层：
#   * **正文档**烘满字符集 —— 任意中文都可能出现在这些字号上：
#     14 = 设置/计算器/播放器的次要与数值文本，15 = 启动器标签与状态栏，
#     18 = 终端正文；
#   * **大字号档**（20/22/32/34/38/96）只出现在短串上（标题、按键、数字），
#     按需在线绘制：一屏不过几十个新字形，几十毫秒一次，之后进缓存。
#
# 以后要加档位，先量体积（每字节约 0.93×字号² 字节）再决定烘哪几档。
SIZES = [18, 20, 24]

MAGIC = b"PGLY"
VERSION = 1

# 表头：magic / version / flags / size_bits / font_hash(16) / count / coverage_len / reserved
HEADER = struct.Struct("<4sHHI16sIII")
# 字形记录：glyph_id / width / height / left / top / 2×reserved / offset（12 字节）
RECORD = struct.Struct("<HBBbbBBI")

assert HEADER.size == 40 and RECORD.size == 12


def bake_size_record(glyph_id: int, width: int, height: int, left: int, top: int, offset: int) -> bytes:
    return RECORD.pack(
        glyph_id,
        width & 0xFF,
        height & 0xFF,
        max(-128, min(127, left)),
        max(-128, min(127, top)),
        0,
        0,
        offset,
    )


def load_charset(path: pathlib.Path) -> list[str]:
    text = path.read_text(encoding="utf-8")

    # 一行放得下，但允许将来改成一行一个字符，也允许整行注释（不能按行内 # 切，ASCII 的
    # `#` 本身就是字符集里的一个字符）。
    #
    # **只跳控制字符**：空格与全角空格是真正的字形，它们的 advance 与 .notdef 差很远
    # （224 对 1000），跳掉它们等于把英文里每个空格都撑宽四倍。
    chars = []
    seen = set()

    for line in text.splitlines():
        if line.lstrip().startswith("#"):
            continue

        for ch in line:
            if ord(ch) < 0x20 or ch in seen:
                continue

            seen.add(ch)
            chars.append(ch)

    return chars


def bake(face: freetype.Face, ch: str) -> tuple[int, bytes, int, int, int, int] | None:
    """光栅化一个字符，返回 (glyph_id, 覆盖, 宽, 高, left, top)。"""
    glyph_id = face.get_char_index(ch)

    if glyph_id == 0:
        return None

    # 与 swash 保持一致的画法：**不做 hinting**（设备端在线绘制也是不 hinting 的），
    # 逐像素对齐（亚像素分箱不使用），256 级灰度。
    face.load_char(ch, freetype.FT_LOAD_RENDER | freetype.FT_LOAD_NO_HINTING)

    bitmap = face.glyph.bitmap
    width, height = bitmap.width, bitmap.rows

    if bitmap.pixel_mode != freetype.FT_PIXEL_MODE_GRAY:
        raise SystemExit(f"[!] {ch!r} 的位图不是 8 位灰度（pixel_mode={bitmap.pixel_mode}）")

    # 位图行间可能有 pitch 填充，逐行取，别把填充当像素。
    raw = bitmap.buffer
    pitch = bitmap.pitch
    coverage = (
        bytes(raw[: width * height])
        if pitch == width
        else b"".join(bytes(raw[row * pitch : row * pitch + width]) for row in range(height))
    )

    left, top = face.glyph.bitmap_left, face.glyph.bitmap_top

    # 记录里的 left/top 是有符号 8 位。越界就报错而不是截断：截断会让字形整体错位，
    # 而错位比崩溃难查得多。
    if not (-128 <= left <= 127 and -128 <= top <= 127):
        raise SystemExit(
            f"[!] {ch!r} 的墨迹偏移越出 i8：left={left} top={top}"
        )

    return (glyph_id, coverage, width, height, left, top)


def bake_size(size: int, face: freetype.Face, chars: list[str], source: bytes) -> tuple[bytes, dict]:
    face.set_pixel_sizes(size, size)

    glyphs: dict[int, tuple[bytes, int, int, int, int]] = {}
    missing = []

    for ch in chars:
        result = bake(face, ch)

        if result is None:
            missing.append(ch)
            continue

        glyph_id, coverage, width, height, left, top = result

        # 多个码位可能映到同一个字形（例：全角与半角、兼容形式），按 glyph_id 去重。
        glyphs.setdefault(glyph_id, (coverage, width, height, left, top))

    pool = bytearray()
    records = bytearray()

    for glyph_id in sorted(glyphs):
        coverage, width, height, left, top = glyphs[glyph_id]
        offset = len(pool)
        pool += coverage
        records += bake_size_record(glyph_id, width, height, left, top, offset)

    size_bits = struct.unpack("<I", struct.pack("<f", float(size)))[0]
    table = (
        HEADER.pack(MAGIC, VERSION, 0, size_bits, hashlib.sha256(source).digest()[:16], len(glyphs), len(pool), 0)
        + bytes(records)
        + bytes(pool)
    )

    stats = {
        "size": size,
        "glyphs": len(glyphs),
        "missing": missing,
        "coverage": len(pool),
        "table": len(table),
        "per_glyph": len(pool) / max(1, len(glyphs)),
    }

    return table, stats


def shown(path: pathlib.Path) -> str:
    """A path as the manifest should print it: relative when inside this repo, absolute otherwise."""
    try:
        return str(path.relative_to(ROOT))
    except ValueError:
        return str(path)


def write_manifest(args, chars: list[str], manifest: list, source: bytes) -> None:
    """把「这批表是怎么来的」写成文件，跟产物放在一起。"""
    digest = hashlib.sha256(source).hexdigest()
    lines = [
        "# Baked glyph tables",
        "",
        "Generated by [`bake.py`](../bake.py) — do not edit by hand;",
        "rebake with `python3 bake.py` after changing the charset or the sizes.",
        "",
        "## What was baked",
        "",
        f"* source font: `{shown(args.source)}` — {len(source)} B, sha256 `{digest}`",
        f"* charset: `{shown(args.charset)}` — {len(chars)} characters",
        "* coverage: 8-bit alpha, no hinting, pixel-aligned (subpixel bins unused)",
        "",
        "| table | size | glyphs | coverage | file | bytes/glyph |",
        "| --- | ---: | ---: | ---: | ---: | ---: |",
    ]

    for name, stats in manifest:
        lines.append(
            f"| `{name}` | {stats['size']} px | {stats['glyphs']} | "
            f"{stats['coverage'] / 1024:.1f} KiB | {stats['table'] / 1024:.1f} KiB | "
            f"{stats['per_glyph']:.0f} B |"
        )

    lines += [
        "",
        "## Which sizes are cached, and what happens without one",
        "",
        "The table above is the whole list. Any other size — and any glyph the source font does not",
        "have — is rasterised on the device the first time it is drawn (about 1.6 ms a glyph on the",
        "ESP32-S3) and cached in RAM after that. **Nothing is missing and nothing is wrong**; a size",
        "that is not here costs one slow frame per screen, per size.",
        "",
        "So adding a size to an app is allowed and does not break anything. It is worth baking",
        "when that size carries a lot of text.",
        "",
    ]

    (args.output / "MANIFEST.md").write_text("\n".join(lines), encoding="utf-8")
    print(f"\n写入 {args.output / 'MANIFEST.md'}")


def main() -> int:
    parser = argparse.ArgumentParser(description="把矢量字体烘成位图字形表")
    parser.add_argument("--sizes", type=int, nargs="+", default=SIZES)
    parser.add_argument("--source", type=pathlib.Path, default=FONT,
                        help="运行时子集字体路径（默认：dist/SourceHanSansSC-Regular-Subset.otf）")
    parser.add_argument("--charset", type=pathlib.Path, default=CHARSET,
                        help="字符集文件路径（默认：source/charset-common.txt）")
    parser.add_argument("--output", type=pathlib.Path, default=OUTPUT,
                        help="输出目录（默认：dist/）")
    args = parser.parse_args()

    if not args.source.exists():
        print(f"[!] 缺少运行时字体：{args.source}")
        print("    先子集化源字体（需要 fontTools）：")
        print("      pyftsubset source/source-han-sans-sc-regular.otf \\")
        print("        --text-file=source/charset-common.txt \\")
        print("        --drop-tables+=DSIG \\")
        print("        --output-file=dist/SourceHanSansSC-Regular-Subset.otf")
        print()
        print("    或先下载源字体：")
        print("      curl -sSL -o source/source-han-sans-sc-regular.otf \\")
        print("        https://github.com/adobe-fonts/source-han-sans/raw/release/OTF/SimplifiedChinese/SourceHanSansSC-Regular.otf")
        return 1

    source = args.source.read_bytes()
    chars = load_charset(args.charset)
    face = freetype.Face(str(args.source))

    # 哨兵：字体与字符集里都必须有空格。少了它，U+0020 会落到 .notdef，而 .notdef 的 advance
    # 是 1000/1000 em（Source Han Sans 里真正的空格是 224）—— 英文里每个空格都宽四倍。
    sentinels = (" ", "　", "0", "A")
    absent = [ch for ch in sentinels if face.get_char_index(ch) == 0]

    if absent:
        raise SystemExit(
            f"[!] 运行时字体缺少这些基础字符：{''.join(absent)!r} —— 先重做子集"
        )

    dropped = [ch for ch in sentinels if ch not in chars]

    if dropped:
        raise SystemExit(
            f"[!] 字符集里少了这些基础字符：{''.join(dropped)!r} —— 检查 {args.charset.name}"
        )

    print(f"源字体   {args.source.name}  {len(source) / 1e6:.2f} MB  sha256 {hashlib.sha256(source).hexdigest()[:16]}…")
    print(f"字符集   {args.charset.name}  {len(chars)} 字")
    print(f"输出     {args.output}/  （字号 {', '.join(str(s) for s in args.sizes)} px）")
    print()

    args.output.mkdir(parents=True, exist_ok=True)
    manifest = []

    for size in args.sizes:
        table, stats = bake_size(size, face, chars, source)
        name = f"{args.source.stem}-common@{size}px.bin"
        (args.output / name).write_bytes(table)

        print(
            f"{name:<52} {stats['table'] / 1024:>7.1f} KB   "
            f"{stats['glyphs']} 字形  每字 {stats['per_glyph']:>5.0f} B"
            + (f"  缺 {len(stats['missing'])} 字" if stats["missing"] else "")
        )

        manifest.append((name, stats))

    print()
    print("缺字（字体里没有，设备端会走在线绘制）：")
    for name, stats in manifest:
        if stats["missing"]:
            print(f"  {name}: {''.join(stats['missing'])}")

    write_manifest(args, chars, manifest, source)

    return 0


if __name__ == "__main__":
    sys.exit(main())
