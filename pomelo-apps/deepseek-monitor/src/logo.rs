//! 应用自己的标志，画在标题前面。
//!
//! 这块屏上没有 PNG 解码器，也没有 `iced/image`（那个 feature 在设备上没开）：一张图在构建时
//! 就烘成 RGB565 + alpha 的数组，运行时交给渲染器的 `draw_bitmap_565` 直接铺上去。做法与外壳
//! 磁贴那套一致，见 `../build_support/bake.rs`。

include!(concat!(env!("OUT_DIR"), "/baked_icon.rs"));

#[allow(unused_imports)]
use iced::widget::{container, Space};
#[allow(unused_imports)]
use iced::{Element, Length, Size};

/// 标题旁边的那枚标志，边长 `size` 点。
#[cfg(not(feature = "desktop"))]
pub fn logo<'a, Message: 'a>(size: f32) -> Element<'a, Message> {
    Element::new(Logo { size })
}

/// 桌面预览时没有 `draw_bitmap_565`（那是 pomelo 的渲染器），留一个同尺寸的空位，
/// 免得 `cargo test --workspace` 和本地预览跟着挂。
#[cfg(feature = "desktop")]
pub fn logo<'a, Message: 'a>(size: f32) -> Element<'a, Message> {
    container(Space::new())
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .into()
}

#[cfg(not(feature = "desktop"))]
struct Logo {
    size: f32,
}

#[cfg(not(feature = "desktop"))]
impl<Message, Theme> iced::advanced::widget::Widget<Message, Theme, iced::Renderer> for Logo {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(self.size), Length::Fixed(self.size))
    }

    fn layout(
        &mut self,
        _tree: &mut iced::advanced::widget::Tree,
        _renderer: &iced::Renderer,
        limits: &iced::advanced::layout::Limits,
    ) -> iced::advanced::layout::Node {
        iced::advanced::layout::Node::new(limits.resolve(
            Length::Fixed(self.size),
            Length::Fixed(self.size),
            Size::new(self.size, self.size),
        ))
    }

    fn draw(
        &self,
        _tree: &iced::advanced::widget::Tree,
        renderer: &mut iced::Renderer,
        _theme: &Theme,
        _style: &iced::advanced::renderer::Style,
        layout: iced::advanced::Layout<'_>,
        _cursor: iced::advanced::mouse::Cursor,
        _viewport: &iced::Rectangle,
    ) {
        renderer.draw_bitmap_565(
            layout.bounds(),
            baked::DEEPSEEK_SIZE,
            baked::DEEPSEEK_SIZE,
            &baked::DEEPSEEK_RGB,
            &baked::DEEPSEEK_ALPHA,
        );
    }
}
