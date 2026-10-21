//! 标题前面那枚标志。
//!
//! 一张 PNG 在构建时烘成 RGB565 + alpha 的静态数组：这块屏没有解码器，一个 28 点的标志不该在
//! 运行时解一张图。做法与外壳磁贴那套相同，见 `../build_support/bake.rs`。

include!("../build_support/bake.rs");

fn main() {
    bake("workbuddy.png", "WORKBUDDY", 72);
}
