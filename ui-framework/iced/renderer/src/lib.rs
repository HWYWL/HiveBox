//! The official renderer for iced.
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(feature = "wgpu-bare")]
pub use iced_wgpu as wgpu;

pub mod fallback;

pub use iced_graphics as graphics;
pub use iced_graphics::core;

#[cfg(feature = "geometry")]
pub use iced_graphics::geometry;

/// The default graphics renderer for [`iced`].
///
/// [`iced`]: https://github.com/iced-rs/iced
pub type Renderer = renderer::Renderer;

/// The default graphics compositor for [`iced`].
///
/// [`iced`]: https://github.com/iced-rs/iced
pub type Compositor = renderer::Compositor;

#[cfg(all(feature = "wgpu-bare", feature = "tiny-skia"))]
mod renderer {
    pub type Renderer = crate::fallback::Renderer<
        iced_wgpu::Renderer,
        iced_tiny_skia::Renderer,
    >;

    pub type Compositor = crate::fallback::Compositor<
        iced_wgpu::window::Compositor,
        iced_tiny_skia::window::Compositor,
    >;
}

#[cfg(all(feature = "wgpu-bare", not(feature = "tiny-skia")))]
mod renderer {
    pub type Renderer = iced_wgpu::Renderer;
    pub type Compositor = iced_wgpu::window::Compositor;
}

#[cfg(all(not(feature = "wgpu-bare"), feature = "tiny-skia"))]
mod renderer {
    pub type Renderer = iced_tiny_skia::Renderer;
    pub type Compositor = iced_tiny_skia::window::Compositor;
}

/// The integrator brings its own surface, its own compositor *and* its own renderer: ours is
/// `iced-pomelo-gfx`, which records a frame's commands rather than rasterising them, so that the
/// platform layer can replay them into a panel that is not a window. The unit type is still the
/// compositor name for the same reason as below -- neither this crate nor iced's facade ever
/// instantiates it when the surface belongs to someone else.
///
/// This takes precedence over `custom` below, and the two are otherwise the same idea. A build of
/// this stack has both on: `custom` is how the host says "we bring the surface", `pomelo` is how it
/// says the renderer is ours.
#[cfg(all(
    feature = "pomelo",
    not(any(feature = "wgpu-bare", feature = "tiny-skia"))
))]
mod renderer {
    pub type Renderer = iced_pomelo_gfx::Renderer;
    pub type Compositor = ();
}

#[cfg(not(any(
    feature = "wgpu-bare",
    feature = "tiny-skia",
    feature = "custom",
    feature = "pomelo"
)))]
mod renderer {
    #[cfg(not(debug_assertions))]
    compile_error!(
        "Cannot compile `iced_renderer` in release mode \
        without a renderer feature enabled. \
        Enable one of the `wgpu`, `tiny-skia` or `pomelo` features."
    );

    pub type Renderer = ();
    pub type Compositor = ();
}

/// The integrator brings its own surface and compositor, so this crate's `Compositor` alias is
/// never used. A [`Renderer`] still has to be named, because the unit type only implements the
/// renderer traits under `debug_assertions` -- which is the real reason the guard above exists,
/// and why opting out of it has to mean "use this one" rather than "use nothing".
#[cfg(all(
    feature = "custom",
    not(feature = "pomelo"),
    not(any(feature = "wgpu-bare", feature = "tiny-skia"))
))]
mod renderer {
    pub type Renderer = iced_tiny_skia::Renderer;
    pub type Compositor = ();
}
