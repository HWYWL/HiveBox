#!/usr/bin/env python3
"""Build the applications that are packages, and put them in `bin/`.

An application is a wasm module plus a metadata document plus an optional icon, and this script is
the only place that knows how those three become a `.app` file. The container is deliberately
simple — a fixed 32-byte header and four length-prefixed sections — because both sides of it are
written here: this script packs it and `pomelo-apps/app-abi` reads it.

Usage:
    python tools/build_apps.py                 # build every package, write bin/apps/
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
# The highest service ABI the SDK in this tree speaks. Kept in step with `app-abi`'s `ABI_MINOR`:
# the header says what the package was built against, and the runtime refuses a package whose minor
# is newer than its own.
ABI_MINOR = 7
HEADER_LEN = 32
MAX_PACKAGE = 2 * 1024 * 1024

# The permission bits, by the name the ABI uses. Spelled out here rather than imported, because this
# script runs before a build and the table is what turns "the module imports this" into "the package
# must declare that".
PERMISSIONS = {
    "NETWORK": 1,
    "SYSTEM": 2,
    "AUDIO": 4,
    "NAS": 8,
    "WIFI": 16,
    "CLOUD": 32,
    "FILES": 64,
    "WEB": 128,
}

# Which import needs which bit. Deliberately exhaustive over the `host` module: an import that is
# not listed is one the host does not gate (time, text metrics, preferences, the private files, the
# log, and the frame loop), while an import that is listed but missing from this table is a hole a
# test would not see. `hb_file_*` is not here: those already live inside the package's directory, and
# `FILES` is for reading beyond it.
IMPORT_PERMISSIONS = {
    "hb_http_start": "NETWORK",
    "hb_http_state": "NETWORK",
    "hb_http_fault": "NETWORK",
    "hb_http_progress": "NETWORK",
    "hb_http_take": "NETWORK",
    "hb_http_cancel": "NETWORK",
    "hb_system": "SYSTEM",
    "hb_firmware_version": "SYSTEM",
    "hb_audio_play": "AUDIO",
    "hb_audio_control": "AUDIO",
    "hb_audio_state": "AUDIO",
    "hb_wifi_status": "WIFI",
    "hb_wifi_scan_start": "WIFI",
    "hb_wifi_scan_state": "WIFI",
    "hb_wifi_scan_count": "WIFI",
    "hb_wifi_scan_ap": "WIFI",
    "hb_wifi_connect": "WIFI",
    "hb_nas_snapshot": "NAS",
    "hb_nas_disks": "NAS",
    "hb_nas_refresh": "NAS",
    "hb_nas_shell_open": "NAS",
    "hb_nas_shell_write": "NAS",
    "hb_nas_shell_status": "NAS",
    "hb_nas_shell_fault": "NAS",
    "hb_nas_shell_take": "NAS",
    "hb_nas_shell_close": "NAS",
    "hb_cloud_key": "CLOUD",
    "hb_cloud_start": "CLOUD",
    "hb_cloud_state": "CLOUD",
    "hb_cloud_take": "CLOUD",
    "hb_cloud_cancel": "CLOUD",
}

PERMISSION_NAMES = {
    "NETWORK": "网络",
    "SYSTEM": "系统",
    "AUDIO": "音频",
    "NAS": "SSH",
    "WIFI": "Wi-Fi",
    "CLOUD": "云服务",
    "FILES": "文件",
}


def _uleb(data: bytes, at: int) -> tuple[int, int]:
    """Reads one unsigned LEB128 and returns it with the position after it."""
    result = 0
    shift = 0

    while True:
        if at >= len(data):
            raise SystemExit("wasm 在 LEB128 中途结束")

        byte = data[at]
        at += 1
        result |= (byte & 0x7F) << shift

        if not byte & 0x80:
            return result, at

        shift += 7


def _name(data: bytes, at: int) -> tuple[str, int]:
    """Reads a wasm name: a length and that many bytes of UTF-8."""
    length, at = _uleb(data, at)
    end = at + length

    if end > len(data):
        raise SystemExit("wasm 名称段越界")

    return data[at:end].decode("utf-8", "replace"), end


def wasm_imports(wasm: bytes) -> list[tuple[str, str]]:
    """Every `(module, name)` the module imports, in order.

    Only the import section is decoded, and only far enough to read the two names: the descriptors
    are skipped by their own grammar, because a package built for the wrong ABI is a mistake this
    script should report rather than a wasm parser it should carry.
    """
    if wasm[:4] != b"\0asm":
        raise SystemExit("模块不是 wasm（缺少 \\0asm）")

    at = 8
    imports: list[tuple[str, str]] = []

    while at < len(wasm):
        section = wasm[at]
        at += 1
        size, at = _uleb(wasm, at)
        end = at + size

        if end > len(wasm):
            raise SystemExit("wasm 段长度越界")

        if section == 2:
            count, at = _uleb(wasm, at)

            for _ in range(count):
                module, at = _name(wasm, at)
                name, at = _name(wasm, at)
                kind = wasm[at]
                at += 1
                imports.append((module, name))

                if kind == 0:  # function
                    _, at = _uleb(wasm, at)
                elif kind == 1:  # table
                    at += 1  # reference type
                    flags = wasm[at]
                    at += 1
                    _, at = _uleb(wasm, at)
                    if flags & 1:
                        _, at = _uleb(wasm, at)
                elif kind == 2:  # memory
                    flags = wasm[at]
                    at += 1
                    _, at = _uleb(wasm, at)
                    if flags & 1:
                        _, at = _uleb(wasm, at)
                elif kind == 3:  # global
                    at += 2
                elif kind == 4:  # tag
                    at += 1
                    _, at = _uleb(wasm, at)
                else:
                    raise SystemExit(f"未知的 wasm import 种类 {kind}")

                if at > end:
                    raise SystemExit("wasm import 段越界")

        at = end

    return imports


def check_permissions(app: "App", wasm: bytes) -> None:
    """Fails the build when the module asks for a service the metadata did not declare.

    This is the one permission mistake that cannot be fixed at run time: the host grants the
    package's bits at instantiation, so a missing bit is a call that always returns `DENIED`. Better
    to learn it here than from an application's "not available" screen.
    """
    imports = wasm_imports(wasm)

    foreign = sorted({module for module, _ in imports if module != "host"})
    if foreign:
        raise SystemExit(f"{app.id}: 模块从 {', '.join(foreign)} 导入，宿主只提供 host 模块")

    missing: dict[str, list[str]] = {}
    for _, name in imports:
        permission = IMPORT_PERMISSIONS.get(name)

        if permission and not app.permissions & PERMISSIONS[permission]:
            missing.setdefault(permission, []).append(name)

    if missing:
        wanted = "、".join(
            f"{PERMISSION_NAMES[permission]}（{permission}）" for permission in missing
        )
        detail = "; ".join(f"{permission}: {', '.join(names)}" for permission, names in missing.items())
        raise SystemExit(
            f"{app.id}: 模块导入了需要 {wanted} 的服务，但清单的 permissions 是 "
            f"{app.permissions}（{detail}）"
        )


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
        return f"hivebox-app-{self.id}-{self.version}.app"


# The packages this project publishes. A package that is not in this table is not built, which is the
# same rule the release script follows: what is listed is what exists.
CATALOGUE = [
    App(
        id="calculator",
        crate_name="calculator-wasm",
        name="Calculator",
        name_zh="计算器",
        version="v0.2.1",
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
        version="v0.2.1",
        accent="#D63384",
        description="时间来自宿主时钟，含开机时长",
        memory_kb=256,
        glyph="schedule",
    ),
    App(
        id="games",
        crate_name="games-wasm",
        name="Games",
        name_zh="游戏",
        version="v0.2.1",
        accent="#FBC02D",
        description="2048：四个方向键，一局不留痕",
        memory_kb=256,
        glyph="sports_esports",
    ),
    App(
        id="power-log",
        crate_name="power-log-wasm",
        name="Power Log",
        name_zh="电量日志",
        version="v0.2.1",
        accent="#AA60D6",
        description="三十秒一个采样，画一小时的电量曲线",
        memory_kb=256,
        permissions=PERMISSIONS["SYSTEM"],
        glyph="insights",
    ),
    App(
        id="task-viewer",
        crate_name="task-viewer-wasm",
        name="Tasks",
        name_zh="任务详情",
        version="v0.2.1",
        accent="#00A8BE",
        description="内存、外部内存、存储与开机时长，画成 meter",
        memory_kb=256,
        permissions=PERMISSIONS["SYSTEM"],
        glyph="monitor_heart",
    ),
    App(
        id="wifi-map",
        crate_name="wifi-map-wasm",
        name="Wi-Fi Map",
        name_zh="信道地图",
        version="v0.2.1",
        accent="#00897B",
        description="十三条信道各挤了多少个网络",
        memory_kb=256,
        permissions=PERMISSIONS["WIFI"],
        glyph="wifi_find",
    ),
    App(
        id="weather",
        crate_name="weather-wasm",
        name="Weather",
        name_zh="天气",
        version="v0.2.1",
        accent="#0288D1",
        description="ip-api 定位，Open-Meteo 五天预报",
        memory_kb=512,
        permissions=PERMISSIONS["NETWORK"],
        glyph="partly_cloudy_day",
    ),
    App(
        id="nas-monitor",
        crate_name="nas-monitor-wasm",
        name="Server",
        name_zh="服务器",
        version="v0.2.5",
        accent="#705CF0",
        description="CPU、温度、内存、网络与磁盘，读数来自固件",
        memory_kb=256,
        permissions=PERMISSIONS["NAS"],
        glyph="dns",
    ),
    App(
        id="cloud-monitor",
        crate_name="cloud-monitor-wasm",
        name="Cloud",
        name_zh="云服务",
        version="v0.2.1",
        accent="#5B8FB0",
        description="列出一个账号的机器，并开关机或重启",
        memory_kb=512,
        permissions=PERMISSIONS["CLOUD"],
        glyph="cloud_sync",
    ),
    App(
        id="files",
        crate_name="files-wasm",
        name="Files",
        name_zh="文件管理",
        version="v0.2.1",
        accent="#A66F4A",
        description="两个卷里的目录：进去、退出来、删除",
        memory_kb=512,
        permissions=PERMISSIONS["FILES"],
        glyph="folder_open",
    ),
    App(
        id="music-player",
        crate_name="music-player-wasm",
        name="Music",
        name_zh="音乐",
        version="v0.2.1",
        accent="#E94560",
        description="卡上 music 目录里的曲目，解码在固件那一侧",
        memory_kb=512,
        permissions=PERMISSIONS["FILES"] | PERMISSIONS["AUDIO"],
        glyph="music_note",
    ),
    App(
        id="web-manager",
        crate_name="web-manager-wasm",
        name="Web Manager",
        name_zh="网页管理",
        version="v0.2.1",
        accent="#2384F5",
        description="板子那个 HTTP 服务的开关与地址",
        memory_kb=256,
        permissions=PERMISSIONS["WEB"],
        glyph="public",
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
    parser = argparse.ArgumentParser(description="构建 .app 应用包")
    parser.add_argument("--only", action="append", default=[], help="只构建指定 id，可重复")
    parser.add_argument("--out", default=str(ROOT / "bin"), help="仓库的 bin/ 目录（默认）")
    parser.add_argument("--debug", action="store_true", help="用 debug 配置构建")
    parser.add_argument("--dry-run", action="store_true", help="只打印将要做什么")
    args = parser.parse_args()

    # 包不落在 `bin/` 本身，而落在它的 `apps/` 下：`bin/` 里还躺着合并镜像、OTA 镜像和字库表，
    # 十几个应用平铺在它们旁边是这个目录读不懂的样子。清单片段留在 `bin/`（`publish_bin.py
    # --apps` 就在那里找它），下载地址里的那一段目录由 `release::APPS_DIR` 加。
    out = Path(args.out).resolve()
    packages = out / "apps"
    selected = [app for app in CATALOGUE if not args.only or app.id in args.only]

    if not selected:
        raise SystemExit(f"没有匹配的应用：{', '.join(args.only)}")

    packages.mkdir(parents=True, exist_ok=True)
    entries = []

    for app in selected:
        print(f"[{app.id}] {app.name_zh} {app.version}")

        if args.dry_run:
            print(f"  将会写出 {packages / app.file_name()}")
            continue

        wasm_path = cargo_build(app, release=not args.debug)
        wasm = wasm_path.read_bytes()
        check_permissions(app, wasm)
        icon = encode_icon(app.icon)
        package = pack(app, wasm, icon)

        target = packages / app.file_name()
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
