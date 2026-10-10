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
│   ├── charset-common.txt               # 字符集定义（3755 汉字 + ASCII + 标点）
│   └── source-han-sans-sc-regular.otf   # 源字体（不进 git，见下方下载命令）
└── dist/
    ├── SourceHanSansSC-Regular-Subset.otf         # 运行时子集字体（进固件）
    ├── SourceHanSansSC-Regular-Subset-common@14px.bin
    ├── SourceHanSansSC-Regular-Subset-common@15px.bin
    ├── SourceHanSansSC-Regular-Subset-common@18px.bin
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

### 子集化（仅在字符集 `source/charset-common.txt` 变更时需要）

```bash
pyftsubset source/source-han-sans-sc-regular.otf \
    --text-file=source/charset-common.txt \
    --drop-tables+=DSIG \
    --output-file=dist/SourceHanSansSC-Regular-Subset.otf
```

### 烘培字形表

```bash
python3 bake.py                      # 默认字号：14、15、18 px
python3 bake.py --sizes 14 15 18 21  # 添加 21 px
```

烘培完成后提交 `dist/` 的变更，在 `pomelo-os` 里更新 `vendor/pomelo-font` 的 submodule 指针。

## 字体与字号选择

字体是**思源黑体 SC Regular**（Source Han Sans SC Regular）的精简子集，包含：

- GB2312 一级汉字（区 16–55，共 3755 字）
- ASCII 可打印字符（U+0020–U+007E）
- 中文标点与全角形式
- **界面用到的个别补充字**：不在 GB2312 一级字里的字（例如「渲」「浏」），在 `source/charset-common.txt` 末尾单独列一行补入 —— 一级字之外的常用字不多，但界面每写一句新话都可能碰到一个，碰到就补一个字、重跑一次子集化与烘焙

> 界面文案里出现的新字如果不在子集里，屏幕上画出来的是一个方框（`.notdef`），不会报错也不会让构建失败。补字的步骤见「子集化」与「烘培字形表」两节：**子集化会重排 glyph_id，所以补字之后必须重烘**，两处 `dist/` 也要一起更新。

烘培的字号对应正文用途，覆盖 app 中高频出现的字号：

| 字号 | 用途 |
| ---: | --- |
| 14 px | 设置/计算器/播放器的次要文本与数值 |
| 15 px | 启动器标签与状态栏 |
| 18 px | 终端正文 |

大字号（32/34/38/96 px 等展示字号）字符串很短，按需在线光栅化（约 1.6 ms/字），之后进 RAM 缓存，不值得烘。

## 字体许可

思源黑体以 **SIL Open Font License 1.1** 发布，全文见 `OFL.txt`。OFL 允许子集、内嵌与再分发，因此 `dist/SourceHanSansSC-Regular-Subset.otf` 可以随系统一起发布。

`bake.py` 及本仓库的其余代码以 **GPL-3.0-only** 发布，见 `LICENSE`。

## 在 pomelo-os 中的使用

本仓库作为 `pomelo-os` 的 git submodule，挂载在 `vendor/pomelo-font/`。`iced-pomelo-winit` 通过 `include_bytes!` 引用 `dist/` 中的文件，路径为 `../../pomelo-font/dist/...`（相对于 `iced-pomelo-winit/src/`）。
