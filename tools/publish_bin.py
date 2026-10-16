#!/usr/bin/env python3
"""
tools/publish_bin.py

把一次构建的产物归置成 `bin/`：更新页要下载的两个文件（固件镜像、字库）、给人往 SD 卡里
放的那一对卡上字体包（`bin/card/`），以及把它们各自的名字、版本、大小和摘要写下来的
`bin/manifest.json`。

更新页（`pomelo-apps/updater`）读的就是这份清单，而它只认文件名里的版本 ——
`hivebox-firmware-v0.7.0.bin` 是 `v0.7.0`，`hivebox-font-24px-v1.bin` 是 `v1`。所以
**文件名里的版本必须和镜像自报的版本一致**，否则机器会把自己认成另一个版本，更新页
要么反复提示同一个更新，要么永远不提示。镜像自报的版本由 ESP-IDF 从 `git describe`
写进 `esp_app_desc_t.version`（见 `pomelo-firmware/firmware/CMakeLists.txt`），这里因此
优先问镜像自己，问不到才退回 `git describe`。

**要问的是应用镜像，不是要发布的那份合并镜像。**`bin/` 里放的是合并镜像（刷机写 `0x0`
用），而 `esptool image-info` 读不了它 —— 合并镜像是一串首尾相接的独立镜像（bootloader、
分区表、应用……），esptool 只认第一个，给的是一句"没有应用镜像描述"。能读出
`esp_app_desc_t` 的只有同一次构建的 `build/firmware.bin`，也就是 app 分区里那份。两者在
同一个 `build/` 目录下，所以这里顺着 `--firmware` 旁边去找它。找不到（构建目录被清过）
才退回 `git describe`，并且**把这件事说出来** —— 退回之后拿到的不是"镜像自报的版本"，
而是"工作区离最近的 tag 有多远"，两者只在要发布的那次提交正好打着 tag 时才相等。

每种产物只留最新的一份：`bin/` 里旧版本的固件/字库会被删掉，和这个仓库的做法一致 ——
每次提交删掉旧的、只提交最新的。

用法：

    python tools/publish_bin.py                      # 固件取默认构建产物，字库用 --font 给
    python tools/publish_bin.py --font /path/to/hivebox-font-24px-v1.bin
    python tools/publish_bin.py --version v0.7.0 --font-version 1
    python tools/publish_bin.py --card-dir ui-framework/pomelo-font/dist/card
    python tools/publish_bin.py --dry-run            # 只打印会做什么

字库版本按这个顺序找：`--font-version`、文件名里的 `v` 加数字、字库旁边的 `VERSION` 文件
（`fonts/dist/VERSION`）。第三条是固件那条路在烘焙产物上的对应物 —— 那张表的名字里带的是
字号（`@24px`）而不是版本，所以版本得另找一处写下来。
"""

import argparse
import datetime
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys

WORKSPACE_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# 1. 默认输入：ESP-IDF 的合并镜像（`idf.py merge-bin` 的产物，见发布工作流）。
DEFAULT_FIRMWARE = os.path.join(
    WORKSPACE_ROOT, "pomelo-firmware", "firmware", "build", "merged-binary.bin"
)

# 1b. 同一个 `build/` 里能读出 `esp_app_desc_t` 的那一份：app 分区里的应用镜像。
#     它是**问版本**的对象，而不是**发布**的对象 —— 合并镜像读不出描述（见文件头）。
APP_IMAGE_NAME = "firmware.bin"

# 2. 输出目录：仓库根下的 `bin/`，也正是更新页读取的路径
#    （`release::MANIFEST_URL` = `<repo>/raw/main/bin/manifest.json`）。
DEFAULT_BIN_DIR = os.path.join(WORKSPACE_ROOT, "bin")

# 3. 产物文件名，插进去的是版本号本身（版本号已含前缀 `v`）。
FIRMWARE_TEMPLATE = "hivebox-firmware-{version}.bin"
FONT_TEMPLATE = "hivebox-font-24px-{version}.bin"

# 4. 清旧版本时用的匹配式：只动这一类文件，免得把别的东西删掉。
FIRMWARE_GLOB = re.compile(r"^hivebox-firmware-.*\.bin$")
FONT_GLOB = re.compile(r"^hivebox-font-.*\.bin$")

# 5. 卡上字体包（字体 + 照它烘的表）在 `bin/` 里的落脚处，以及它两个文件的命名规则。
#
#    单独一层子目录，**不能平铺**：包里的表叫 `hivebox-font-24px-v3.bin`，和更新页那张表
#    （`hivebox-font-24px-v1.bin`）走的是同一条命名规则，平铺在一起就会被 `prune` 的
#    「同一种只留最新」互相删掉 —— 那两件东西名字像，用途完全不同：一件给更新页下到
#    `/sdcard/fonts`，另一件要**和字体一起**放在那里才认。
CARD_DIR_NAME = "card"
CARD_FACE_GLOB = re.compile(r"^hivebox-face-.*\.otf$")
CARD_TABLE_GLOB = re.compile(r"^hivebox-font-.*\.bin$")


def display_path(path):
    """给人看的路径：在仓库里就写相对路径，不在就写绝对路径。

    Windows 上跨盘符 `os.path.relpath` 会抛 `ValueError`（C: 到 E: 之间没有相对路径），
    而 `--firmware` 指到另一个盘是完全正常的用法 —— 打印一行路径不该因此崩掉。
    """
    try:
        return os.path.relpath(path, WORKSPACE_ROOT)
    except ValueError:
        return os.path.abspath(path)


def sha256_of(path):
    """一个文件的 SHA-256，小写十六进制 —— 与清单里 `sha256` 的写法一致。"""
    digest = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def version_from_text(text):
    """从一段文字里读出开头的版本号，如 `v0.7.0` / `0.7.0` / `v1`。读不出返回 None。

    只取开头那段数字与点，遇到第一个别的字符就停 —— 这正是更新页
    `release::Version::parse` 的做法，于是 `v0.6.0-3-g1a2b`（describe 的尾巴）在这里和
    在机器上被收窄成同一个 `v0.6.0`。
    """
    rest = text[1:] if text[:1] in ("v", "V") else text
    match = re.match(r"[0-9.]+", rest)
    if not match:
        return None
    head = match.group(0).rstrip(".")
    if not head:
        return None
    return "v" + head


def version_from_file_name(name):
    """从文件名里读出版本号：`hivebox-font-24px-v1.bin` 是 `v1`。

    取**最后一个** `v`（或 `V`）加数字的位置，而不是第一个数字 —— 名字先是描述再是版本，
    `24px` 是字号不是版本。与更新页 `Version::from_file_name` 同一条规则。
    """
    for at in range(len(name) - 2, -1, -1):
        if name[at] in ("v", "V") and name[at + 1].isdigit():
            return version_from_text(name[at:])
    return None


def version_from_beside(path):
    """读 `path` 旁边的 `VERSION` 文件，读不到返回 None。

    烘焙出来的字形表名字里是字号（`…-common@24px.bin`），没有一个地方能放下版本号 ——
    于是版本写在它旁边。格式就是一行 `1` 或 `v1`，多一行注释也认（第一个非空、非 `#` 的行）。
    """
    beside = os.path.join(os.path.dirname(os.path.abspath(path)), "VERSION")
    if not os.path.isfile(beside):
        return None

    with open(beside, "r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if line and not line.startswith("#"):
                return version_from_text(line)
    return None


def normalized_version(text):
    """把用户给的版本号规整成 `vX.Y.Z` 的样子；`1` 和 `v1` 都是 `v1`。"""
    if text is None:
        return None
    text = text.strip()
    if not text:
        return None
    return version_from_text(text) or version_from_text("v" + text)


def firmware_version_from_image(path):
    """问镜像自己它是什么版本（`esp_app_desc_t.version`），问不到返回 None。

    这是唯一不会说谎的来源：文件名的版本就是要和它一致。需要 esptool，且是
    ESP-IDF 的 Python 环境里那一份；本机没有就退回去问 git。

    `path` 必须是**应用镜像**（`build/firmware.bin`）：合并镜像里第一个镜像是
    bootloader，没有应用描述，esptool 会以非零退出，这里就成了"读不出来"。
    """
    try:
        result = subprocess.run(
            [sys.executable, "-m", "esptool", "--chip", "esp32s3", "image-info", path],
            capture_output=True,
            text=True,
            timeout=120,
        )
    except (OSError, subprocess.SubprocessError):
        return None

    if result.returncode != 0:
        return None

    for line in result.stdout.splitlines():
        if line.startswith("App version:"):
            value = line.split(":", 1)[1].strip()
            if value:
                return value
    return None


def firmware_version_from_git():
    """`git describe` 给出的版本，与固件 CMake 用的是同一条命令（同一份 `PROJECT_VER`）。"""
    try:
        result = subprocess.run(
            ["git", "describe", "--tags", "--always", "--dirty", "--abbrev=4"],
            cwd=WORKSPACE_ROOT,
            capture_output=True,
            text=True,
            timeout=30,
        )
    except (OSError, subprocess.SubprocessError):
        return None

    if result.returncode != 0:
        return None
    return result.stdout.strip() or None


def app_image_beside(artifact):
    """`artifact` 旁边那份应用镜像，没有则返回 None。

    就是同一个 `build/` 目录里的 `firmware.bin`：`idf.py build merge-bin` 先编出它，再把
    bootloader、分区表和它拼成合并镜像，所以两者的版本是同一个。

    `artifact` 自己就叫 `firmware.bin` 时返回 None —— 那说明调用方给的正是能读的那一份，
    没有"旁边"可找。
    """
    if os.path.basename(artifact) == APP_IMAGE_NAME:
        return None

    beside = os.path.join(os.path.dirname(os.path.abspath(artifact)), APP_IMAGE_NAME)
    return beside if os.path.isfile(beside) else None


def resolve_firmware_version(artifact, override):
    """固件版本的来源顺序：命令行 > 应用镜像自报 > git describe。

    `artifact` 是**要发布的那一份**（合并镜像）；**要问的是它旁边那份应用镜像**，见
    `app_image_beside` —— 合并镜像读不出 `esp_app_desc_t`。给的就是应用镜像时问它自己。
    """
    if override:
        print(f"[*] 命令行给的版本：{override}")
        return override

    app_image = app_image_beside(artifact) or artifact
    from_image = firmware_version_from_image(app_image)
    if from_image:
        print(f"[*] 镜像自报的版本：{from_image}（读的是 {display_path(app_image)}）")
        return from_image

    # 读不出来就说清楚，而不是悄悄换一个来源 —— 这两个来源不总是相等。
    print(f"[!] 读不出 {display_path(app_image)} 的版本，改问 git describe。")
    print("    两者不等价：git 给的是「工作区离最近的 tag 有多远」，只有在要发布的这次")
    print("    提交正好打着 tag 时，它才等于镜像自报的版本。先 `idf.py build`，或用 --version。")

    from_git = firmware_version_from_git()
    if from_git:
        print(f"[*] git describe 的版本：{from_git}")
        return from_git

    return None


def prune(directory, pattern, keep, dry_run):
    """删掉 `directory` 里匹配 `pattern` 又不是 `keep` 的文件，返回删掉的名字。"""
    if not os.path.isdir(directory):
        return []

    victims = []
    for name in sorted(os.listdir(directory)):
        if name != keep and pattern.match(name):
            victims.append(name)

    for name in victims:
        path = os.path.join(directory, name)
        if dry_run:
            print(f"[dry-run] 删除旧版本：{name}")
        else:
            os.remove(path)
            print(f"[-] 删除旧版本：{name}")
    return victims


def place(source, directory, name, dry_run):
    """把一个产物放进 `bin/`，返回它的大小与摘要。"""
    if dry_run:
        size = os.path.getsize(source)
        digest = sha256_of(source)
        print(f"[dry-run] 放入：{name}（{size} 字节）")
        return size, digest

    target = os.path.join(directory, name)
    shutil.copyfile(source, target)
    size = os.path.getsize(target)
    digest = sha256_of(target)
    print(f"[+] 写入：{name}（{size} 字节）")
    return size, digest


def card_half(names, pattern, source_dir):
    """`names` 里唯一匹配 `pattern` 的那个文件（返回完整路径）；不是正好一个则返回 None。

    卡上包是**一对**，而这一对里「哪个更新就用哪个」在这里没有意义：表和字体必须同版，
    随便挑一半就是把两套字形号配在一起。所以上一版留在目录里的那一份
    （`hivebox-face-v2.otf`）不是「旧的」而是「说不清的」—— 让调用方把目录清到只剩一对，
    这里不替它猜。
    """
    matches = sorted(name for name in names if pattern.match(name))
    if len(matches) != 1:
        return None
    return os.path.join(source_dir, matches[0])


def card_pair(source_dir):
    """认出一对卡上字体包，返回 `((字体, 表, 版本), 出错说明)`。

    两半是一件事，因为它们在设备那边也是一件事：表头里带着烘它那份字体的 SHA-256
    （前 16 字节），拿错一份表就是递过去另一套字形号，渲染器会拒收（见固件里的
    `prefer_card_font`）。所以这里要求两半都在、且**版本号相同**。

    这一步只认不写：`bin/` 的落盘要等固件、字库都核对完了再一起做（见 `main`）。
    """
    if not os.path.isdir(source_dir):
        return None, f"卡上包的目录不存在：{display_path(source_dir)}"

    names = sorted(os.listdir(source_dir))
    face = card_half(names, CARD_FACE_GLOB, source_dir)
    table = card_half(names, CARD_TABLE_GLOB, source_dir)
    if not face or not table:
        return None, (
            f"{display_path(source_dir)} 里不是正好一对：字体 `hivebox-face-*.otf` 与它的表 "
            "`hivebox-font-*.bin` 各要一份（多出来的那份是上一版留下的，先删掉）"
        )

    face_version = version_from_file_name(os.path.basename(face))
    table_version = version_from_file_name(os.path.basename(table))
    if not face_version or not table_version:
        return None, (
            f"卡上包的名字里读不出版本号：{os.path.basename(face)} / {os.path.basename(table)}"
            "（要写成 `hivebox-face-vN.otf` 与 `hivebox-font-24px-vN.bin`）"
        )
    if face_version != table_version:
        return None, (
            f"卡上包的两半不是同一版：字体 {face_version}、表 {table_version} —— "
            "表只对烘它的那份字体有意义，配错等于递过去另一套字形号"
        )

    return (face, table, face_version), None


def publish_card(pair, bin_dir, dry_run):
    """把一对卡上包放进 `bin/card/`，返回写进清单的那一项。"""
    face, table, version = pair
    card_dir = os.path.join(bin_dir, CARD_DIR_NAME)
    if not dry_run:
        os.makedirs(card_dir, exist_ok=True)

    face_name = os.path.basename(face)
    table_name = os.path.basename(table)
    face_size, face_sha = place(face, bin_dir, f"{CARD_DIR_NAME}/{face_name}", dry_run)
    table_size, table_sha = place(table, bin_dir, f"{CARD_DIR_NAME}/{table_name}", dry_run)
    prune(card_dir, CARD_FACE_GLOB, face_name, dry_run)
    prune(card_dir, CARD_TABLE_GLOB, table_name, dry_run)

    return {
        "version": version,
        "face": {
            "file": f"{CARD_DIR_NAME}/{face_name}",
            "size": face_size,
            "sha256": face_sha,
        },
        "table": {
            "file": f"{CARD_DIR_NAME}/{table_name}",
            "size": table_size,
            "sha256": table_sha,
        },
    }


def write_manifest(directory, manifest, dry_run):
    """把清单写下来，缩进两格 —— 它是要提交进仓库、被人读的。"""
    path = os.path.join(directory, "manifest.json")
    text = json.dumps(manifest, indent=2, ensure_ascii=False) + "\n"

    if dry_run:
        print(f"[dry-run] 写入：manifest.json\n{text}")
        return

    with open(path, "w", encoding="utf-8") as f:
        f.write(text)
    print(f"[+] 写入：manifest.json")


def main():
    parser = argparse.ArgumentParser(
        description="把构建产物归置成 bin/：带版本号的固件、字库、卡上字体包，以及更新页读的 manifest.json。"
    )
    parser.add_argument(
        "--firmware",
        default=DEFAULT_FIRMWARE,
        help=f"要发布的固件镜像（默认 {os.path.relpath(DEFAULT_FIRMWARE, WORKSPACE_ROOT)}）；"
        f"版本从同一个 build/ 里的 {APP_IMAGE_NAME} 读",
    )
    parser.add_argument("--font", help="字库文件；不给则清单里不写 font，更新页那一行就没有可下的东西")
    parser.add_argument("--version", help="固件版本号；不给则问应用镜像，再不给则问 git describe")
    parser.add_argument("--font-version", help="字库版本号；不给则从字库文件名里读")
    parser.add_argument(
        "--card-dir",
        help="卡上字体包的目录（字体 `hivebox-face-*.otf` 与它的表 `hivebox-font-*.bin` 各一份）；"
        f"给就把这一对归置进 bin/{CARD_DIR_NAME}/ 并写进清单的 card 项",
    )
    parser.add_argument("--bin", default=DEFAULT_BIN_DIR, help="输出目录（默认仓库根的 bin/）")
    parser.add_argument("--dry-run", action="store_true", help="只打印会做什么，不落盘")
    args = parser.parse_args()

    if not os.path.isfile(args.firmware):
        print(f"[!] 找不到固件镜像：{args.firmware}")
        print("    先构建它（idf.py build merge-bin），或用 --firmware 指定。")
        return 1

    if args.font and not os.path.isfile(args.font):
        print(f"[!] 找不到字库文件：{args.font}")
        return 1

    # 卡上包**先认后写**：认不过就一个字节都不落盘，免得把这一版的一半留在 bin/ 里。
    card = None
    if args.card_dir:
        card, problem = card_pair(args.card_dir)
        if problem:
            print(f"[!] {problem}")
            return 1

    # ---- 版本号 ---------------------------------------------------------------
    firmware_version = resolve_firmware_version(args.firmware, args.version)
    if not firmware_version:
        print("[!] 读不出固件版本：镜像里没有，git 也说不出。用 --version 直接给一个。")
        return 1
    if "dirty" in firmware_version:
        print(f"[!] 版本号带着 -dirty（{firmware_version}）：这份代码不是 tag 那份，先提交再发布。")
        return 1

    font_version = None
    if args.font:
        font_version = (
            normalized_version(args.font_version)
            or version_from_file_name(os.path.basename(args.font))
            or version_from_beside(args.font)
        )
        if not font_version:
            print("[!] 读不出字库版本：文件名里没有 v 加数字，旁边也没有 VERSION。")
            print("    用 --font-version 直接给一个，或写一份 VERSION 放在字库旁边。")
            return 1

    # ---- 文件名 ---------------------------------------------------------------
    firmware_name = FIRMWARE_TEMPLATE.format(version=firmware_version)
    font_name = FONT_TEMPLATE.format(version=font_version) if args.font else None

    if not args.dry_run:
        os.makedirs(args.bin, exist_ok=True)

    # ---- 落盘与清理 -----------------------------------------------------------
    firmware_size, firmware_sha = place(args.firmware, args.bin, firmware_name, args.dry_run)
    prune(args.bin, FIRMWARE_GLOB, firmware_name, args.dry_run)

    manifest = {
        "generated": datetime.datetime.now(datetime.timezone.utc)
        .replace(microsecond=0)
        .isoformat()
        .replace("+00:00", "Z"),
        "firmware": {
            "version": firmware_version,
            "file": firmware_name,
            "size": firmware_size,
            "sha256": firmware_sha,
        },
    }

    if args.font:
        font_size, font_sha = place(args.font, args.bin, font_name, args.dry_run)
        prune(args.bin, FONT_GLOB, font_name, args.dry_run)
        manifest["font"] = {
            "version": font_version,
            "file": font_name,
            "size": font_size,
            "sha256": font_sha,
        }

    if card:
        manifest["card"] = publish_card(card, args.bin, args.dry_run)

    write_manifest(args.bin, manifest, args.dry_run)

    print(
        f"[*] 完成。bin/ 现在有：{firmware_name}"
        + (f"、{font_name}" if font_name else "")
        + (f"、{CARD_DIR_NAME}/ 里一对（{card[2]}）" if card else "")
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
