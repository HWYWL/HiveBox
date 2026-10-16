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
        ├── SourceHanSansSC-Regular-Subset-common@24px.bin
        ├── MANIFEST.md                                ← 由本脚本生成
        └── card/                                      ← 字体包，进 SD 卡（见下方 `--card`）

# 首次使用：下载源字体

    curl -sSL -o source/source-han-sans-sc-regular.otf \
      https://github.com/adobe-fonts/source-han-sans/raw/release/OTF/SimplifiedChinese/SourceHanSansSC-Regular.otf

# 子集化（仅在字符集变更时需要；需要 fontTools）

    pyftsubset source/source-han-sans-sc-regular.otf \
        --text-file=source/charset-common.txt \
        --drop-tables+=DSIG \
        --output-file=dist/SourceHanSansSC-Regular-Subset.otf

# 烘培（子集化之后、或字号变更时需要重烘）

    python3 bake.py                        # 默认字号：24 px
    python3 bake.py --sizes 24 32          # 指定字号

# 卡上的字体包（让 SD 卡上的文件名、Wi-Fi 名不再画成方框）

    pyftsubset source/source-han-sans-sc-regular.otf \
        --text-file=source/charset-gb2312.txt \
        --drop-tables+=DSIG \
        --output-file=dist/SourceHanSansSC-Regular-Subset-gb2312.otf
    python3 bake.py \
        --source dist/SourceHanSansSC-Regular-Subset-gb2312.otf \
        --charset source/charset-gb2312.txt \
        --card v3

镜像里那份子集只有 528 个字，屏幕上的文件名与 Wi-Fi 名里用的是它没有的字，所以**换表救不了
它们**：表里的字形号是字体的字形号，设备端在排版那一步拿到的就已经是 .notdef 了。要让这些字
画出来，卡上得同时有更宽的**字体**和它的表 —— `--card` 就是按设备端认得的名字把这两个文件写进
`dist/card/`，拷进 SD 卡的 `/sdcard/fonts`（`updater::FONT_DIR`），重启生效。

字号清单要和设备端实际用到的字号一致：多烘只浪费 flash，少烘只是让那些字号退回在线绘制
（正确性不受影响）。`dist/MANIFEST.md` 记录当前烘了哪些。

界面只有一个字号（24 px，即 `pomelo-widgets` 的 `FONT_BASE`），所以只烘一档：查表时字号是
逐位比较的，烘了别的字号也没人读得到。

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

# 控制台未必是 UTF-8：Windows 的 PowerShell 默认 GBK，而「缺字」那一行会把字符集里字体
# 没有的字原样打出来 —— 那里面可能有 GBK 编不出来的 emoji，于是烘培成功了、脚本却抛
# UnicodeEncodeError 退出，看起来像失败。让它降级成 `?`，不要让它变成错误。
for _stream in (sys.stdout, sys.stderr):
    if hasattr(_stream, "reconfigure"):
        _stream.reconfigure(errors="replace")

ROOT = pathlib.Path(__file__).resolve().parent

# **从运行时那份子集烘**，不从 16.5 MB 的源字体烘。子集化会重排 glyph_id，所以表里的编号
# 必须与设备真正加载的那份字体一致；直接从子集烘就结构上不会错（也顺带让表头的哈希指的就是
# 运行时字体）。重新子集化之后必须重烘。
FONT = ROOT / "dist/SourceHanSansSC-Regular-Subset.otf"
CHARSET = ROOT / "source/charset-common.txt"
OUTPUT = ROOT / "dist"

# 要烘的字号：一个就够。
#
# 界面把正文字号统一收敛到 24 px（`pomelo-widgets` 的 `FONT_BASE`），四档可调的字号系统已经
# 删除，所以 18/20 两档不再有人要求。设备端查表时把字号逐位（f32 的位）比较，别的字号命中不了
# 任何表，就退回在线光栅化（约 1.6 ms/字形）并进 RAM 缓存 —— 慢，但不会画错；反过来，多烘一档
# 就是白占 flash：满字符集在 24 px 这一档已经要 ~2 MB（每字节约 0.93×字号² 字节）。
#
# 以后要加档位，先量体积，并且确认设备端真的会在这个字号上画字。
SIZES = [24]

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


def table_stem(source: pathlib.Path, charset: pathlib.Path) -> str:
    """表名里的字符集标记：`charset-common.txt` → `…-Subset-common`，`charset-gb2312.txt` → `…-gb2312`。

    名字里已经带着这个标记就不重复写：广字集的子集本来就叫
    `SourceHanSansSC-Regular-Subset-gb2312.otf`，再加一个 `-gb2312` 只会让人以为它是另一份东西。
    """
    tag = charset.stem.removeprefix("charset-")

    if source.stem.endswith(f"-{tag}"):
        return source.stem

    return f"{source.stem}-{tag}"


def manifest_name(args) -> str:
    """这份清单描述的是哪一次烘培：默认字符集仍叫 `MANIFEST.md`，别的字符集带上它的标记。

    一个文件写不下两次结果，而后烘的那一次会静静地把先烘的那一次的记录覆盖掉 —— 于是仓库里
    写着的那份数字就不再是固件里那份表了。名字分开，两次烘培就都留在 `dist/` 里。
    """
    tag = args.charset.stem.removeprefix("charset-")

    return "MANIFEST.md" if tag == CHARSET.stem.removeprefix("charset-") else f"MANIFEST-{tag}.md"


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

    written = args.output / manifest_name(args)
    written.write_text("\n".join(lines), encoding="utf-8")
    print(f"\n写入 {written}")


# 卡上一个「字体包」的名字：字体与它的表同名版本，设备端（`updater::card_font`）就是靠这个认出
# 两半是一份东西的。名字是协议的一部分 —— 改了这里就要改 `pomelo-apps/updater`。
CARD_FACE = "hivebox-face-{version}.otf"
CARD_TABLE = "hivebox-font-24px-{version}.bin"
# 设备端读卡上字体的目录，即 `updater::FONT_DIR`。
CARD_DIR = "/sdcard/fonts"


def write_card_package(args, manifest: list) -> None:
    """把这一份字体与它的表按卡认得的名字另存一份，写进 `dist/card/`。

    为什么要有这一步：表里的字形号是**字体**的字形号，所以一张表只对烘它的那份字体有意义。
    镜像里那份子集只有 528 个字，SD 卡上的文件名、Wi-Fi 名里用的字大多不在其中 —— 而那些字
    光靠换表是画不出来的：设备端在排版那一步拿到的就已经是 .notdef 了，表里再怎么全也没有
    它的编号。所以要让卡上的字画出来，卡上得同时有字体和它的表。
    """
    if 24 not in args.sizes:
        raise SystemExit("[!] --card 需要 24 px 那一档：设备端只查这一档")

    version = args.card
    card = args.output / "card"
    card.mkdir(parents=True, exist_ok=True)

    face = card / CARD_FACE.format(version=version)
    table = card / CARD_TABLE.format(version=version)

    face.write_bytes(args.source.read_bytes())

    for name, stats in manifest:
        if stats["size"] == 24:
            table.write_bytes((args.output / name).read_bytes())
            break
    else:
        raise SystemExit("[!] 这一批里没有 24 px 的表")

    print(f"\n卡上的字体包 {version}（两个文件一起放进 SD 卡的 {CARD_DIR}）：")
    print(f"  {face.name}  {face.stat().st_size / 1e6:.2f} MB")
    print(f"  {table.name}  {table.stat().st_size / 1e6:.2f} MB")


def main() -> int:
    parser = argparse.ArgumentParser(description="把矢量字体烘成位图字形表")
    parser.add_argument("--sizes", type=int, nargs="+", default=SIZES)
    parser.add_argument("--source", type=pathlib.Path, default=FONT,
                        help="运行时子集字体路径（默认：dist/SourceHanSansSC-Regular-Subset.otf）")
    parser.add_argument("--charset", type=pathlib.Path, default=CHARSET,
                        help="字符集文件路径（默认：source/charset-common.txt）")
    parser.add_argument("--output", type=pathlib.Path, default=OUTPUT,
                        help="输出目录（默认：dist/）")
    parser.add_argument("--card", metavar="VERSION", default=None,
                        help="另外按卡上的名字存一份字体包（字体 + 表，放 <输出目录>/card/）")
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
        name = f"{table_stem(args.source, args.charset)}@{size}px.bin"
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

    if args.card:
        write_card_package(args, manifest)

    return 0


if __name__ == "__main__":
    sys.exit(main())
