//! Runs one package on a PC, without a window.
//!
//! ```text
//! cargo run -p app-host -- path/to/app.app [--frames 3] [--size 480x480]
//! ```
//!
//! It prints what the package says about itself, then builds frames and counts what the display list
//! holds. No renderer, no board: this answers "does this package start, and what does it draw" in the
//! loop where a package is being written, which is the question that costs the most time to answer
//! through a flash cycle.

use std::process::ExitCode;

use app_host::display::DrawCommand;
use app_host::services::DesktopServices;
use app_host::vm::host_now_ms;
use app_host::{HostedApp, Package};

fn main() -> ExitCode {
    let mut path = None;
    let mut frames = 1usize;
    let mut size = (480.0f32, 480.0f32);
    let mut arguments = std::env::args().skip(1);

    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--frames" => {
                frames = arguments
                    .next()
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(1)
                    .clamp(1, 600);
            }
            "--size" => {
                if let Some(value) = arguments.next() {
                    if let Some((width, height)) = value.split_once('x') {
                        if let (Ok(width), Ok(height)) = (width.parse(), height.parse()) {
                            size = (width, height);
                        }
                    }
                }
            }
            other => path = Some(other.to_string()),
        }
    }

    let Some(path) = path else {
        eprintln!("用法: app-host <app.app> [--frames N] [--size 480x480]");
        return ExitCode::from(2);
    };

    let package = match Package::read(std::path::Path::new(&path)) {
        Ok(package) => package,
        Err(error) => {
            eprintln!("读不了 {path}: {error}");
            return ExitCode::FAILURE;
        }
    };

    let meta = &package.meta;
    println!(
        "{} {}  ({}，{} 字节，{} KB 内存)",
        meta.name_zh,
        meta.version,
        meta.id,
        package.wasm.len(),
        meta.memory_kb
    );

    if !meta.description.is_empty() {
        println!("  {}", meta.description);
    }

    let base = std::env::temp_dir().join("hivebox-app-host-run");
    let services = DesktopServices::new(&base, meta.id.clone());

    let mut app = HostedApp::open(
        &package,
        base.join(&meta.id),
        Box::new(services),
        size,
        1.0,
        host_now_ms(),
    );

    if let Some(error) = app.error() {
        eprintln!("启动失败: {error}");
        return ExitCode::FAILURE;
    }

    let started = std::time::Instant::now();

    for _ in 0..frames {
        let Some(list) = app.frame(host_now_ms()) else {
            eprintln!(
                "第 {} 帧失败: {}",
                frames,
                app.error().unwrap_or("应用已退出")
            );
            return ExitCode::FAILURE;
        };

        println!(
            "帧: {} 条命令（{}）",
            list.commands.len(),
            summarize(&list.commands)
        );
    }

    println!(
        "{} 帧用了 {:?}，线性内存 {} 字节",
        frames,
        started.elapsed(),
        app.memory_bytes()
    );

    ExitCode::SUCCESS
}

fn summarize(commands: &[DrawCommand]) -> String {
    let mut rects = 0;
    let mut rounded = 0;
    let mut lines = 0;
    let mut texts = 0;
    let mut icons = 0;
    let mut clips = 0;

    for command in commands {
        match command {
            DrawCommand::Rect { .. } | DrawCommand::Circle { .. } => rects += 1,
            DrawCommand::RoundedRect { .. } | DrawCommand::RoundedStroke { .. } => rounded += 1,
            DrawCommand::Line { .. } => lines += 1,
            DrawCommand::Text { .. } => texts += 1,
            DrawCommand::Icon { .. } => icons += 1,
            DrawCommand::PushClip { .. } | DrawCommand::PopClip => clips += 1,
            DrawCommand::Clear { .. } => {}
        }
    }

    format!("矩形 {rects}，圆角 {rounded}，线 {lines}，文字 {texts}，图标 {icons}，裁剪 {clips}")
}
