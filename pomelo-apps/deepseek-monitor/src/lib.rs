//! DeepSeek — 一枚账号有多少钱，今天花了多少。
//!
//! 这一页比旁边的 WorkBuddy 简单，因为它读的东西少：两个接口，一个说余额，一个说今天的用量。
//! 而这两件事的凭据不是同一枚 —— API Key 只读得到余额，当天的用量要开放平台控制台的
//! `userToken`。所以这一页把「没有 userToken」当成常态画：余额照给，用量给破折号，并说明
//! 去哪儿填。
//!
//! # 单位
//!
//! 余额与消费都是**人民币**，不是积分也不是 token。token 数是另一个数，单独一行。
//!
//! # 一次请求怎么走
//!
//! 页面在 [`DeepSeek::new`] 里发起一次，之后每 [`EVERY`] 秒一次；`update` 在 `Running` 时只看一
//! 帧，等 `Done` 了再把答案收下来。

mod logo;

use std::sync::Arc;
use std::time::Instant;

use iced::widget::{button, column, container, row, text, Space};
use iced::{Alignment, Border, Color, Element, Length, Subscription, Theme};
use pomelo_hal::types::{
    DeepSeekAccount, DeepSeekAnswer, DeepSeekFault, DeepSeekReport, DeepSeekState, UserTokenState,
};
use pomelo_hal::Board;
use pomelo_material_symbols::{self as icons, Icon};
use pomelo_widgets::{Language, SystemPreferences, ThemeMode, FONT_BASE};

/// 页面收到的消息。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Message {
    /// 一帧：轮询状态，到点了就再问一次。
    Tick(Instant),
    /// 再读一次。
    Refresh,
}

/// 上一句要对人说的话。
struct Notice {
    text: String,
    failed: bool,
}

/// 一页。
pub struct DeepSeek {
    board: Arc<Board>,
    /// 上一次读到的账目。没有就是还没读到过。
    report: Option<DeepSeekReport>,
    /// 存了什么凭据 —— Key 的掩码，和有没有 userToken。
    account: DeepSeekAccount,
    /// 后端走到哪了。
    state: DeepSeekState,
    /// 上一句要说的话。
    notice: Option<Notice>,
    /// 上一帧的答案收没收。
    collected: bool,
    /// 语言偏好，页面跟着切。
    preferences: SystemPreferences,
}

impl DeepSeek {
    /// 新的一页，并且马上读一次。
    pub fn new(board: Arc<Board>) -> Self {
        let mut app = Self {
            board,
            report: None,
            account: DeepSeekAccount::default(),
            state: DeepSeekState::Idle,
            notice: None,
            collected: true,
            preferences: SystemPreferences::new(Language::Chinese, ThemeMode::Dark),
        };

        app.account = app.board.deepseek().account();
        app.ask();

        app
    }

    /// 页面跟着外层走。
    pub fn set_preferences(&mut self, preferences: SystemPreferences) {
        self.preferences = preferences;
    }

    /// 上一次读到的账目。
    pub fn report(&self) -> Option<&DeepSeekReport> {
        self.report.as_ref()
    }

    /// 存没存 API Key。
    pub fn configured(&self) -> bool {
        self.account.configured()
    }

    /// 有没有在等网络 —— 页面要据此决定要不要画那一帧。
    pub fn busy(&self) -> bool {
        matches!(self.state, DeepSeekState::Running) || !self.collected
    }

    /// 上一句要说的话。
    pub fn notice_text(&self) -> &str {
        self.notice.as_ref().map_or("", |notice| notice.text.as_str())
    }

    /// 这一页是不是在等一个答案。
    pub fn waiting(&self) -> bool {
        matches!(self.state, DeepSeekState::Running)
    }

    /// 发起一次。
    fn ask(&mut self) {
        self.account = self.board.deepseek().account();
        self.notice = None;

        match self.board.deepseek().start() {
            Ok(()) => {
                self.state = DeepSeekState::Running;
                self.collected = false;
            }
            Err(_) => {
                self.state = DeepSeekState::Idle;
                self.notice = Some(Notice {
                    text: String::from("这次没能发出去"),
                    failed: true,
                });
            }
        }
    }

    /// 收一次答案。
    fn collect(&mut self) {
        self.state = self.board.deepseek().state();

        match self.state {
            DeepSeekState::Done => {
                self.collected = true;

                if let Ok(answer) = self.board.deepseek().take_answer() {
                    match answer {
                        DeepSeekAnswer::Report(report) => {
                            self.report = Some(report);
                            self.notice = None;
                        }
                        DeepSeekAnswer::Refused { fault, message } => {
                            self.notice = Some(Notice {
                                text: sentence(fault, &message),
                                failed: true,
                            });
                        }
                    }
                }

                self.state = DeepSeekState::Idle;
            }
            DeepSeekState::Failed(fault) => {
                self.collected = true;
                self.notice = Some(Notice {
                    text: sentence(fault, ""),
                    failed: true,
                });
                self.state = DeepSeekState::Idle;
            }
            _ => {}
        }
    }

    /// 处理一条消息。
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Tick(_now) => self.collect(),
            Message::Refresh => {
                if !self.busy() {
                    self.ask();
                }
            }
        }
    }

    /// 等答案的那些帧。
    ///
    /// 只在这一页真的在等网络时才订阅帧：一个已经画完的页面不该让这块板子每十六毫秒醒一次。
    pub fn subscription(&self) -> Subscription<Message> {
        if !self.waiting() {
            return Subscription::none();
        }

        iced::window::frames().map(|_| Message::Tick(Instant::now()))
    }

    /// 画出来。
    ///
    /// 一屏四条：标题、余额、今天、一条按小时的柱子。四周留 16 点的边 —— 这块屏的四角是圆的，
    /// 贴边的字会被切掉（真机上就是这样把 `7,443,75…` 切掉过一次）。
    pub fn view(&self) -> Element<'_, Message> {
        let report = self.report.as_ref();

        let body = column![
            self.header(),
            Space::new().height(2),
            text(self.subtitle())
                .size(FONT_BASE * 0.78)
                .style(dim),
            Space::new().height(8),
            self.balance_card(report),
            Space::new().height(6),
            self.stats_card(report),
            Space::new().height(6),
            self.today_card(report),
            Space::new().height(Length::Fill),
            self.notice_view(),
        ]
        .spacing(0)
        .width(Length::Fill);

        container(body)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding([10, 14])
            .into()
    }

    /// 顶上一行：名字、状态、刷新。
    ///
    /// 刷新放在右上角而不是底边：底边两个角是圆的，一个按钮贴在那儿会被切掉一半。
    fn header(&self) -> Element<'_, Message> {
        let usable = self.report.as_ref().is_some_and(|report| report.available);

        row![
            logo::logo(FONT_BASE * 1.35),
            Space::new().width(6),
            text(String::from("DeepSeek"))
                .size(FONT_BASE * 1.25)
                .font(semibold()),
            Space::new().width(Length::Fill),
            status_pill(self.waiting(), usable, self.configured()),
            Space::new().width(8),
            refresh_button(self.busy()),
        ]
        .align_y(Alignment::Center)
        .into()
    }

    /// 标题下面那句：用的是哪枚 Key，什么币种。
    fn subtitle(&self) -> String {
        if !self.account.configured() {
            return String::from("还没有存 API Key · 在网页上填");
        }

        match self.report.as_ref() {
            Some(report) if !report.currency.is_empty() => {
                format!("{} · {}", self.account.masked_key, report.currency)
            }
            _ => self.account.masked_key.clone(),
        }
    }

    /// 余额那一张。
    fn balance_card(&self, report: Option<&DeepSeekReport>) -> Element<'_, Message> {
        let total = report.map_or_else(|| String::from("—"), |r| money(r.total, &r.currency));
        let topped = report.map_or_else(|| String::from("—"), |r| money(r.topped, &r.currency));
        let granted = report.map_or_else(|| String::from("—"), |r| money(r.granted, &r.currency));
        let spent = report
            .and_then(|r| r.total_cost)
            .map_or_else(|| String::from("—"), |cost| format!("{cost:.2}"));

        let head = row![
            text(String::from("余额")).size(FONT_BASE * 0.8).style(dim),
            Space::new().width(Length::Fill),
            text(String::from("累计消费")).size(FONT_BASE * 0.72).style(muted),
            Space::new().width(4),
            text(spent).size(FONT_BASE * 0.78).style(muted),
        ]
        .align_y(Alignment::Center);

        let big = row![
            text(total)
                .size(FONT_BASE * 2.1)
                .font(iced::Font {
                    weight: iced::font::Weight::Bold,
                    ..Default::default()
                }),
            Space::new().width(6),
            text(report.map_or(String::from(""), |r| r.currency.clone()))
                .size(FONT_BASE * 0.8)
                .style(muted),
        ]
        .align_y(Alignment::End);

        let detail = row![
            text(String::from("充值")).size(FONT_BASE * 0.72).style(muted),
            Space::new().width(4),
            text(topped).size(FONT_BASE * 0.85),
            Space::new().width(Length::Fill),
            text(String::from("赠送")).size(FONT_BASE * 0.72).style(muted),
            Space::new().width(4),
            text(granted).size(FONT_BASE * 0.85),
        ]
        .align_y(Alignment::Center);

        card(column![
            head,
            Space::new().height(2),
            big,
            Space::new().height(4),
            detail
        ])
    }

    /// 今天的三项：花了多少、请求了几次、烧了多少 token。
    ///
    /// 单独一张卡而不是塞进下面那张：这三个数是「今天怎么样」的答案，而下面那张是「今天什么时候
    /// 忙」—— 一个看总量，一个看分布，混在一起时两边都读不清。
    fn stats_card(&self, report: Option<&DeepSeekReport>) -> Element<'_, Message> {
        let cost = report
            .and_then(|r| r.today_cost)
            .map_or_else(|| String::from("—"), |cost| format!("{cost:.2}"));
        let requests = report
            .and_then(|r| r.today_requests)
            .map_or_else(|| String::from("—"), |count| count.to_string());
        let tokens = compact(report.and_then(|r| r.today_tokens));

        card(
            row![
                stat_block(String::from("消费"), cost, String::new(), true),
                divider(),
                stat_block(String::from("请求"), requests, String::from("次"), false),
                divider(),
                stat_block(String::from("Tokens"), tokens, String::new(), false),
            ]
            .width(Length::Fill)
            .align_y(Alignment::Center),
        )
    }

    /// 今天那一张：一条按小时的柱状图。
    fn today_card<'a>(&'a self, report: Option<&'a DeepSeekReport>) -> Element<'a, Message> {
        let head = row![
            icon(Icon::INSIGHTS, FONT_BASE * 0.9, brand_pigment),
            Space::new().width(4),
            text(String::from("今天 · 每小时请求")).size(FONT_BASE * 0.8).style(dim),
        ]
        .align_y(Alignment::Center);

        /* 三种「读不到」是三句话：没填、过期了、这次没读上来。 */
        let why = match (self.account.has_user_token, report.map(|r| r.user_token)) {
            (false, _) | (_, Some(UserTokenState::None)) => {
                Some(("没有控制台令牌，读不到用量", "在网页上填 userToken 就有"))
            }
            (_, Some(UserTokenState::Bad)) => {
                Some(("userToken 过期了", "去网页重新拿一枚（步骤在那页写着）"))
            }
            (_, Some(UserTokenState::Fail)) => Some(("用量这次没读上来", "过一会儿按重读")),
            _ => None,
        };

        if let Some((why, then)) = why {
            return card(
                column![
                    head,
                    Space::new().height(4),
                    text(String::from(why)).size(FONT_BASE * 0.8).style(dim),
                    text(String::from(then)).size(FONT_BASE * 0.72).style(muted),
                ]
                .spacing(2),
            );
        }

        let hours = report.map(|r| r.hours.as_slice()).unwrap_or(&[]);

        card(column![
            head,
            Space::new().height(6),
            chart(hours),
            Space::new().height(4),
            axis(),
        ])
    }

    /// 底部那一句，或者是空的。
    fn notice_view(&self) -> Element<'_, Message> {
        match &self.notice {
            Some(notice) => text(notice.text.clone())
                .size(FONT_BASE * 0.75)
                .style(if notice.failed { danger } else { dim })
                .width(Length::Fill)
                .into(),
            None => Space::new().height(0).into(),
        }
    }
}

/// 桌面上的默认值：一块模拟的板子。
///
/// `Board::simulated` 只在桌面构建里存在，所以这个实现也跟着只在桌面上存在 —— 固件永远走
/// [`DeepSeek::new`]，把真的板子交给它。
#[cfg(not(target_os = "espidf"))]
impl Default for DeepSeek {
    fn default() -> Self {
        Self::new(Arc::new(Board::simulated()))
    }
}

/// 一格统计：名字、数、单位（单位可以没有）。
///
/// `loud` 的那一格用品牌的蓝：三项里最该先被读到的是「今天花了多少钱」。
fn stat_block(
    label: String,
    value: String,
    unit: String,
    loud: bool,
) -> Element<'static, Message> {
    let pigment = if loud {
        Color::from_rgb(0.55, 0.63, 1.0)
    } else {
        Color::from_rgb(0.93, 0.94, 0.96)
    };

    let mut value_row = row![
        text(value)
            .size(FONT_BASE * 1.25)
            .font(iced::Font {
                weight: iced::font::Weight::Semibold,
                ..Default::default()
            })
            .style(move |_: &Theme| iced::widget::text::Style {
                color: Some(pigment)
            }),
    ]
    .align_y(Alignment::End);

    if !unit.is_empty() {
        value_row = value_row.push(Space::new().width(3)).push(
            text(unit)
                .size(FONT_BASE * 0.68)
                .style(muted),
        );
    }

    column![
        text(label).size(FONT_BASE * 0.72).style(muted),
        Space::new().height(1),
        value_row,
    ]
    .width(Length::FillPortion(1))
    .into()
}

/// 三格之间那条竖线。
fn divider() -> Element<'static, Message> {
    container(Space::new())
        .width(1)
        .height(26)
        .style(|theme: &Theme| {
            let base = theme.extended_palette().background.base.color;
            let dark = base.r + base.g + base.b < 1.5;

            container::Style {
                background: Some(
                    if dark {
                        Color::from_rgba(1.0, 1.0, 1.0, 0.10)
                    } else {
                        Color::from_rgba(0.0, 0.0, 0.0, 0.08)
                    }
                    .into(),
                ),
                ..container::Style::default()
            }
        })
        .into()
}

/// 今天 24 小时的柱子。
///
/// 没有 canvas：24 个 `container` 就是 24 根柱子，高度按峰值归一。零点的那几根留 3 点高，
/// 让「这一小时没有请求」和「这一小时很小」分得开。
fn chart(hours: &[u32]) -> Element<'_, Message> {
    let peak = hours.iter().copied().max().unwrap_or(0).max(1) as f32;

    let mut bars = row![]
        .spacing(2)
        .height(46)
        .width(Length::Fill)
        .align_y(Alignment::End);

    for value in hours {
        let filled = *value > 0;
        let height = if filled {
            5.0 + (*value as f32 / peak) * 38.0
        } else {
            4.0
        };

        bars = bars.push(
            container(Space::new())
                .width(Length::FillPortion(1))
                .height(height)
                .style(move |theme: &Theme| bar_style(theme, filled)),
        );
    }

    bars.into()
}

/// 柱子下面那条刻度：0 点、6 点、12 点、18 点、24 点。
fn axis() -> Element<'static, Message> {
    let label = |word: &'static str| -> Element<'static, Message> {
        text(String::from(word))
            .size(FONT_BASE * 0.66)
            .style(muted)
            .into()
    };

    row![
        label("0"),
        Space::new().width(Length::Fill),
        label("6"),
        Space::new().width(Length::Fill),
        label("12"),
        Space::new().width(Length::Fill),
        label("18"),
        Space::new().width(Length::Fill),
        label("24"),
    ]
    .width(Length::Fill)
    .into()
}

/// 半粗，标题用。
fn semibold() -> iced::Font {
    iced::Font {
        weight: iced::font::Weight::Semibold,
        ..Default::default()
    }
}

/// 一个小圆点 + 一句话：可用 / 不可用 / 读取中。
fn status_pill(waiting: bool, usable: bool, configured: bool) -> Element<'static, Message> {
    let (color, words) = if waiting {
        (Color::from_rgb(0.95, 0.75, 0.35), String::from("读取中"))
    } else if !configured {
        (Color::from_rgb(0.55, 0.58, 0.64), String::from("未配置"))
    } else if usable {
        (Color::from_rgb(0.24, 0.78, 0.52), String::from("可用"))
    } else {
        (Color::from_rgb(0.95, 0.45, 0.42), String::from("不可用"))
    };

    container(
        row![
            container(Space::new())
                .width(6)
                .height(6)
                .style(move |_: &Theme| container::Style {
                    background: Some(color.into()),
                    border: Border {
                        radius: 3.0.into(),
                        ..Border::default()
                    },
                    ..container::Style::default()
                }),
            Space::new().width(5),
            text(words).size(FONT_BASE * 0.7),
        ]
        .align_y(Alignment::Center),
    )
    .padding([3, 8])
    .style(|theme: &Theme| container::Style {
        background: Some(iced::Background::Color(
            theme.extended_palette().background.weak.color,
        )),
        border: Border {
            radius: 9.0.into(),
            ..Border::default()
        },
        ..container::Style::default()
    })
    .into()
}

/// 右上角那个刷新：一个圆形的图标按钮。
fn refresh_button(busy: bool) -> Element<'static, Message> {
    button(icon(Icon::REFRESH, FONT_BASE * 0.95, muted_pigment))
        .on_press_maybe((!busy).then_some(Message::Refresh))
        .padding(6)
        .style(|theme: &Theme, status: button::Status| {
            let weak = theme.extended_palette().background.weak.color;

            button::Style {
                background: Some(iced::Background::Color(match status {
                    button::Status::Hovered | button::Status::Pressed => {
                        Color { a: weak.a + 0.1, ..weak }
                    }
                    _ => weak,
                })),
                border: Border {
                    radius: 15.0.into(),
                    ..Border::default()
                },
                ..button::Style::default()
            }
        })
        .into()
}

/// 一根柱子：有值用品牌的蓝，没值是一条暗暗的底线。
fn bar_style(theme: &Theme, filled: bool) -> container::Style {
    let base = theme.extended_palette().background.base.color;
    let dark = base.r + base.g + base.b < 1.5;

    let color = if filled {
        Color::from_rgb(0.42, 0.53, 0.98)
    } else if dark {
        Color::from_rgba(1.0, 1.0, 1.0, 0.14)
    } else {
        Color::from_rgba(0.0, 0.0, 0.0, 0.12)
    };

    container::Style {
        background: Some(color.into()),
        border: Border {
            radius: 2.0.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// 一笔钱。不知道就是破折号。
///
/// 不带 ¥ / $：这块板子的字体里没有 ¥（画出来是一个方框），而币种在标题里说过一次了 —— 一行
/// 「余额 · CNY」比每个数字前面挂一个方块清楚。
fn money(value: Option<f32>, _currency: &str) -> String {
    match value {
        Some(value) => format!("{value:.2}"),
        None => String::from("—"),
    }
}

/// 一个大整数缩成三个字符左右：`7.44M`、`123.4K`。
///
/// token 一天就是七八位数，三位一逗号在 480 点宽的屏上放不下 —— 那一行的第三个数被切掉过。
/// 精确值在网页上能看，这一页要的是「量级」。
fn compact(value: Option<u64>) -> String {
    let Some(value) = value else {
        return String::from("—");
    };

    if value < 1_000 {
        return value.to_string();
    }

    if value < 1_000_000 {
        return format!("{:.1}K", value as f64 / 1_000.0);
    }

    if value < 1_000_000_000 {
        return format!("{:.2}M", value as f64 / 1_000_000.0);
    }

    format!("{:.2}G", value as f64 / 1_000_000_000.0)
}

/// 一个大整数，每三位一个逗号。
#[allow(dead_code)]
fn group(value: Option<u64>) -> String {
    let Some(value) = value else {
        return String::from("—");
    };

    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);

    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }

    out
}

/// 把故障说成一句人话。
pub fn sentence(fault: DeepSeekFault, message: &str) -> String {
    let said = match fault {
        DeepSeekFault::NoKey => "还没存 API Key，去网页上填",
        DeepSeekFault::NoNetwork => "板子没联网",
        DeepSeekFault::Unreachable => "连不上 DeepSeek",
        DeepSeekFault::Tls => "HTTPS 握手没过",
        DeepSeekFault::Http => "接口没答上来",
        DeepSeekFault::Auth => "Key 不对，或者 userToken 过期了",
        DeepSeekFault::Api => "账号拒绝了这次读取",
        DeepSeekFault::Storage => "答案放不下，或者写不进卡",
        DeepSeekFault::Failed => "读失败",
    };

    let detail = message.trim();

    if detail.is_empty() {
        String::from(said)
    } else {
        format!("{said}：{detail}")
    }
}

fn icon<'a>(glyph: Icon, size: f32, pigment: fn(&Theme) -> Color) -> Element<'a, Message> {
    text(glyph.glyph())
        .font(icons::font())
        .size(size)
        .style(move |theme: &Theme| iced::widget::text::Style {
            color: Some(pigment(theme)),
        })
        .into()
}

fn card<'a>(body: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(body)
        .width(Length::Fill)
        .padding([10, 12])
        .style(|theme: &Theme| {
            let base = theme.extended_palette().background.base.color;
            let dark = base.r + base.g + base.b < 1.5;

            let fill = if dark {
                Color::from_rgba(1.0, 1.0, 1.0, 0.055)
            } else {
                Color::from_rgba(0.0, 0.0, 0.0, 0.04)
            };

            container::Style {
                background: Some(iced::Background::Color(fill)),
                border: Border {
                    radius: 12.0.into(),
                    width: 1.0,
                    color: Color {
                        a: 0.09,
                        ..if dark { Color::WHITE } else { Color::BLACK }
                    },
                },
                ..container::Style::default()
            }
        })
        .into()
}

fn soft_button(_theme: &Theme, status: button::Status) -> button::Style {
    let alpha = match status {
        button::Status::Hovered => 0.16,
        button::Status::Pressed => 0.24,
        _ => 0.10,
    };

    button::Style {
        background: Some(iced::Background::Color(Color::from_rgba(1.0, 1.0, 1.0, alpha))),
        text_color: Color::from_rgb(0.92, 0.93, 0.95),
        border: Border {
            radius: 8.0.into(),
            ..Border::default()
        },
        ..button::Style::default()
    }
}

fn brand(_theme: &Theme) -> Color {
    Color::from_rgb(0.48, 0.56, 1.0)
}

/// 「可用」那句话的颜色。
fn brand_text(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(brand(theme)),
    }
}

fn brand_pigment(theme: &Theme) -> Color {
    brand(theme)
}

fn muted_pigment(_theme: &Theme) -> Color {
    Color::from_rgb(0.62, 0.66, 0.74)
}

fn danger(_theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(Color::from_rgb(0.95, 0.45, 0.42)),
    }
}

fn dim(_theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(Color::from_rgb(0.70, 0.74, 0.80)),
    }
}

fn muted(_theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(Color::from_rgb(0.55, 0.58, 0.64)),
    }
}
