#!/usr/bin/env python3
"""
tools/publish_bin.py

把一次构建的产物归置成 `bin/`：更新页要下载的固件镜像与字库，以及把它们各自的名字、版本、
大小和摘要写下来的 `bin/manifest.json`。

字库这一项有两种形态，写在同一个 `font` 里：

- **一张表**（`--font`）：表头里那个字体 SHA-256 就是**镜像内置那份字体**的，所以它单独
  一个文件就能用 —— 镜像里已经有另一半了。
- **一个包**（`--card-dir`）：表之外多一个 `"face"`，指向烘出这张表的那份字体。覆盖超出
  内置子集的那一份只能这么给：表只对烘它的那份字体有意义，两件必须同版、同行。更新页按
  `"face"` → `"file"` 的顺序把两个文件都下到 `/sdcard/fonts`（见 `pomelo-apps/updater`）。

两件东西都**平铺**在 `bin/` 里，清单里的名字就是卡上的名字：更新页把同一个 `file` 既当
下载地址又当落盘路径（`release::url_of` 与 `Download::of`），带目录的名字会被写进
`/sdcard/fonts/` 底下，而开机只扫这一层。

更新页（`pomelo-apps/updater`）读的就是这份清单，而它只认文件名里的版本 ——
`hivebox-firmware-v0.7.0.bin` 是 `v0.7.0`，`hivebox-font-24px-v1.bin` 是 `v1`。所以
**文件名里的版本必须和镜像自报的版本一致**，否则机器会把自己认成另一个版本，更新页
要么反复提示同一个更新，要么永远不提示。镜像自报的版本由 ESP-IDF 从 `git describe`
写进 `esp_app_desc_t.version`（见 `pomelo-firmware/firmware/CMakeLists.txt`），这里因此
优先问镜像自己，问不到才退回 `git describe`。

**一次构建出两份镜像，发布的也是两份，清单指的是其中一份。**两者在同一个 `build/` 里，只差
一次 `merge-bin`：

- `merged-binary.bin` —— **合并镜像**：bootloader + 分区表 + 应用，首尾相接。刷机时写 `0x0`，
  一条命令写完不用管偏移，发给**用 USB 刷机**的人（README 3.1 那条命令）。
- `firmware.bin` —— **应用镜像**：一个 app 槽的内容，也是 `esp_ota_begin` 要的东西。

**更新页下的必须是应用镜像。**在线更新做的正是"把一份镜像写进另一个槽"，而槽里放的从来不是
一整片 flash 的开头，只是一个应用：把合并镜像塞进去，`esp_ota_end` 会拒收，而那个槽这时已经被
写过了。所以两份都进 `bin/`，名字不同 —— `hivebox-firmware-<版本>.bin` 是合并镜像（这个名字
README 与发布工作流都指着它，不能改），`hivebox-ota-<版本>.bin` 是应用镜像，**清单里的
`firmware.file` 指的是后者**。

版本也**只能问应用镜像**：`esptool image-info` 读的是 `esp_app_desc_t`，而合并镜像是一串首尾
相接的独立镜像，esptool 只认第一个（bootloader），给的是一句"没有应用镜像描述"。这里因此顺着
`--firmware` 旁边去找 `firmware.bin`（`--app` 可以另外指一个）。找不到（构建目录被清过）才
退回 `git describe`，并且**把这件事说出来** —— 退回之后拿到的不是"镜像自报的版本"，而是
"工作区离最近的 tag 有多远"，两者只在要发布的那次提交正好打着 tag 时才相等。

每种产物只留最新的一份：`bin/` 里旧版本的固件/字库会被删掉，和这个仓库的做法一致 ——
每次提交删掉旧的、只提交最新的。

**应用包是清单里的第三个数组 `apps`。**它和固件、字库不是一回事：那两项是「机器自己用的」
（一个槽、一张表），而应用包是**给人装、给人卸**的，所以清单里是一项一个条目，商店按它画
「可安装」那一栏。条目的内容由 `tools/build_apps.py` 从包本身读出来，写进 `bin/apps.json`；
这个脚本用 `--apps` 把那份片段并进清单，同时把 `.app` 放进 `bin/apps/`（名字里的 id 与版本
就是清单里的 id 与版本），并按 **id** 清理旧版本 —— 一个应用同时只该有一个包在 `bin/apps/`，
否则清单里留哪个都是猜。`bin/` 自己留着合并镜像、OTA 镜像和字库表，十几份应用不平铺在它们
旁边。

应用包的下载地址是「清单所在目录 + `apps/` + `file`」，那一层由 `release::APPS_DIR` 加，所以
`file` 只能是一个名字，不能带目录：带目录的条目在这里就被拒绝，而不是等到商店把包下到
`bin/apps/` 之外的地方去。

用法：

    python tools/publish_bin.py                      # 两份镜像取默认构建产物，字库用 --card-dir 给
    python tools/publish_bin.py --card-dir ui-framework/pomelo-font/dist/card
    python tools/publish_bin.py --font /path/to/hivebox-font-24px-v1.bin
    python tools/publish_bin.py --version v0.7.0 --font-version 1
    python tools/publish_bin.py --app build/firmware.bin   # 应用镜像不在合并镜像旁边时
    python tools/publish_bin.py --apps bin/apps.json       # 把 build_apps.py 的片段并进清单
    python tools/publish_bin.py --dry-run            # 只打印会做什么

`--font` 与 `--card-dir` 只能给一个（清单里 `font` 只有一项，而包是两者里更强的那种），
`--font-version` 只能配 `--font`（包的版本就写在它两个名字里）。

单独一张表的版本按这个顺序找：`--font-version`、文件名里的 `v` 加数字、字库旁边的
`VERSION` 文件（`fonts/dist/VERSION`）。第三条是固件那条路在烘焙产物上的对应物 —— 那张表的
名字里带的是字号（`@24px`）而不是版本，所以版本得另找一处写下来。包不需要任何一条：它的
两个名字里都有版本，而且必须相同，`card_pair` 认不过就不发布。
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
#    这是刷机用的那一份，也是发布工作流从 `bin/` 里挑出来的那一份。
DEFAULT_FIRMWARE = os.path.join(
    WORKSPACE_ROOT, "pomelo-firmware", "firmware", "build", "merged-binary.bin"
)

# 1b. 同一个 `build/` 里的应用镜像：**在线更新下的那一份**（清单的 `firmware.file`），
#     也是唯一能读出 `esp_app_desc_t` 的一份 —— 合并镜像读不出描述（见文件头）。
APP_IMAGE_NAME = "firmware.bin"

# 2. 输出目录：仓库根下的 `bin/`，也正是更新页读取的路径
#    （`release::MANIFEST_URL` = `<repo>/raw/main/bin/manifest.json`）。
DEFAULT_BIN_DIR = os.path.join(WORKSPACE_ROOT, "bin")

# 3. 产物文件名，插进去的是版本号本身（版本号已含前缀 `v`）。
#
#    固件两个名字，**都不能省**：合并镜像给 USB 刷机的人，OTA 镜像给板子自己下（见文件头）。
#    两个名字必须互不匹配（`hivebox-ota-*` 不落在 `hivebox-firmware-*` 里），否则发布工作流
#    里那句 `ls bin/hivebox-firmware-*.bin` 会挑出两个文件，`ARTIFACT` 就成了一串多行。
FIRMWARE_TEMPLATE = "hivebox-firmware-{version}.bin"
OTA_TEMPLATE = "hivebox-ota-{version}.bin"
FONT_TEMPLATE = "hivebox-font-24px-{version}.bin"

# 4. 清旧版本时用的匹配式：只动这一类文件，免得把别的东西删掉。
#
#    字库那一条同时管着这一项的两种形态：单独一张表，和字体包里的表 —— 两者走的是同一条
#    命名规则（`hivebox-font-24px-vN.bin`），占的也是清单里同一个 `font` 项（见文件头），
#    所以在发布的那份就是留下的那份，上一版这时才真的是「旧的」。
FIRMWARE_GLOB = re.compile(r"^hivebox-firmware-.*\.bin$")
OTA_GLOB = re.compile(r"^hivebox-ota-.*\.bin$")
FONT_GLOB = re.compile(r"^hivebox-font-.*\.bin$")

# 5. 字体包里另一半（烘出那张表的那份字体）的命名规则。
#
#    版本必须和表的**完全相同**，这一点由 `card_pair` 认死；这里只管名字长什么样。
CARD_FACE_GLOB = re.compile(r"^hivebox-face-.*\.otf$")

# 6. 应用包的命名规则：`hivebox-app-<id>-<版本>.app`，与 `build_apps.py` 写出的名字一致。
#
#    捕获 id 是为了按 id 清理：应用包和固件不同，`bin/apps/` 里可以同时有十几个不同的应用，
#    每个只该有一版。id 的字符集与 `app-host` 的 `validate_id` 一致（小写字母、数字、`-`、`_`）。
APP_GLOB = re.compile(r"^hivebox-app-(?P<id>[a-z0-9][a-z0-9_-]*)-v[0-9][^/\\]*\.app$")

# 7. 应用包在 `bin/` 里占的那层目录。
#
#    `bin/` 里还躺着合并镜像、OTA 镜像和字库表，十几个应用平铺在旁边就成了读不懂的一堆；
#    商店下载地址里的这一层由 `release::APPS_DIR` 加上，两边必须同名。
APPS_SUBDIR = "apps"


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


def find_app_image(artifact, override):
    """要发布的那份**应用镜像**，找不到则 None。

    默认是 `artifact` 旁边那份 `build/firmware.bin`：`idf.py build merge-bin` 先编出它，再把
    bootloader、分区表和它拼成合并镜像，所以两者在同一个 `build/` 里、版本也是同一个。给的
    `artifact` 自己就叫 `firmware.bin` 时就是它自己，没有"旁边"可找。`--app` 给了一个不存在的
    路径时也返回 None —— 让调用方去说"找不到"，而不是这里悄悄换一个来源。
    """
    if override:
        return override if os.path.isfile(override) else None

    if os.path.basename(artifact) == APP_IMAGE_NAME:
        return artifact

    beside = os.path.join(os.path.dirname(os.path.abspath(artifact)), APP_IMAGE_NAME)
    return beside if os.path.isfile(beside) else None


def resolve_firmware_version(app_image, override):
    """固件版本的来源顺序：命令行 > 应用镜像自报 > git describe。

    `app_image` 已经在手上（`find_app_image` 找到的那份）：**能读出 `esp_app_desc_t` 的只有
    它**，合并镜像给的是一句"没有应用镜像描述"（见文件头）。
    """
    if override:
        print(f"[*] 命令行给的版本：{override}")
        return override

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
    # 表用的就是清单里 `font` 那一条的匹配式：单独一张表和包里的表是同一种东西、同一项。
    table = card_half(names, FONT_GLOB, source_dir)
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


def publish_font_package(pair, bin_dir, dry_run):
    """把一对卡上字体包放进 `bin/`，返回写进清单 `font` 项的那一份。

    两件都**平铺**，不带子目录：清单里的名字既是更新页的下载地址，也是它落到卡上的名字
    （`release::url_of` 与 `Download::of` 用的是同一个 `file`），一个带目录的名字会被写进
    `/sdcard/fonts/` 底下的子目录，而开机只扫这一层。
    """
    face, table, version = pair
    face_name = os.path.basename(face)
    table_name = os.path.basename(table)

    # 表在前、字体在后只是打印顺序：两份都写成了才写清单，中间出错这一版就没有清单。
    table_size, table_sha = place(table, bin_dir, table_name, dry_run)
    face_size, face_sha = place(face, bin_dir, face_name, dry_run)
    prune(bin_dir, FONT_GLOB, table_name, dry_run)
    prune(bin_dir, CARD_FACE_GLOB, face_name, dry_run)

    return {
        "version": version,
        "file": table_name,
        "size": table_size,
        "sha256": table_sha,
        "face": {
            "file": face_name,
            "size": face_size,
            "sha256": face_sha,
        },
    }


def read_app_fragment(path):
    """读 `build_apps.py` 写的片段，返回 `(条目, 出错说明)`。

    片段是 `{"apps": [...]}`，每个条目至少要有 `id` 与 `file`：前者是目录名与去重的键，
    后者既是下载地址的最后一段，也是 `bin/` 里的名字。`file` 带目录会被拒绝 —— 商店把清单里
    的名字原样拼在清单目录后面，带目录的条目会下到别处去。

    版本从**文件名**里读，和清单里的引用方（`updater::Version::from_file_name`）同一条规则：
    条目里的 `version` 字段只用来核对，不参与命名。
    """
    if not os.path.isfile(path):
        return None, f"找不到应用片段：{display_path(path)}"

    try:
        with open(path, "r", encoding="utf-8") as f:
            document = json.load(f)
    except (OSError, ValueError) as error:
        return None, f"读不了应用片段 {display_path(path)}：{error}"

    entries = document.get("apps") if isinstance(document, dict) else None
    if not isinstance(entries, list) or not entries:
        return None, f"{display_path(path)} 里没有 apps 数组（先跑 tools/build_apps.py）"

    seen = set()

    for entry in entries:
        if not isinstance(entry, dict):
            return None, f"{display_path(path)} 的 apps 里有一项不是对象"

        app_id = entry.get("id")
        name = entry.get("file")

        if not isinstance(app_id, str) or not app_id:
            return None, f"{display_path(path)} 的某一项没有 id"
        if not isinstance(name, str) or not name:
            return None, f"{app_id} 没有 file"
        if not APP_GLOB.match(name):
            return None, (
                f"{app_id} 的包名不是 `hivebox-app-<id>-v<版本>.app`：{name}"
                f"（商店把 file 当 URL 的最后一段，也当 bin/{APPS_SUBDIR}/ 里的名字）"
            )

        if "/" in name or "\\" in name:
            return None, f"{app_id} 的 file 带了目录：{name}"

        if app_id in seen:
            return None, f"应用片段里有两个 {app_id}：一个应用一次只能发布一版"
        seen.add(app_id)

        beside = os.path.join(os.path.dirname(os.path.abspath(path)), APPS_SUBDIR, name)
        if not os.path.isfile(beside):
            return None, f"应用片段提到的包不在它旁边的 {APPS_SUBDIR}/：{name}"

    return entries, None


def prune_apps(directory, app_id, keep, dry_run):
    """删掉 `directory` 里同一个 id 的其它版本，只留 `keep`。

    与固件、字库的 `prune` 不同，这里按**每个 id 各留一份**：一个应用只有一版是清单会指的，
    留着旧包既是没人下载的字节，也会让 `build_apps.py` 下次读到的片段和实际文件对不上。
    """
    if not os.path.isdir(directory):
        return []

    victims = []
    for name in sorted(os.listdir(directory)):
        if name == keep or not APP_GLOB.match(name):
            continue
        if APP_GLOB.match(name).group("id") != app_id:
            continue
        victims.append(name)

    for name in victims:
        path = os.path.join(directory, name)
        if dry_run:
            print(f"[dry-run] 删除旧版本：{name}")
        else:
            os.remove(path)
            print(f"[-] 删除旧版本：{name}")

    return victims


def publish_apps(entries, source_dir, bin_dir, dry_run):
    """把应用包放进 `bin/apps/`，返回写进清单 `apps` 的那一份。

    大小与摘要**重新算**，不信片段里写的那两个数：片段是上一次构建的产物，而这个函数是
    发布这一步 —— 清单里的 sha256 是商店核对下载的凭据，它必须是 `bin/apps/` 里那个文件的
    值。清单里的 `file` 仍然是**光名字**，不带目录：那一层由 `release::AppRelease::url`
    用 `APPS_DIR` 拼回下载地址。
    """
    apps_dir = os.path.join(bin_dir, APPS_SUBDIR)
    if not dry_run:
        os.makedirs(apps_dir, exist_ok=True)

    published = []

    for entry in sorted(entries, key=lambda item: item["id"]):
        name = entry["file"]
        source = os.path.join(source_dir, name)
        target = os.path.join(apps_dir, name)

        if os.path.abspath(source) == os.path.abspath(target):
            # 片段就在 bin/apps/ 里（`build_apps.py` 的默认输出），文件已经在该在的地方了。
            size = os.path.getsize(target)
            digest = sha256_of(target)
            if not dry_run:
                print(f"[=] 已在 bin/{APPS_SUBDIR}/：{name}（{size} 字节）")
        else:
            size, digest = place(source, apps_dir, name, dry_run)

        prune_apps(apps_dir, entry["id"], name, dry_run)

        published.append(
            {
                "id": entry["id"],
                "name": entry.get("name") or entry["id"],
                "name_zh": entry.get("name_zh") or entry.get("name") or entry["id"],
                "version": entry.get("version") or version_from_file_name(name),
                "file": name,
                "size": size,
                "sha256": digest,
                "abi": entry.get("abi", 1),
                "accent": entry.get("accent", "#7C8392"),
                "description": entry.get("description", ""),
                "glyph": entry.get("glyph", ""),
                "permissions": entry.get("permissions", 0),
            }
        )

    return published


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
        help=f"要发布的**合并镜像**（默认 {os.path.relpath(DEFAULT_FIRMWARE, WORKSPACE_ROOT)}）；"
        f"USB 刷机写 0x0 用的就是它，而清单指的是它旁边那份 {APP_IMAGE_NAME}（见 --app）",
    )
    parser.add_argument(
        "--app",
        help=f"要发布的**应用镜像**（清单里的 firmware.file，在线更新下的那一份）；"
        f"不给则取 --firmware 旁边那份 {APP_IMAGE_NAME}（同一个 build/）",
    )
    parser.add_argument(
        "--font",
        help="单独一张表（烘自镜像内置那份字体）；不给且没有 --card-dir 时清单里不写 font，"
        "更新页那一行就没有可下的东西",
    )
    parser.add_argument("--version", help="固件版本号；不给则问应用镜像，再不给则问 git describe")
    parser.add_argument("--font-version", help="字库版本号；不给则从字库文件名里读")
    parser.add_argument(
        "--card-dir",
        help="字体包的目录（字体 `hivebox-face-*.otf` 与它的表 `hivebox-font-*.bin` 各一份）；"
        "给就把这一对平铺进 bin/ 并写进清单的 font 项（表当 `file`，字体当 `face`）",
    )
    parser.add_argument(
        "--apps",
        help="应用包片段（`build_apps.py` 写的 `apps.json`）；给就把其中的包平铺进 bin/，"
        "并写进清单的 apps 数组 —— 商店的「可安装」那一栏读的就是它",
    )
    parser.add_argument("--bin", default=DEFAULT_BIN_DIR, help="输出目录（默认仓库根的 bin/）")
    parser.add_argument("--dry-run", action="store_true", help="只打印会做什么，不落盘")
    args = parser.parse_args()

    if not os.path.isfile(args.firmware):
        print(f"[!] 找不到合并镜像：{args.firmware}")
        print("    先构建它（idf.py build merge-bin），或用 --firmware 指定。")
        return 1

    # 应用镜像**先找到再往下走**：清单写的是它，版本问的也是它，缺了它这一版就没有可下的固件
    # —— 而这是发布里最容易悄悄少掉的一件（构建目录被清过、或者只 merge-bin 没 build）。
    app_image = find_app_image(args.firmware, args.app)
    if not app_image:
        print(f"[!] 找不到应用镜像：{args.app or APP_IMAGE_NAME + '（在 ' + display_path(args.firmware) + ' 旁边）'}")
        print(f"    要发布的是它旁边那份 {APP_IMAGE_NAME} —— 在线更新把一个应用写进一个槽，")
        print(f"    合并镜像（bootloader + 分区表 + 应用）不是槽里的东西。先 `idf.py build`，")
        print(f"    或用 --app 指定。")
        return 1

    if args.font and not os.path.isfile(args.font):
        print(f"[!] 找不到字库文件：{args.font}")
        return 1

    # 清单里的 `font` 只有一项，所以「一张表」和「一整包」只能来一个。包是两者里更强的那一种
    # （表要和烘它的字体一起用），但也确实存在只有表的那种发布：它配的是镜像内置那份字体。
    if args.font and args.card_dir:
        print("[!] --font 与 --card-dir 只能给一个：清单里的 font 只有一项。")
        print("    一张表和一整包是这一项的两种形态（包的 `face` 缺省时就是一个文件），")
        print("    要发包就只给 --card-dir。")
        return 1

    if args.card_dir and args.font_version:
        print("[!] --font-version 与 --card-dir 不能一起给：包的版本就在它的两个名字里，")
        print("    而清单里的版本也只能是那个（更新页只读文件名里的版本）。")
        return 1

    # 卡上包**先认后写**：认不过就一个字节都不落盘，免得把这一版的一半留在 bin/ 里。
    card = None
    if args.card_dir:
        card, problem = card_pair(args.card_dir)
        if problem:
            print(f"[!] {problem}")
            return 1

    # 应用包也是先认后写：片段里的每个 id、每个包名、每个文件都要在，否则这一版的一个应用
    # 会以「清单里有、bin/ 里没有」的形态发出去，而商店按下按钮才发现。
    apps = None
    if args.apps:
        apps, problem = read_app_fragment(args.apps)
        if problem:
            print(f"[!] {problem}")
            return 1

    # ---- 版本号 ---------------------------------------------------------------
    firmware_version = resolve_firmware_version(app_image, args.version)
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
    ota_name = OTA_TEMPLATE.format(version=firmware_version)

    # 包里的表名是它自己的（版本已由 `card_pair` 从名字里读过并核对过），单独一张表才按模板拼。
    font_name = None
    if card:
        font_name = os.path.basename(card[1])
    elif args.font:
        font_name = FONT_TEMPLATE.format(version=font_version)

    if not args.dry_run:
        os.makedirs(args.bin, exist_ok=True)

    # ---- 落盘与清理 -----------------------------------------------------------
    #
    # 两份镜像都落盘：合并镜像是给 USB 刷机的人的（README 3.1），而**清单指的是 OTA 那份**
    # ——更新页 `esp_ota_begin` 要的是一个应用的镜像，不是一整片 flash 的开头（见文件头）。
    place(args.firmware, args.bin, firmware_name, args.dry_run)
    prune(args.bin, FIRMWARE_GLOB, firmware_name, args.dry_run)

    ota_size, ota_sha = place(app_image, args.bin, ota_name, args.dry_run)
    prune(args.bin, OTA_GLOB, ota_name, args.dry_run)

    manifest = {
        "generated": datetime.datetime.now(datetime.timezone.utc)
        .replace(microsecond=0)
        .isoformat()
        .replace("+00:00", "Z"),
        "firmware": {
            "version": firmware_version,
            "file": ota_name,
            "size": ota_size,
            "sha256": ota_sha,
        },
    }

    if card:
        manifest["font"] = publish_font_package(card, args.bin, args.dry_run)
    elif args.font:
        font_size, font_sha = place(args.font, args.bin, font_name, args.dry_run)
        prune(args.bin, FONT_GLOB, font_name, args.dry_run)
        manifest["font"] = {
            "version": font_version,
            "file": font_name,
            "size": font_size,
            "sha256": font_sha,
        }

    app_entries = []
    if apps is not None:
        # 包在片段旁边的 `apps/` 里（build_apps.py 的布局），不在片段自己那一层。
        source_dir = os.path.join(os.path.dirname(os.path.abspath(args.apps)), APPS_SUBDIR)
        app_entries = publish_apps(apps, source_dir, args.bin, args.dry_run)
        manifest["apps"] = app_entries

    write_manifest(args.bin, manifest, args.dry_run)

    published = [
        f"{firmware_name}（合并镜像，USB 刷 0x0）",
        f"{ota_name}（应用镜像，清单的 firmware.file 指的就是它）",
    ]
    if card:
        published.append(f"{font_name} + {os.path.basename(card[0])}（{card[2]} 的一对）")
    elif font_name:
        published.append(font_name)
    if app_entries:
        published.append(f"{len(app_entries)} 个应用包")

    print("[*] 完成。bin/ 现在有：" + "、".join(published))
    return 0


if __name__ == "__main__":
    sys.exit(main())
