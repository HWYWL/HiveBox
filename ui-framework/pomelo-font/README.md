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
│   ├── charset-common.txt               # 字符集（由 tools/scan_charset.py 生成，勿手改）
│   └── source-han-sans-sc-regular.otf   # 源字体（不进 git，见下方下载命令）
└── dist/
    ├── SourceHanSansSC-Regular-Subset.otf         # 运行时子集字体（进固件）
    ├── SourceHanSansSC-Regular-Subset-common@24px.bin
    └── MANIFEST.md                                # 由 bake.py 生成，记录烘培参数
```

`dist/` 中的文件是已提交的产物，直接被 `iced-pomelo-winit` 通过 `include_bytes!` 嵌入固件。

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

## 字符集

字体是**思源黑体 SC Regular**（Source Han Sans SC Regular）的子集，收的字就是**这棵树画出来的字**：
由主仓库的 `tools/scan_charset.py` 从源码的字符串与字符字面量里扫出来（注释不算 —— 注释不进固件），
再加上 ASCII 可打印区和一个全角空格。

这条规则换掉了曾经的「GB2312 一级字（3755 字）+ 手工补字」，换来的体积是这样：

| 字符集 | 字数 | 运行时子集 | 24 px 预烘表 |
| --- | ---: | ---: | ---: |
| GB2312 一级字 | 4013 | 1.70 MB | 1.98 MB |
| 源码里画出来的字 | 529 | 0.19 MB | 0.23 MB |

差出来的 **3.27 MB** 正是 app 能不能立起两个 OTA 槽位的关键：16 MB 的 flash 装不下两个各带一套完整
字库的 app。

反过来，字数是跟着代码走的：多写一句文案就多一个字，删掉一句就少一个字，不再需要「碰到了补一行」。

**代价是兜底没了**。屏幕上会出现、而源码里没写过的字 —— SD 卡上的文件名（`files` 的
`format!("删除「{name}」？")`）、Wi-Fi 名字、网络接口返回的文案 —— 画出来是方框（`.notdef`）。
这是刻意的取舍：要兜底就把 GB2312 那一档放回来，也就是把那 3.27 MB 放回来。

> 界面文案里出现的新字如果不在子集里，屏幕上画出来的是一个方框（`.notdef`），不会报错也不会让构建失败。补字的步骤是：把要写的话写进代码，重跑 `python tools/scan_charset.py --write`（在主仓库根目录），再做「子集化」与「烘培字形表」两节 —— **子集化会重排 glyph_id，所以字符集一变就必须重烘**，主仓库 `iced-pomelo-winit/fonts/dist/` 与本仓库 `dist/` 两处都要更新。

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
