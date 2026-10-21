//! WorkBuddy —— 一个账号签没签到、连了几天、令牌还剩多久。
//!
//! 这是「服务器」和「云服务」之外的第三块牌子，问的是同一种问题：**那台不属于这块板子的东西，
//! 现在怎么样。** 只是这回的对象是一个账号 —— 每天签一次到能攒积分，令牌隔一阵子会过期，而人
//! 通常想不起来这两件事。
//!
//! # 为什么没有「剩余积分 / 总额」
//!
//! 因为那个接口没人读过。签到活动给的是**活动自己的累计**（`activity_credits`），不是账户余额：
//! 拿它冒充余额，就是画一个看着精确、其实是错的数字。所以这两格是破折号，等接口知道了再填 ——
//! 见 [`WorkBuddyReport::credit_remaining`]。
//!
//! # 令牌不在这页上
//!
//! 两 KB 的 JWT，网页上粘一次就够了，不会有人想在 480 点的屏幕上敲它。这页只从后端拿一个掩码
//! （`…a1b2`）和主机名，用来说「存了一个」。
//!
//! # 一次请求怎么走
//!
//! 和云服务那张页一样：`start` 立刻返回，页面每帧看 `state`，不 `Running` 的那一帧把答案取走。
//! 一次往返是一次 TLS 握手加一个 POST，等它就是一页停在那儿不画。

use std::sync::Arc;
use std::time::Instant;

use iced::widget::{button, column, container, row, text, Space};
use iced::{Border, Color, Element, Length, Subscription, Task, Theme};
use pomelo_hal::types::{WorkBuddyAccount, WorkBuddyAnswer, WorkBuddyFault, WorkBuddyReport, WorkBuddyState};
use pomelo_hal::Board;
use pomelo_widgets::{Language, SystemPreferences, ThemeMode, FONT_BASE};

/// 一次「今天签过了没」的问。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// 读一遍活动。
    Status,
    /// 今天签一下。已经签过的那次不是错误。
    Checkin,
}

/// 这页能收到的消息。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Message {
    /// 一帧。轮询请求、让「已签」保持新鲜都靠它。
    Tick(Instant),
    /// 重新读一遍。
    Refresh,
    /// 签到。
    Checkin,
}

/// 上一次请求留下的那句话说完了没有 —— 页面只记得最近一句。
struct Notice {
    text: String,
    /// 是错误（红）还是结果（灰）。
    failed: bool,
}

/// 这一页。
pub struct WorkBuddy {
    board: Arc<Board>,
    /// 最近一次读到的报告。没有就是还没读到过。
    report: Option<WorkBuddyReport>,
    /// 令牌存了没有，以及它的掩码。
    account: WorkBuddyAccount,
    /// 请求走到哪了。
    state: WorkBuddyState,
    /// 最近一次请求的结果，一句话。
    notice: Option<Notice>,
    /// 这一帧的答案是不是已经收过了。
    collected: bool,
    /// 界面偏好：语言与主题。
    preferences: SystemPreferences,
    /// 下一次自己再读一遍的时刻，`None` 表示这页不自己动。
    next_read: Option<Instant>,
}

/// 自己重读的间隔：一分钟。
///
/// 签到一天一次，令牌按天过期 —— 一分钟一问已经比这两件事都勤了，再勤就是拿板子的电换一个不会
/// 变的数字。
const EVERY: std::time::Duration = std::time::Duration::from_secs(60);

impl WorkBuddy {
    /// 打开这一页，并且立刻问一次。
    pub fn new(board: Arc<Board>) -> Self {
        let mut app = Self {
            board,
            report: None,
            account: WorkBuddyAccount::default(),
            state: WorkBuddyState::Idle,
            notice: None,
            collected: true,
            preferences: SystemPreferences::new(Language::Chinese, ThemeMode::Dark),
            next_read: None,
        };

        app.account = app.board.workbuddy().account();
        app.ask(Action::Status);

        app
    }

    /// 让这页用外壳的偏好画。
    pub fn set_preferences(&mut self, preferences: SystemPreferences) {
        self.preferences = preferences;
    }

    /// 最近一次读到的报告。
    pub fn report(&self) -> Option<&WorkBuddyReport> {
        self.report.as_ref()
    }

    /// 令牌存了没有。
    pub fn configured(&self) -> bool {
        self.account.configured()
    }

    /// 请求是不是还在路上 —— 也就是这页要不要继续要帧。
    pub fn busy(&self) -> bool {
        matches!(self.state, WorkBuddyState::Running) || !self.collected
    }

    /// 最近留下的那句话，给外壳和测试看。空串表示没有。
    pub fn notice_text(&self) -> &str {
        self.notice.as_ref().map_or("", |notice| notice.text.as_str())
    }

    /// 这页还有没有只有帧能推动的事。
    pub fn waiting(&self) -> bool {
        self.busy()
    }

    /// 一次请求，立刻返回。
    fn ask(&mut self, action: Action) {
        self.account = self.board.workbuddy().account();

        let asked = match action {
            Action::Status => pomelo_hal::types::WorkBuddyAction::Status,
            Action::Checkin => pomelo_hal::types::WorkBuddyAction::Checkin,
        };

        if self.board.workbuddy().start(asked).is_err() {
            /* 已经在问一件了：手上这一件还没收，等它。 */
            return;
        }

        self.collected = false;
        self.state = WorkBuddyState::Running;
    }

    /// 处理一条消息。
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Tick(now) => self.tick(now),
            Message::Refresh => self.ask(Action::Status),
            Message::Checkin => self.ask(Action::Checkin),
        }
    }

    /// 一帧：读答案，到点自己再问一次。
    fn tick(&mut self, now: Instant) {
        self.pull();

        if self.busy() {
            return;
        }

        let due = match self.next_read {
            Some(at) => now >= at,
            // 头一次：把下一次的时刻记下来就走，不在这里再问一遍 —— 这一帧刚收回来的答案
            // 还没画出去，马上又问一次就是把它盖掉。
            None => {
                self.next_read = Some(now + EVERY);
                false
            }
        };

        if due {
            self.next_read = Some(now + EVERY);
            self.ask(Action::Status);
        }
    }

    /// 把已经落地的答案收走，别的不做。
    fn pull(&mut self) {
        let state = self.board.workbuddy().state();
        self.state = state;

        if matches!(state, WorkBuddyState::Running) || self.collected {
            return;
        }

        self.collected = true;

        let answer = self.board.workbuddy().take_answer().ok();

        if let Some(answer) = answer {
            /* 收回来的那一帧，后端已经回到 Idle；这页说 `Done` 是在说一件刚发生的事，下一帧就
             * 不算数了。失败留着：那是画给人看的东西，不是等待。 */
            if matches!(self.state, WorkBuddyState::Done) {
                self.state = WorkBuddyState::Idle;
            }

            self.receive(answer);
        }
    }

    /// 一条答案。
    fn receive(&mut self, answer: WorkBuddyAnswer) {
        match answer {
            WorkBuddyAnswer::Report(report) => {
                let credited = report.credited.filter(|credit| *credit > 0);
                let already = report.already_checked_in;

                self.notice = match (credited, already) {
                    (Some(credit), _) => Some(Notice {
                        text: format!("今天签到 +{credit} 积分"),
                        failed: false,
                    }),
                    (None, true) => Some(Notice {
                        text: String::from("今天已经签过了"),
                        failed: false,
                    }),
                    _ => None,
                };

                self.report = Some(report);
            }
            WorkBuddyAnswer::Refused { fault, message } => {
                self.notice = Some(Notice {
                    text: sentence(fault, &message),
                    failed: true,
                });
            }
        }

        self.account = self.board.workbuddy().account();
    }

    /// 这页的帧。只在有事在路上的时候要 —— 见 [`WorkBuddy::waiting`]。
    pub fn subscription(&self) -> Subscription<Message> {
        if !self.waiting() {
            return Subscription::none();
        }

        iced::window::frames().map(|_| Message::Tick(Instant::now()))
    }

    /// 画出来。
    pub fn view(&self) -> Element<'_, Message> {
        let theme = self.preferences.theme;

        let mut body = column![].spacing(6).width(Length::Fill);

        body = body.push(self.header(theme));
        body = body.push(self.checkin_card(theme));
        body = body.push(self.panel_card(theme));

        if let Some(notice) = &self.notice {
            body = body.push(text(notice.text.as_str()).size(FONT_BASE * 0.82).style(
                move |theme: &Theme| iced::widget::text::Style {
                    color: Some(if notice.failed { danger(theme) } else { muted(theme) }),
                },
            ));
        }

        body = body.push(self.actions());

        container(body)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(14)
            .into()
    }

    /// 顶上那一行：名字，和令牌存了没有。
    fn header(&self, _theme: ThemeMode) -> Element<'_, Message> {
        let who = if self.account.configured() {
            format!("{} · {}", self.account.host, self.account.masked)
        } else {
            String::from("没有令牌 · 在网页上填 /api/workbuddy/token")
        };

        let title = text("WorkBuddy").size(FONT_BASE * 1.3);

        let mut head = row![title].spacing(8).align_y(iced::Alignment::Center);
        head = head.push(Space::new().width(Length::Fill));
        head = head.push(
            text(if self.busy() { "读取中…" } else { "" })
                .size(FONT_BASE * 0.9)
                .style(move |theme: &Theme| iced::widget::text::Style {
                    color: Some(muted(theme)),
                }),
        );

        column![
            head,
            text(who)
                .size(FONT_BASE * 0.78)
                .style(move |theme: &Theme| iced::widget::text::Style {
                    color: Some(muted(theme)),
                }),
        ]
        .spacing(4)
        .width(Length::Fill)
        .into()
    }

    /// 今天签没签，连了几天。
    fn checkin_card(&self, theme: ThemeMode) -> Element<'_, Message> {
        let (value, detail) = match self.report.as_ref() {
            None => (String::from("—"), String::from("还没读到")),
            Some(report) if report.checked_in_today => (
                String::from("今天已签"),
                format!("连续 {} 天 · 每天 {} 积分", report.streak_days, report.daily_credit),
            ),
            Some(report) => (
                String::from("今天还没签"),
                format!("连续 {} 天 · 签到得 {} 积分", report.streak_days, report.daily_credit),
            ),
        };

        let activity = self
            .report
            .as_ref()
            .map(|report| report.activity.clone())
            .filter(|name| !name.is_empty());

        let mut body = column![
            text(value).size(FONT_BASE * 1.2),
            text(detail)
                .size(FONT_BASE * 0.85)
                .style(move |theme: &Theme| iced::widget::text::Style {
                    color: Some(muted(theme)),
                }),
        ]
        .spacing(4);

        if let Some(activity) = activity {
            body = body.push(
                text(activity)
                    .size(FONT_BASE * 0.85)
                    .style(move |theme: &Theme| iced::widget::text::Style {
                        color: Some(muted(theme)),
                    }),
            );
        }

        card(body.into(), theme)
    }

    /// 积分与令牌：四行，一行一件事。
    ///
    /// 合成一张卡而不是两张，是这块屏幕的高度决定的：四百三十点里挤着页头、两张卡和两个按钮，
    /// 一张只有两行的卡就是给按钮挤出去的二十个像素。
    fn panel_card(&self, theme: ThemeMode) -> Element<'_, Message> {
        let report = self.report.as_ref();

        let activity = report.map_or(0, |report| report.activity_credits);
        let remaining = report
            .and_then(|report| report.credit_remaining)
            .map_or_else(|| String::from("—"), |credit| credit.to_string());
        let total = report
            .and_then(|report| report.credit_total)
            .map_or_else(|| String::from("—"), |credit| credit.to_string());

        let days = |value: Option<i64>| match value {
            Some(days) => format!("{days} 天"),
            None => String::from("—"),
        };

        let access = days(report.and_then(|report| report.access_days_left));
        let refresh = days(report.and_then(|report| report.refresh_days_left));

        /* 行名要活得比这一行久：`line` 借的是它，而临时值的借用活不到 `column!` 结束。 */
        let token_label = format!("令牌 · 刷新 {refresh}");

        let body = column![
            line(String::from("活动累计"), activity.to_string(), theme),
            line(String::from("剩余"), remaining, theme),
            line(String::from("总额"), total, theme),
            line(token_label, access, theme),
        ]
        .spacing(5);

        card(body.into(), theme)
    }

    /// 底下两个按钮。
    fn actions(&self) -> Element<'_, Message> {
        let signed = self
            .report
            .as_ref()
            .is_some_and(|report| report.checked_in_today);

        let checkin = if signed {
            button(text("已签到").size(FONT_BASE * 0.95)).padding([9, 16]).style(button::secondary)
        } else {
            button(text("签到").size(FONT_BASE * 0.95))
                .padding([9, 16])
                .on_press(Message::Checkin)
        };

        let refresh = button(text("重读").size(FONT_BASE * 0.95))
            .padding([9, 16])
            .style(button::secondary)
            .on_press(Message::Refresh);

        row![checkin, refresh]
            .spacing(10)
            .width(Length::Fill)
            .into()
    }
}

/// 只有一个模拟板子时才有的默认值：固件里没有「随便一个板子」这回事，一页总是被外壳
/// 带着真实的板子打开的。
#[cfg(not(target_os = "espidf"))]
impl Default for WorkBuddy {
    fn default() -> Self {
        Self::new(Arc::new(Board::simulated()))
    }
}

/// 一行「名字 …… 值」。
///
/// 名字是 `String` 而不是 `&str`：其中一行是拼出来的（「令牌 · 刷新 29 天」），而一个借来的名字
/// 活不过这一行自己的作用域。
fn line<'a>(label: String, value: String, _theme: ThemeMode) -> Element<'a, Message> {
    row![
        text(label)
            .size(FONT_BASE * 0.95)
            .style(|theme: &Theme| iced::widget::text::Style {
                color: Some(muted(theme)),
            }),
        Space::new().width(Length::Fill),
        text(value).size(FONT_BASE * 1.05),
    ]
    .width(Length::Fill)
    .align_y(iced::Alignment::Center)
    .into()
}

/// 一张卡片：一块底色，一道细边。
fn card<'a>(body: Element<'a, Message>, _theme: ThemeMode) -> Element<'a, Message> {
    container(body)
        .width(Length::Fill)
        .padding(9)
        .style(|theme: &Theme| {
            let palette = theme.extended_palette();

            container::Style {
                background: Some(palette.background.weak.color.into()),
                border: Border {
                    color: palette.background.strong.color,
                    width: 1.0,
                    radius: 14.0.into(),
                },
                ..container::Style::default()
            }
        })
        .into()
}

/// 次要文字的颜色。
fn muted(theme: &Theme) -> Color {
    theme.extended_palette().background.base.text.scale_alpha(0.62)
}

/// 出错时那一句话的颜色。
fn danger(theme: &Theme) -> Color {
    theme.extended_palette().danger.base.color
}

/// 一个故障码，画成一页能读的句子。
///
/// 页面自己的词，和 `WorkBuddyFault::code` 那一套一一对应：码是给日志和网页看的，句子是给人看的。
pub fn sentence(fault: WorkBuddyFault, message: &str) -> String {
    if !message.is_empty() {
        return message.to_string();
    }

    match fault {
        WorkBuddyFault::Failed => String::from("请求没有成功"),
        WorkBuddyFault::Args => String::from("这个动作不认识"),
        WorkBuddyFault::NoToken => String::from("还没有令牌，在网页上填 /api/workbuddy/token"),
        WorkBuddyFault::NoNetwork => String::from("板子没连上网络"),
        WorkBuddyFault::Unreachable => String::from("连不上 WorkBuddy，等一下再试"),
        WorkBuddyFault::Tls => String::from("TLS 握手没过（证书或时间）"),
        WorkBuddyFault::Http => String::from("接口回了一个用不了的状态"),
        WorkBuddyFault::Auth => String::from("令牌过期或不对，去客户端换一个"),
        WorkBuddyFault::Api => String::from("账号拒绝了这次请求"),
        WorkBuddyFault::Storage => String::from("板子存不下这个答案"),
    }
}

/// 一个只用来让 [`Message`] 能在 `Task` 里出现的空任务。
pub fn none() -> Task<Message> {
    Task::none()
}

#[cfg(test)]
mod tests;
