//! 这一页的测试：一次往返读到了什么、没有令牌时说什么、签到之后变没变。
//!
//! 用的是 `Board::simulated()` 里那个模拟账号，所以没有网络也没有板子 —— 这正是这一层存在的
//! 理由：页面在桌面能画、能测，靠的是后端可以是一个假账号。

use std::sync::Arc;
use std::time::Instant;

use pomelo_hal::Board;

use crate::{Message, WorkBuddy};

/// 一台桌面模拟的板子。
fn board() -> Arc<Board> {
    Arc::new(Board::simulated())
}

/// 把一次请求走完：板子推几帧，页面每帧收一次。
fn settle(app: &mut WorkBuddy, board: &Arc<Board>) {
    for _ in 0..8 {
        board.tick();
        app.update(Message::Tick(Instant::now()));
    }
}

#[test]
fn opening_the_page_asks_the_account_straight_away() {
    let board = board();
    let app = WorkBuddy::new(Arc::clone(&board));

    assert!(app.busy(), "打开这一页就该有一次请求在路上");
    assert!(app.report().is_none(), "还没读到，就该是空的");
    assert!(app.configured(), "模拟账号里是存了令牌的");
}

#[test]
fn a_round_trip_lands_in_the_page() {
    let board = board();
    let mut app = WorkBuddy::new(Arc::clone(&board));

    settle(&mut app, &board);

    let report = app.report().expect("一次完整的往返之后应当有报告");

    assert!(report.checked_in_today);
    assert_eq!(report.streak_days, 12);
    assert_eq!(report.daily_credit, 10);
    assert_eq!(report.access_days_left, Some(27));
    assert_eq!(
        report.credit_remaining, None,
        "余额接口还不知道，页面画破折号而不是零"
    );
}

#[test]
fn a_board_with_no_token_says_where_one_goes() {
    let board = board();
    board.workbuddy().forget().unwrap();

    let mut app = WorkBuddy::new(Arc::clone(&board));

    /* 没有令牌时 C 侧当场就给答案，所以不用推帧也有话可说。 */
    app.update(Message::Tick(Instant::now()));

    assert!(!app.configured());
    assert!(app
        .notice_text()
        .contains("/api/workbuddy/token"), "实际是：{}", app.notice_text());
}

#[test]
fn checking_in_changes_the_page() {
    let board = board();

    let mut app = WorkBuddy::new(Arc::clone(&board));
    settle(&mut app, &board);

    let before = app.report().cloned().unwrap();
    assert!(before.checked_in_today);

    app.update(Message::Checkin);
    settle(&mut app, &board);

    let after = app.report().cloned().unwrap();

    assert!(after.already_checked_in, "已经签过的那一次不是错误");
    assert_eq!(after.streak_days, before.streak_days);
}

#[test]
fn the_page_draws() {
    let board = board();
    let mut app = WorkBuddy::new(Arc::clone(&board));

    settle(&mut app, &board);

    let _ = app.view();
}
