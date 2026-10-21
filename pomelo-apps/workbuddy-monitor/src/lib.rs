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

mod logo;

use std::sync::Arc;
use std::time::Instant;

use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{Alignment, Border, Color, Element, Length, Subscription, Task, Theme};
use pomelo_hal::types::{WorkBuddyAccount, WorkBuddyAnswer, WorkBuddyFault, WorkBuddyReport, WorkBuddyState};
use pomelo_hal::Board;
use pomelo_material_symbols::{self as icons, Icon};
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
    ///
    /// 一屏放得下就不再滚，放不下就滚 —— 这一页的内容从四行长到了三张卡，而面板的高度是别人
    /// 定的。滚动条只在内容真的更长时出现，所以放得下的时候它和没有一样。
    pub fn view(&self) -> Element<'_, Message> {
        let theme = self.preferences.theme;

        let mut body = column![].spacing(6).width(Length::Fill);

        body = body.push(self.header(theme));
        body = body.push(self.checkin_card(theme));
        body = body.push(self.credits_card(theme));
        body = body.push(self.facts_card(theme));

        if let Some(notice) = &self.notice {
            body = body.push(text(notice.text.as_str()).size(FONT_BASE * 0.82).style(
                move |theme: &Theme| iced::widget::text::Style {
                    color: Some(if notice.failed { danger(theme) } else { muted(theme) }),
                },
            ));
        }

        body = body.push(self.actions());

        scrollable(container(body).width(Length::Fill).padding(10))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// 顶上那一行：名字，和令牌存了没有。
    fn header(&self, _theme: ThemeMode) -> Element<'_, Message> {
        let who = if self.account.configured() {
            format!("{} · {}", self.account.host, self.account.masked)
        } else {
            String::from("没有令牌 · 在网页上填 /api/workbuddy/token")
        };

        let title = text("WorkBuddy").size(FONT_BASE * 1.15);

        let mut head = row![logo::logo(FONT_BASE * 1.25), title]
            .spacing(8)
            .align_y(iced::Alignment::Center);
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
                format!(
                    "连续 {} 天 · 每天 {} 积分 · 累计 {}",
                    report.streak_days, report.daily_credit, report.activity_credits
                ),
            ),
            Some(report) => (
                String::from("今天还没签"),
                format!(
                    "连续 {} 天 · 签到得 {} · 累计 {}",
                    report.streak_days, report.daily_credit, report.activity_credits
                ),
            ),
        };

        /* 签过的用一枚打勾，没签的用一份待办：这是整页唯一一个「今天做没做」的问题，图标先回答
         * 一半，字再回答另一半。 */
        let mark = if self.report.as_ref().is_some_and(|report| report.checked_in_today) {
            Icon::CHECK_CIRCLE
        } else {
            Icon::EVENT_AVAILABLE
        };

        let body = column![
            row![
                icon(mark, FONT_BASE * 1.35, brand),
                text(value).size(FONT_BASE * 1.2),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            text(detail).size(FONT_BASE * 0.85).style(dim),
        ]
        .spacing(4);

        card(body.into(), theme)
    }

    /// 积分：一个大的剩余数，一条剩余率，一句总额。
    ///
    /// 原来这里是一行「剩余 / 总额」。两个数并排说的是同一件事的两头，却让人自己去做那道除法 ——
    /// 一条按比例画出来的带子把这道除法做掉，剩下的是「还剩多少」这一个判断。
    fn credits_card(&self, theme: ThemeMode) -> Element<'_, Message> {
        let report = self.report.as_ref();

        let remaining = report.and_then(|report| report.credit_remaining);
        let total = report.and_then(|report| report.credit_total);

        let one_decimal = |value: Option<f32>| match value {
            Some(value) => format!("{value:.1}"),
            None => String::from("—"),
        };

        let ratio = match (remaining, total) {
            (Some(remaining), Some(total)) if total > 0.0 => (remaining / total).clamp(0.0, 1.0),
            _ => 0.0,
        };

        let percent = match (remaining, total) {
            (Some(_), Some(total)) if total > 0.0 => format!("{:.0}%", ratio * 100.0),
            _ => String::from("—"),
        };

        /* 一条带子的颜色是这三档里唯一一个会变的东西：用光了是红的，快用光了是黄的，其余是那个
         * 绿。它变的不是数字，是让人抬眼就看见的那一下。 */
        /* 本月那一行是 API 的 `TotalDosage`：它自己会滞后两三个小时，所以说的是「本月」，
         * 不能冒充「今天」——今天的数在下一张卡里，是逐条数出来的。 */
        let month = match (
            report.and_then(|report| report.usage_kinds),
            report.and_then(|report| report.usage_calls),
        ) {
            (Some(kinds), Some(calls)) => format!("本月 {kinds} 类 · {calls} 次"),
            _ => String::new(),
        };

        let pigment: fn(&Theme) -> Color = if ratio < 0.08 {
            danger
        } else if ratio < 0.25 {
            warn
        } else {
            brand
        };

        let body = column![
            row![
                icon(Icon::SAVINGS, FONT_BASE * 1.1, brand),
                text(String::from("剩余积分"))
                    .size(FONT_BASE * 0.9)
                    .style(dim),
                Space::new().width(Length::Fill),
                text(percent).size(FONT_BASE * 0.85).style(dim),
            ]
            .align_y(Alignment::Center)
            .spacing(6),
            row![
                text(one_decimal(remaining)).size(FONT_BASE * 1.75),
                Space::new().width(Length::Fill),
                text(format!("共 {}", one_decimal(total)))
                    .size(FONT_BASE * 0.85)
                    .style(dim),
            ]
            .align_y(Alignment::End),
            bar(ratio, pigment),
            text(month)
                .size(FONT_BASE * 0.78)
                .style(dim)
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Right),
        ]
        .spacing(6);

        card(body.into(), theme)
    }

    /// 三件事各一行：这个月用了多少、最近哪天到期、令牌还剩几天。
    ///
    /// 一行一件，且每行都有一个图标 —— 三行的名字都是四个字，没有图标的行读起来是一列等长的
    /// 汉字，有图标的那一列一眼就能扫过去。
    fn facts_card(&self, theme: ThemeMode) -> Element<'_, Message> {
        let report = self.report.as_ref();

        /* 今日是这一页唯一会变的数：请求数一次一次数出来，消耗是一个一个 credit 加出来的。
         * 它的单位是「分」而不是 token —— 账号接口里只有 credit，没有 token。 */
        let today = match (
            report.and_then(|report| report.today_requests),
            report.and_then(|report| report.today_credit),
        ) {
            (Some(requests), Some(credit)) => format!("{requests} 次 · {credit:.1} 分"),
            (Some(requests), None) => format!("{requests} 次"),
            (None, Some(credit)) => format!("{credit:.1} 分"),
            _ => String::from("—"),
        };

        let expiry = match (report.map(|report| report.soonest_expiry.as_str()), report.and_then(|report| report.soonest_credit)) {
            (Some(date), Some(credit)) if !date.is_empty() => format!("{date} · {credit:.0} 分"),
            (Some(date), None) if !date.is_empty() => String::from(date),
            _ => String::from("—"),
        };

        let days = |value: Option<i64>| match value {
            Some(days) => format!("{days} 天"),
            None => String::from("—"),
        };

        let access = days(report.and_then(|report| report.access_days_left));
        let refresh = days(report.and_then(|report| report.refresh_days_left));

        let body = column![
            fact(Icon::INSIGHTS, String::from("今日消耗"), today),
            fact(
                Icon::CALENDAR_MONTH,
                String::from("最近到期"),
                expiry
            ),
            fact(Icon::KEY, format!("令牌 · 刷新 {refresh}"), access),
        ]
        .spacing(7);

        card(body.into(), theme)
    }

    /// 底下两个按钮。
    fn actions(&self) -> Element<'_, Message> {
        let signed = self
            .report
            .as_ref()
            .is_some_and(|report| report.checked_in_today);

        let label = |glyph: Icon, name: &'static str| {
            row![
                icon(glyph, FONT_BASE * 0.95, brand),
                text(name).size(FONT_BASE * 0.95),
            ]
            .spacing(6)
            .align_y(Alignment::Center)
        };

        let checkin = if signed {
            button(label(Icon::CHECK, "已签到"))
                .padding([9, 16])
                .style(button::secondary)
        } else {
            button(label(Icon::REDEEM, "签到"))
                .padding([9, 16])
                .on_press(Message::Checkin)
        };

        let refresh = button(label(Icon::REFRESH, "重读"))
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

/// 一行「图标 名字 …… 值」。
///
/// 名字是 `String` 而不是 `&str`：其中一行是拼出来的（「令牌 · 刷新 29 天」），而一个借来的名字
/// 活不过这一行自己的作用域。
fn fact<'a>(glyph: Icon, label: String, value: String) -> Element<'a, Message> {
    row![
        icon(glyph, FONT_BASE * 0.95, dim_pigment),
        text(label).size(FONT_BASE * 0.9).style(dim),
        Space::new().width(Length::Fill),
        text(value).size(FONT_BASE * 1.0),
    ]
    .width(Length::Fill)
    .align_y(Alignment::Center)
    .spacing(6)
    .into()
}

/// 一个 Material 符号，按给定颜色画。
fn icon<'a>(glyph: Icon, size: f32, pigment: fn(&Theme) -> Color) -> Element<'a, Message> {
    text(glyph.glyph())
        .font(icons::font())
        .size(size)
        .style(move |theme: &Theme| iced::widget::text::Style {
            color: Some(pigment(theme)),
        })
        .into()
}

/// 一条剩余率的带子：左边是还在的，右边是已经用掉的。
///
/// 两段宽度按 `FillPortion` 分，所以它自己会跟着面板的宽度伸缩 —— 这一页没有一处写死了像素宽。
fn bar<'a>(ratio: f32, pigment: fn(&Theme) -> Color) -> Element<'a, Message> {
    let filled = ((ratio.clamp(0.0, 1.0)) * 100.0).round().clamp(1.0, 100.0) as u16;
    let rest = 100u16.saturating_sub(filled).max(1);

    fn end(color: Color) -> container::Style {
        container::Style {
            background: Some(color.into()),
            border: Border {
                radius: 3.0.into(),
                ..Border::default()
            },
            ..container::Style::default()
        }
    }

    row![
        container(Space::new())
            .width(Length::FillPortion(filled))
            .height(6)
            .style(move |theme: &Theme| end(pigment(theme))),
        container(Space::new())
            .width(Length::FillPortion(rest))
            .height(6)
            .style(|theme: &Theme| end(theme.extended_palette().background.strong.color)),
    ]
    .spacing(0)
    .height(6)
    .width(Length::Fill)
    .into()
}

/// 次要文字用的一套样式，省得每一行都写一遍闭包。
fn dim(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(muted(theme)),
    }
}

/// 次要图标的颜色。
fn dim_pigment(theme: &Theme) -> Color {
    muted(theme)
}

/// WorkBuddy 那个标志的绿：磁贴的底色是它，这一页的高亮也是它。
fn brand(_theme: &Theme) -> Color {
    Color::from_rgb8(36, 190, 143)
}

/// 「快用完了」。
fn warn(_theme: &Theme) -> Color {
    Color::from_rgb8(230, 160, 40)
}

/// 一张卡片：一块底色，一道细边。
fn card<'a>(body: Element<'a, Message>, _theme: ThemeMode) -> Element<'a, Message> {
    container(body)
        .width(Length::Fill)
        .padding(8)
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
