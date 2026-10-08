# 待办规划: 利用 ESP32-S3 Xtensa LX7 PIE 128 位矢量指令加速 RGB565 渲染与混合

- **模块**: `pomelo-gfx` (`src/color.rs`, `src/raster.rs`, `src/paint.rs`)
- **硬件目标**: ESP32-S3 (双核 Xtensa LX7 @ 240MHz, 128 位 PIE 矢量扩展寄存器 `q0`~`q7`)
- **创建日期**: 2026-10-01
- **状态**: 规划中 (Proposed)

---

## 一、 背景与现状分析

### 1.1 现状与优势
`pomelo-gfx` 是一款直接面向 16 位面板原生像素格式（RGB565）的轻量级 2D 光栅化引擎。
* **原生直绘优势**：通过绕开通用 2D 图形库（如 Skia / tiny-skia）所必须的 4 字节 RGBA8888 画布和二次全屏格式转换开销，纯色矩形填充已实现 ~0.12 µs/像素的高吞吐；
* **硬件浮点利用**：在坐标变换、贝塞尔曲线扁平化（`src/geometry.rs`, `src/path.rs`）中大量使用的 `f32` 运算，已被当前编译工具链（`xtensa-esp32s3-espidf`）自动映射为 Xtensa LX7 的硬件单周期单精度浮点指令（FPU）。

### 1.2 性能瓶颈：纯标量颜色混合（Scalar Blending）
尽管浮点已被硬件化，**像素级的光栅化与颜色混合仍然全部为标量循环**：
1. **逐像素拆解与拼装**：
   在 [src/color.rs](file:///home/edward/Projects/pomelo-project/pomelo-gfx/src/color.rs) 的 `blend_rgb565` 中，每一个像素的 Alpha 混合都必须经历多次移位（`>> 11`, `>> 5`）、整型乘加和除以 255 的运算：
   ```rust
   let r = ((r_s * a + r_d * inv_a + 127) / 255) as u16;
   let g = ((g_s * a + g_d * inv_a + 127) / 255) as u16;
   let b = ((b_s * a + b_d * inv_a + 127) / 255) as u16;
   (r << 11) | (g << 5) | b
   ```
2. **字符与图标蒙版混合（Text / Icon Blit）**：
   在 [src/raster.rs](file:///home/edward/Projects/pomelo-project/pomelo-gfx/src/raster.rs) 的 `blit_mask` 中，文字渲染遍历 8-bit Alpha 蒙版，逐个像素调用标量 `blend_rgb565`，未发生批量并行处理。
3. **编译器自动矢量化的局限**：
   当前 Rust Xtensa 编译器分支（`esp198` / rustc 1.98）的 LLVM 自动矢量化器（Auto-vectorizer）无法自动生成 ESP32-S3 专有的 PIE（Processor Interface Extension）128 位矢量指令。128 位的向量寄存器（`q0`~`q7`）在光栅化阶段处于**闲置状态**。

---

## 二、 Xtensa PIE 矢量加速技术方案

ESP32-S3 的 Xtensa LX7 核心具备 8 个 128 位矢量寄存器（`q0` 至 `q7`）。一个 128 位寄存器恰好可以同时容纳 **8 个 16 位 RGB565 像素**（$8 \times 2\text{ bytes} = 16\text{ bytes} = 128\text{ bits}$）。

### 2.1 加速切入点与核心函数

| 热点场景 | 目标函数 | 当前实现 | 矢量化改造方案 |
| :--- | :--- | :--- | :--- |
| **文字 / 图标抗锯齿蒙版** | `raster::blit_mask` | 逐像素读取 mask 并调用 `blend_rgb565` | 一次载入 8 字节 mask 与 8 个 dst 像素，单周期完成 8 像素的 Alpha 并行混合 |
| **半透明矩形 / 图层填充** | `raster::fill_rect` (alpha < 255) | 单像素循环 `blend_rgb565` | 批处理 8 像素的恒定颜色 Alpha 混合（定点乘加） |
| **不透明纯色横条填充** | `raster::fill_u16_slice` | 32 位整型写（每次 2 像素） | 升级为 128 位矢量写入（每次 8 像素写，吞吐提升 4 倍） |
| **渐变着色器** | `paint::LinearGradientShader` | 逐像素浮点/整型插值 | 4 像素或 8 像素批处理颜色步进插值 |

### 2.2 算法优化：快速定点数除法（Fast Integer Division）
目前标量实现使用了 `(x + 127) / 255`。在矢量指令中除法极其昂贵，应转换为**定点乘法与移位近似**：
$$\frac{x}{255} \approx \frac{x \times 257 + 257}{65536} = ((x + 1) + (x + 1) \times 256) \gg 16$$
或者最常用的嵌入式经验公式：
$$((x + 1) \times \text{alpha}) \gg 8$$
这使得 8 个像素的颜色混合可以纯粹由 PIE 矢量的 `vadd`、`vmul` 与 `vsrli` 指令在极低周期内完成。

### 2.3 架构设计：条件编译与平台隔离

为了保持 `pomelo-gfx` 在宿主机桌面端（`cargo test`、模拟器）的编译兼容性，矢量实现必须通过条件编译严格隔离：

```text
src/
├── arch/
│   ├── mod.rs               # 导出平台无关的 span 混合 trait
│   ├── fallback.rs          # 纯 Safe Rust 标量实现（用于 x86_64, aarch64 等）
│   └── xtensa_pie.rs        # #[cfg(target_arch = "xtensa")] 下的内联汇编核心
```

提供统一内部 API：
```rust
// 批量处理连续行上的像素混合
pub(crate) fn blend_span_rgb565(dst: &mut [u16], src: u16, alpha: u8);

// 批量处理 8-bit Alpha 蒙版混合
pub(crate) fn blit_mask_span(dst: &mut [u16], mask: &[u8], src: u16, alpha: u8);
```

---

## 三、 预期性能收益

1. **文字与图标排版渲染性能提升 2x ~ 3x**：
   UI 中大部分脏区为状态栏时间、文字标签或图标刷新。8 像素并行批处理可大幅消除循环开销与分量解包开销。
2. **半透明遮罩与动画帧率翻倍**：
   半透明弹窗、遮罩背景或渐变进度条从当前 ~1.5 µs/像素压缩至 ~0.4 µs/像素以内。
3. **释放 Core 0 算力**：
   降低 GUI 线程绘制一帧的耗时（`paint time`），为 Rune 脚本虚拟机或复杂的组件布局计算留出更多裕量。

---

## 四、 实施路线规划 (Phases)

- [ ] **Phase 1: 算法预演（纯 Rust 定点数优化）**
  - 在 `src/color.rs` 中实现无需除法的快速定点数 `blend_rgb565_fast`；
  - 验证像素误差与视觉效果，确保无偏色或溢出边界 Bug。
- [ ] **Phase 2: 搭建 `arch` 抽象层与基准测试**
  - 抽取 `blend_span` 与 `blit_mask_span` 批处理接口；
  - 建立基准测试（Benchmark），测算当前纯标量在真机上的纳秒/像素开销。
- [ ] **Phase 3: 实现 Xtensa PIE 矢量内联汇编**
  - 使用 `core::arch::asm!` 编写 128 位对齐块的 PIE 汇编指令；
  - 处理首尾非 8 像素对齐边缘的 fallback 逻辑。
- [ ] **Phase 4: 固件联调与性能复测**
  - 编译进固件并开启 `profile` 特性，测量真实 UI 界面在 480×480 分辨率下的每帧 `paint` 耗时变化。
