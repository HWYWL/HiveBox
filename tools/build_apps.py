#!/usr/bin/env python3
"""Build the applications that are packages, and put them in `bin/`.

An application is a wasm module plus a metadata document plus an optional icon, and this script is
the only place that knows how those three become a `.hbapp` file. The container is deliberately
simple — a fixed 32-byte header and four length-prefixed sections — because both sides of it are
written here: this script packs it and `pomelo-apps/app-abi` reads it.

Usage:
    python tools/build_apps.py                 # build every package, write bin/
    python tools/build_apps.py --only clock    # one package
    python tools/build_apps.py --dry-run       # say what would be built
    python tools/build_apps.py --out ../dist   # somewhere other than bin/

The manifest fragment written to `bin/apps.json` is what `publish_bin.py --apps` merges into
`manifest.json`: name, size and SHA-256 of each file, read off the file that was just written.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import struct
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
APPS = ROOT / "pomelo-apps"
TARGET = "wasm32-unknown-unknown"

MAGIC = b"HBAP"
FORMAT = 1
ABI_MAJOR = 1
ABI_MINOR = 0
HEADER_LEN = 32
MAX_PACKAGE = 2 * 1024 * 1024


@dataclass
class App:
    """One package: which crate builds it and what it says about itself."""

    id: str
    crate_name: str
    name: str
    name_zh: str
    version: str
    accent: str
    description: str
    memory_kb: int = 256
    permissions: int = 0
    glyph: str = ""
    icon: Path | None = None
    extra: dict = field(default_factory=dict)

    def metadata(self) -> dict:
        meta = {
            "id": self.id,
            "name": self.name,
            "name_zh": self.name_zh,
            "version": self.version,
            "abi": ABI_MAJOR,
            "memory_kb": self.memory_kb,
            "permissions": self.permissions,
            "accent": self.accent,
            "description": self.description,
        }

        if self.glyph:
            meta["glyph"] = self.glyph

        meta.update(self.extra)
        return meta

    def file_name(self) -> str:
        return f"hivebox-app-{self.id}-{self.version}.hbapp"


# The packages this project publishes. A package that is not in this table is not built, which is the
# same rule the release script follows: what is listed is what exists.
CATALOGUE = [
    App(
        id="calculator",
        crate_name="calculator-wasm",
        name="Calculator",
        name_zh="计算器",
        version="v0.1.0",
        accent="#F69E22",
        description="四则运算，键位与内置版一致",
        memory_kb=256,
        glyph="calculate",
    ),
    App(
        id="clock",
        crate_name="clock-wasm",
        name="Clock",
        name_zh="时钟",
        version="v0.1.0",
        accent="#D63384",
        description="时间来自宿主时钟，含开机时长",
        memory_kb=256,
        glyph="schedule",
    ),
]


def cargo_build(app: App, release: bool) -> Path:
    """Builds one crate for wasm and returns the module's path."""
    command = [
        "cargo",
        "build",
        "-p",
        app.crate_name,
        "--target",
        TARGET,
    ]

    if release:
        command.append("--release")

    print(f"  构建 {app.crate_name} -> {TARGET}")
    result = subprocess.run(command, cwd=APPS, text=True)
    if result.returncode != 0:
        raise SystemExit(f"cargo 构建 {app.crate_name} 失败（退出码 {result.returncode}）")

    profile = "release" if release else "debug"
    wasm = APPS / "target" / TARGET / profile / f"{app.crate_name.replace('-', '_')}.wasm"

    if not wasm.exists():
        raise SystemExit(f"找不到 {wasm}；crate 的 [lib] crate-type 是否为 cdylib？")

    return wasm


def encode_icon(path: Path | None) -> bytes:
    """Turns a 96×96 raw icon into the section the tile reads.

    The input is a pair of files beside each other — `<name>.rgb565` (18432 bytes) and
    `<name>.alpha` (9216 bytes) — because converting a PNG would mean an image library here and a
    decoder on the board, and neither is worth it for a tile. No icon is a valid answer: the
    launcher draws the package's accent with the first character of its name.
    """
    if path is None:
        return b""

    rgb = path.with_suffix(".rgb565").read_bytes()
    alpha = path.with_suffix(".alpha").read_bytes()
    width = height = 96

    if len(rgb) != width * height * 2 or len(alpha) != width * height:
        raise SystemExit(f"{path}: 图标必须是 {width}×{height} 的 RGB565 + alpha 两个文件")

    return struct.pack("<HH", width, height) + rgb + alpha


def pack(app: App, wasm: bytes, icon: bytes) -> bytes:
    """The container: 32-byte header, then metadata, module, icon, resources."""
    meta = json.dumps(app.metadata(), ensure_ascii=False, separators=(",", ":")).encode("utf-8")

    if len(meta) > 8 * 1024:
        raise SystemExit(f"{app.id}: metadata 超过 8 KB")

    total = HEADER_LEN + len(meta) + len(wasm) + len(icon)
    if total > MAX_PACKAGE:
        raise SystemExit(f"{app.id}: 包体 {total} 字节，超过 {MAX_PACKAGE} 上限")

    header = MAGIC
    header += struct.pack("<HHHH", FORMAT, ABI_MAJOR, ABI_MINOR, 0)
    header += struct.pack("<IIII", len(meta), len(wasm), len(icon), 0)
    header += b"\0\0\0\0"
    assert len(header) == HEADER_LEN

    return header + meta + wasm + icon


def main() -> int:
    parser = argparse.ArgumentParser(description="构建 .hbapp 应用包")
    parser.add_argument("--only", action="append", default=[], help="只构建指定 id，可重复")
    parser.add_argument("--out", default=str(ROOT / "bin"), help="输出目录（默认 bin/）")
    parser.add_argument("--debug", action="store_true", help="用 debug 配置构建")
    parser.add_argument("--dry-run", action="store_true", help="只打印将要做什么")
    args = parser.parse_args()

    out = Path(args.out).resolve()
    selected = [app for app in CATALOGUE if not args.only or app.id in args.only]

    if not selected:
        raise SystemExit(f"没有匹配的应用：{', '.join(args.only)}")

    out.mkdir(parents=True, exist_ok=True)
    entries = []

    for app in selected:
        print(f"[{app.id}] {app.name_zh} {app.version}")

        if args.dry_run:
            print(f"  将会写出 {out / app.file_name()}")
            continue

        wasm_path = cargo_build(app, release=not args.debug)
        wasm = wasm_path.read_bytes()
        icon = encode_icon(app.icon)
        package = pack(app, wasm, icon)

        target = out / app.file_name()
        target.write_bytes(package)

        digest = hashlib.sha256(package).hexdigest()

        entries.append(
            {
                "id": app.id,
                "name": app.name,
                "name_zh": app.name_zh,
                "version": app.version,
                "file": app.file_name(),
                "size": len(package),
                "sha256": digest,
                "abi": ABI_MAJOR,
                "accent": app.accent,
                "description": app.description,
                "glyph": app.glyph,
                "permissions": app.permissions,
            }
        )

        print(
            f"  写出 {target.name}：{len(package):,} 字节"
            f"（模块 {len(wasm):,}，说明 {len(package) - len(wasm) - HEADER_LEN - len(icon):,}）"
        )

    if args.dry_run:
        return 0

    fragment = out / "apps.json"
    fragment.write_text(
        json.dumps({"apps": entries}, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    print(f"清单片段 {fragment}：{len(entries)} 个应用")

    return 0


if __name__ == "__main__":
    sys.exit(main())
