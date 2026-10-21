//! `cargo run` —— 一个 WorkBuddy 账号，作为一个桌面窗口。
//!
//! ```text
//! cargo run                      # 从这一层目录：iced 的 iced_winit，和一个窗口
//! ```
//!
//! 板子是桌面模拟器：账号里存着一个令牌、今天已经签过、连了十二天 —— 所以有一张有东西可看的卡片，
//! 也有一个「已签到」的按钮。模拟器只在有人推 `tick` 的时候动，桌面没有固件那个循环，所以这里自己
//! 开一个线程推它：十毫秒一拍，因为桌面的钟是帧，而往返是按页面的 tick 数的。
//!
//! 订阅是页面自己的（[`WorkBuddy::subscription`]），这也是页面替自己做不了的一件事：窗口的钟是它的
//! 帧，板子的钟是外壳的泵。

use std::sync::Arc;
use std::time::Duration;

use pomelo_hal::Board;
use workbuddy_monitor::WorkBuddy;

fn main() -> iced::Result {
    let board = Arc::new(Board::simulated());

    {
        let board = Arc::clone(&board);

        std::thread::spawn(move || loop {
            board.tick();
            std::thread::sleep(Duration::from_millis(10));
        });
    }

    iced::application(
        move || WorkBuddy::new(Arc::clone(&board)),
        WorkBuddy::update,
        WorkBuddy::view,
    )
    .subscription(WorkBuddy::subscription)
    .title("WorkBuddy")
    .run()
}
