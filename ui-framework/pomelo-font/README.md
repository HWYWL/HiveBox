# pomelo-font

字体资产与烘培工具的统一仓库，供 [pomelo-os](https://github.com/pomelos-on-sale/pomelo-os) 嵌入式固件使用。

本仓库收敛了以前散落在主仓库各处的内容：

| 过去的位置 | 现在 |
| --- | --- |
| `tools/bake_glyphs.py` | `bake.py` |
| `assets/fonts/source/charset-common.txt` | `source/charset-common.txt` |
| `assets/fonts/source/source-han-sans-sc-regular.otf` | `source/`（不进 git，见下方） |
| `vendor/iced-pomelo-winit/fonts/*.otf` / `*.bin` | `dist/` |

## 目录结构

```
pomelo-font/
├── bake.py              # 烘培脚本
├── requirements.txt     # pip 依赖
├── source/
│   ├── charset-common.txt               # 镜像那份的字符集（tools/scan_charset.py 生成，勿手改）
│   ├── charset-gb2312.txt               # 卡上那份的字符集（tools/gen_charset_gb2312.py 生成）
│   └── source-han-sans-sc-regular.otf   # 源字体（不进 git，见下方下载命令）
└── dist/
    ├── SourceHanSansSC-Regular-Subset.otf          # 运行时子集字体（进固件）
    ├── SourceHanSansSC-Regular-Subset-common@24px.bin
    ├── MANIFEST.md                                 # 由 bake.py 生成，记录烘培参数
    ├── SourceHanSansSC-Regular-Subset-gb2312.otf   # 卡上那份的子集字体
    ├── SourceHanSansSC-Regular-Subset-gb2312@24px.bin
    ├── MANIFEST-gb2312.md                          # 卡上那一档的烘培参数
    └── card/                                       # 卡上的字体包（`--card`，发布给更新页下载）
        ├── hivebox-face-v3.otf
        └── hivebox-font-24px-v3.bin
```

`dist/` 中的文件是已提交的产物：`SourceHanSansSC-Regular-Subset*.otf` 与两档 `@24px.bin` 里的**镜像那一份**
被 `iced-pomelo-winit` 通过 `include_bytes!` 嵌入固件，`dist/card/` 是给 SD 卡的字体包（不进固件）。
这一对由主仓库的 `tools/publish_bin.py --card-dir dist/card` 归置进 `bin/` —— **平铺**，清单里
`font` 项的 `"file"` 是表、`"face"` 是字体（更新页把一个名字既当下载地址又当落盘路径），板子的
「更新」页从那儿把两件一起下到卡上。

## 使用方法

### 首次使用：安装依赖

```bash
pip install -r requirements.txt
```

### 下载源字体（不进 git，16.5 MB）

```bash
curl -sSL -o source/source-han-sans-sc-regular.otf \
  https://github.com/adobe-fonts/source-han-sans/raw/release/OTF/SimplifiedChinese/SourceHanSansSC-Regular.otf
# sha256: f1d8611151880c6c336aabeac4640ef434fa13cbfbf1ffe82d0a71b2a5637256
```

### 生成字符集（改了界面文案之后）

```bash
python tools/scan_charset.py --write   # 在主仓库根目录跑
```

### 生成卡上的字符集（改了补字清单之后）

```bash
python tools/gen_charset_gb2312.py --write   # 在主仓库根目录跑
```

它按 GB2312 的全部可显字符区重算，再把 `charset-common.txt` 与脚本里的 `EXTRAS`（GB2312 收不下的
字，比如繁体「雲」）并进去。不加 `--write` 时只打印「新增/裁掉」哪几个字 —— 先看这个，确认没跑偏再写。

### 子集化（仅在字符集 `source/charset-common.txt` 变更时需要）

```bash
pyftsubset source/source-han-sans-sc-regular.otf \
    --text-file=source/charset-common.txt \
    --drop-tables+=DSIG \
    --output-file=dist/SourceHanSansSC-Regular-Subset.otf
```

### 烘培字形表

```bash
python3 bake.py                      # 默认字号：24 px
python3 bake.py --sizes 24 32        # 指定字号
```

烘培完成后提交 `dist/` 的变更，在 `pomelo-os` 里更新 `vendor/pomelo-font` 的 submodule 指针。

### 卡上的字体包（`--card`）

镜像里那份子集只有 528 个字，而屏幕上的文件名、Wi-Fi 名里会出现它没有的字 —— 那些字**光换表救不了**：
表里的字形号是**字体**的字形号，设备端在排版那一步拿到的就已经是 `.notdef` 了。所以卡上要同时有字体和
它的表。用卡上那一档字符集子集化、烘培，再加 `--card` 让 `bake.py` 把它们按**设备端认得的名字**另存一份
（版本号两半共用，`updater::card_font` 靠它把两半配成一对）：

```bash
pyftsubset source/source-han-sans-sc-regular.otf \
    --text-file=source/charset-gb2312.txt \
    --drop-tables+=DSIG \
    --output-file=dist/SourceHanSansSC-Regular-Subset-gb2312.otf

python3 bake.py \
    --source dist/SourceHanSansSC-Regular-Subset-gb2312.otf \
    --charset source/charset-gb2312.txt \
    --card v3
```

两个文件一起进 SD 卡的 `fonts/`（`updater::FONT_DIR`，即 `/sdcard/fonts`），重启生效 —— **不用重刷固件**。
`hivebox-face-*.otf` 是字体，`hivebox-font-24px-*.bin` 是它的表，**缺一个就退回内置那份**（半套东西不会
把机器带进「画不出字」的状态）。

放进卡里有两条路，**都不用数据线**：板子的「更新」页点「字库 → 安装」，它按清单把两件一起下到
`/sdcard/fonts`（先字体、后表 —— 表是开机扫的那一件，所以它是把这一对变成字体的那一下）；或者在
Releases 页把这两件下到电脑、拷进卡里。两种都一样：**重启生效**，不需要重刷固件。

换版本号时旧的那一对**不用自己删**：开机认的是「成套里最新的那一对」，孤零零的表（或只有字体、
没有表的那一份）不算数，所以旧的那一对留着也碍不着事，多占的只是卡上的几兆。

## 字符集

字体是**思源黑体 SC Regular**（Source Han Sans SC Regular）的子集。有两档，用两套字符集：

| 档 | 字符集 | 字数 | 运行时子集 | 24 px 预烘表 | 放哪儿 |
| --- | --- | ---: | ---: | ---: | --- |
| 镜像 | `charset-common.txt` | 529 | 0.19 MB | 0.23 MB | flash（两个 OTA 槽的代价压在这儿） |
| 卡上 | `charset-gb2312.txt` | 4538 | 1.91 MB | 2.27 MB | SD 卡（一个字节都不占 flash） |

**镜像那一档**收的是**这棵树画出来的字**：由主仓库的 `tools/scan_charset.py` 从源码的字符串与字符字面量里
扫出来（注释不算 —— 注释不进固件），再加上 ASCII 可打印区和一个全角空格。字数是跟着代码走的：多写一句
文案就多一个字，删掉一句就少一个字，不再需要「碰到了补一行」。差出来的 **3.27 MB** 正是 app 能不能立起两个
OTA 槽位的关键：16 MB 的 flash 装不下两个各带一套完整字库的 app。

**卡上那一档**把兜底放了回来：GB2312 的全部可显字符（符号区 + 一级汉字 3755 字）再加上
`charset-common.txt`。屏幕上会出现、而源码里没写过的字 —— SD 卡上的文件名（`files` 的
`format!("删除「{name}」？")`）、Wi-Fi 名字、网络接口返回的文案 —— 都归这一档。它**不进 flash**，所以
没有体积上的取舍；代价换成「要有卡」与「开机多读 4.2 MB」（读过一次就一直留在 PSRAM 里）。

GB2312 收不下的字（繁体、生僻字）在 `tools/gen_charset_gb2312.py` 的 `EXTRAS` 里一个一个列：一级汉字区
里只有简体「云」，繁体「雲」（U+96F2）不在里面，只能显式补上。

> 界面文案里出现的新字如果不在子集里，屏幕上画出来的是一个方框（`.notdef`），不会报错也不会让构建失败。**镜像那一档**的补字步骤是：把要写的话写进代码，重跑 `python tools/scan_charset.py --write`（在主仓库根目录），再做「子集化」与「烘培字形表」两节 —— **子集化会重排 glyph_id，所以字符集一变就必须重烘**，主仓库 `iced-pomelo-winit/fonts/dist/` 与本仓库 `dist/` 两处都要更新。**卡上那一档**（繁体、生僻字、以及一切源码里没写过的字）走「卡上的字体包」一节。

烘培的字号只有一个：

| 字号 | 用途 |
| ---: | --- |
| 24 px | 界面上的所有文字 |

界面把正文字号统一收敛到 24 px（`pomelo-widgets` 的 `FONT_BASE`），四档可调的字号系统已经删除，
18/20/30 px 不再出现在任何布局里，所以 `bake.py` 只烘这一档。

**为什么一档就够**：设备端查表时把字号逐位（f32 的位）与表头比较，别的字号命中不了任何表，退回
在线光栅化（约 1.6 ms/字形）并进 RAM 缓存 —— 慢，但不会画错。所以多烘一档是白占 flash
（每字节约 0.93×字号² 字节），少烘一档只是慢。

图标不在这个范围内：Material Symbols 是另一个字体家族（`pomelo-material-symbols`），不参与这套
预烘表 —— 预烘表是与它签名的那个字体 id 绑定的，图标字的字号只是各页自己的测量值。

## 字体许可

思源黑体以 **SIL Open Font License 1.1** 发布，全文见 `OFL.txt`。OFL 允许子集、内嵌与再分发，因此 `dist/SourceHanSansSC-Regular-Subset.otf` 可以随系统一起发布。

`bake.py` 及本仓库的其余代码以 **GPL-3.0-only** 发布，见 `LICENSE`。

## 在 pomelo-os 中的使用

本仓库作为 `pomelo-os` 的 git submodule，挂载在 `vendor/pomelo-font/`。`iced-pomelo-winit` 通过 `include_bytes!` 引用 `dist/` 中的文件，路径为 `../../pomelo-font/dist/...`（相对于 `iced-pomelo-winit/src/`）。
