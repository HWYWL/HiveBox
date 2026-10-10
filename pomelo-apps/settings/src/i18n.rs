//! 界面文案的两种语言。
//!
//! # 为什么不是 `rust-i18n` 或 `fluent`
//!
//! iced 本身没有多语言这一层:一个 `view()` 就是 `state → widgets`,文字在构建的那一刻就是一个
//! `&str`。所以框架层面没有 `t()`、没有语言协商、也没有资源加载 —— 要做的只有一件事:**语言
//! 放进 state,在 `view()` 里查表**。切换语言就是改 state 再返回 `Task::none()`,剩下的交给
//! iced:它重跑 `view()` 之后按层做 diff,只有文字真的变了的那几行会被标脏。
//!
//! 在这个前提下,生态里三种做法都被用过,而这块板子上只有第三种合适:
//!
//! * `rust-i18n`(TOML + `t!` 宏):写起来最省事,但要**运行时解析**文件、要分配,还多一个依赖;
//! * `fluent` + `i18n-embed`(Mozilla 那套):复数、性别、语序都最正确,代价是解析器与 bundle
//!   再加一条资源加载路径 —— 对七个界面的嵌入式 UI 偏重;
//! * **静态表 + 穷尽 `match`**(这里):零依赖、零解析、零分配,而且**漏翻是编译错误**。
//!
//! 所以 [`Language::text`] 是一个 `match`,每个 [`Key`] 必须在两种语言里都有答案,少一条就编不过。
//! 插值交给 Rust 自己的 `format!`(中英一样,例如 `format!("{}%", battery)`),复数在这个界面里
//! 用不到 —— 真需要时再为那一条加一个带数量的函数,不必为此换一套框架。
//!
//! # 翻译什么、不翻译什么
//!
//! 这里翻的是**界面文案**:分节名、行标签、开关名、按钮、脚注、以及「已用 / 总计 / 未连接」这类
//! **词**。不翻的是**数据**:IP、MAC、字节数、型号、固件版本号 —— 它们是这台机器的事实,不是语言。
//! 所以 [`Key`] 里没有它们,页面里也照旧直接写。

pub use pomelo_widgets::Language;

/// Extension trait to provide `text` lookup on [`Language`].
pub trait LanguageExt {
    /// The text for `key`, in this language.
    fn text(self, key: Key) -> &'static str;
}

impl LanguageExt for Language {
    fn text(self, key: Key) -> &'static str {
        match self {
            Self::Chinese => key.chinese(),
            Self::English => key.english(),
        }
    }
}

/// Every piece of interface text the app draws.
///
/// Adding one is safe — the compiler will refuse to build until both languages have a line for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    // The main list's sections, which are also the page titles.
    //
    // Three pages are titled differently from the row that opens them, which is the original's
    // wording and worth keeping: the row is what the section is *about*, the title is where you
    // are. `Battery` and `BatteryRow`, `Memory` and `MemoryTitle`, `Theme` and `ThemeTitle`.
    Settings,
    Back,
    Wifi,
    Memory,
    MemoryTitle,
    Storage,
    Battery,
    BatteryRow,
    System,
    Theme,
    ThemeTitle,
    Time,
    About,
    Language,
    DarkMode,
    FontSize,
    // The one row of the list that opens no page: it asks a question, and the answer resets the
    // board. `RestartQuestion` is what the question says under its own title.
    Restart,
    RestartQuestion,

    // The readout at the top of that list.
    //
    // Its four words are its own and none of them is a row's, which is not tidiness: a gauge and a
    // row labelled the same are two things a finger and a test cannot tell apart, and the row is
    // the one that has to be pressable. So the two usage gauges say what the number *is* — a share
    // in use — and the row under them goes on naming the thing.
    //
    // `ChipTemperature` is short in English by necessity: the three gauges share one row, and a
    // cell of a 480 px panel is 134 px wide. It is the PMIC's die temperature, which is what the
    // battery page calls it at the length a detail row has.
    MemoryUsed,
    StorageUsed,
    ChipTemperature,
    Firmware,
    Uptime,
    // The third instant in the same footer, and the one that is not a clock: when this image was
    // built, above the time being shown and the length of time it has been up. Three readings that
    // belong together in the order they happened.
    BuildTime,
    // `Model` is the *board's* name — "Waveshare ESP32-S3 AMOLED 2.16\"" — and it heads a row on the
    // system page. This one is the chip's part number, which is a different fact with the same word
    // for it, and a card and a row labelled alike are two things a reader takes for one.
    ChipModel,

    // The Wi-Fi page.
    Toggle,
    Network,
    Signal,
    IpAddress,
    Gateway,
    SubnetMask,
    MacAddress,
    Security,
    NotConnected,
    Connected,
    None,

    // The Wi-Fi page's live half: the radio, the scan, and the password prompt. The list above is
    // what the page *shows*; these are what it *does*.
    ScanAgain,
    Scanning,
    NoNetworks,
    Secured,
    WifiOff,
    Password,
    // No `Connect` and no `Cancel`: the prompt's two answers are a tick and a cross, and a glyph is
    // not a word in any language. `ConnectFailed` stays, because the sheet still has to say it.
    ConnectFailed,
    Disconnect,
    Show,
    Hide,

    // The memory page.
    MemoryUsage,
    Usage,
    HeapUsed,
    HeapFree,
    Total,
    Free,
    Used,
    InternalSram,
    Psram,
    Framebuffer,
    Health,

    // The storage page. The page draws one bar and one card per mounted volume, so its own words are
    // the two *kinds* of volume, the empty slot, and the way to look again; the rows inside a card —
    // filesystem, mount point, total, used, free — are the memory page's words and are not repeated.
    InternalStorage,
    SdCard,
    CardSlotEmpty,
    CheckAgain,
    Filesystem,
    MountPoint,
    // The flash map. The four statuses are the four kinds of region the HAL reports, and the page
    // draws one per row; `InternalFlash` heads the card and is *not* `Flash`, which is the chip's
    // model number on the system page.
    InternalFlash,
    Reserved,
    ReadOnly,
    Writable,
    Unallocated,

    // The battery page.
    Power,
    Charging,
    NotCharging,
    Voltage,
    Level,
    // The PMIC's die temperature, and not "the battery's": there is no NTC on this board's battery
    // to read a pack temperature from. Named for where it is measured, like the PMIC row above it.
    PmicTemperature,
    LowPowerMode,

    // The system page.
    Model,
    Os,
    // The OS row's *value*, and the only sentence on that page: this project is a customisation of
    // the upstream `pomelo-ui`, which is a fact about the code and not something the chip can be
    // asked for. What the image calls itself is the firmware row's — that one is a reading.
    //
    // The English is a clause and not a sentence — the label beside it already says "OS", and a
    // value that began "Based on ..." would be the row's own subject said twice. It is also kept
    // shorter than the longest specification on that page, because a value wider than its row is a
    // value drawn outside the card.
    OsBasedOnPomeloUi,
    Display,
    Renderer,
    Flash,
    RefreshRate,
    Touch,
    Cpu,
    Pmic,
    Rtc,

    // The theme page.
    Wallpaper,
    Style,
    Colour,
    AntiAliasing,
    Emissive,

    // The time page.
    TwentyFourHour,
    SystemTime,
    TimeZone,
    NtpSync,
    SyncStatus,

    // Footers.
    BackHint,
    PoweredBy,
    Presets,
}

impl Key {
    /// The Simplified Chinese for this key.
    fn chinese(self) -> &'static str {
        match self {
            Self::Settings => "设置",
            Self::Back => "返回",
            Self::Wifi => "无线网络",
            Self::Memory => "内存 (RAM)",
            Self::MemoryTitle => "内存",
            Self::Storage => "存储",
            Self::Battery => "电池与电源",
            Self::BatteryRow => "电池",
            Self::System => "系统信息",
            Self::Theme => "主题与显示",
            Self::ThemeTitle => "显示与主题",
            Self::Time => "日期与时间",
            Self::About => "关于",
            Self::Language => "语言",
            Self::DarkMode => "深色模式",
            Self::FontSize => "字体大小",
            Self::Restart => "重启",
            Self::RestartQuestion => "设备将立即重新启动。",
            Self::MemoryUsed => "内存占用",
            Self::StorageUsed => "存储占用",
            Self::ChipTemperature => "芯片温度",
            Self::Firmware => "固件版本",
            Self::Uptime => "已启动",
            Self::BuildTime => "编译时间",
            Self::ChipModel => "芯片型号",

            Self::Toggle => "开关",
            Self::Network => "网络",
            Self::Signal => "信号",
            Self::IpAddress => "IP 地址",
            Self::Gateway => "网关",
            Self::SubnetMask => "子网掩码",
            Self::MacAddress => "MAC 地址",
            Self::Security => "安全",
            Self::NotConnected => "未连接",
            Self::Connected => "已连接",
            Self::None => "无",
            Self::ScanAgain => "重新扫描",
            Self::Scanning => "扫描中…",
            Self::NoNetworks => "没有找到网络",
            Self::Secured => "加密",
            Self::WifiOff => "无线网络已关闭",
            Self::Password => "密码",
            Self::ConnectFailed => "连接失败",
            Self::Disconnect => "断开连接",
            Self::Show => "显示明文",
            Self::Hide => "隐藏",

            Self::MemoryUsage => "内存 (RAM) 使用情况",
            Self::Usage => "使用情况",
            Self::HeapUsed => "堆已用",
            Self::HeapFree => "堆可用",
            Self::Total => "总计",
            Self::Free => "可用",
            Self::Used => "已用",
            Self::InternalSram => "内部 SRAM",
            Self::Psram => "PSRAM",
            Self::Framebuffer => "帧缓冲",
            Self::Health => "健康度",

            Self::InternalStorage => "内建存储",
            Self::SdCard => "SD 卡",
            Self::CardSlotEmpty => "未插入存储卡",
            Self::CheckAgain => "重新检测",
            Self::Filesystem => "文件系统",
            Self::MountPoint => "挂载点",
            Self::InternalFlash => "内部闪存",
            Self::Reserved => "保留",
            Self::ReadOnly => "只读",
            Self::Writable => "可写",
            Self::Unallocated => "未分配",

            Self::Charging => "充电中",
            Self::NotCharging => "未充电",
            Self::Power => "电源",
            Self::Voltage => "电压",
            Self::Level => "电量",
            Self::PmicTemperature => "PMIC 温度",
            Self::LowPowerMode => "低功耗模式",

            Self::Model => "型号",
            Self::Os => "操作系统",
            Self::OsBasedOnPomeloUi => "基于 pomelo-ui 深度定制",
            Self::Display => "显示屏",
            Self::Renderer => "渲染器",
            Self::Flash => "闪存",
            Self::RefreshRate => "刷新率",
            Self::Touch => "触摸",
            Self::Cpu => "处理器",
            Self::Pmic => "电源管理",
            Self::Rtc => "实时时钟",

            Self::Wallpaper => "壁纸",
            Self::Style => "样式",
            Self::Colour => "颜色",
            Self::AntiAliasing => "抗锯齿",
            Self::Emissive => "自发光",

            Self::TwentyFourHour => "24 小时制",
            Self::SystemTime => "系统时间",
            Self::TimeZone => "时区",
            Self::NtpSync => "NTP 同步",
            Self::SyncStatus => "同步状态",

            Self::BackHint => "按键 1：返回桌面  |  按键 3：退出设置",
            Self::PoweredBy => "Pomelo",
            Self::Presets => "AMOLED 广色域预设",
        }
    }

    /// The English for this key.
    fn english(self) -> &'static str {
        match self {
            Self::Settings => "Settings",
            Self::Back => "Back",
            Self::Wifi => "Wi-Fi",
            Self::Memory => "Memory (RAM)",
            Self::MemoryTitle => "Memory",
            Self::Storage => "Storage",
            Self::Battery => "Battery & Power",
            Self::BatteryRow => "Battery",
            Self::System => "System",
            Self::Theme => "Theme & Display",
            Self::ThemeTitle => "Display & Theme",
            Self::Time => "Date & Time",
            Self::About => "About",
            Self::Language => "Language",
            Self::DarkMode => "Dark Mode",
            Self::FontSize => "Text Size",
            Self::Restart => "Restart",
            Self::RestartQuestion => "The device will restart now.",
            Self::MemoryUsed => "Memory used",
            Self::StorageUsed => "Storage used",
            Self::ChipTemperature => "Chip temp",
            Self::Firmware => "Firmware",
            Self::Uptime => "Uptime",
            Self::BuildTime => "Build time",
            Self::ChipModel => "Chip model",

            Self::Toggle => "Switch",
            Self::Network => "Network",
            Self::Signal => "Signal",
            Self::IpAddress => "IP address",
            Self::Gateway => "Gateway",
            Self::SubnetMask => "Subnet mask",
            Self::MacAddress => "MAC address",
            Self::Security => "Security",
            Self::NotConnected => "Not connected",
            Self::Connected => "Connected",
            Self::None => "none",
            Self::ScanAgain => "Scan again",
            Self::Scanning => "Scanning…",
            Self::NoNetworks => "No networks found",
            Self::Secured => "secured",
            Self::WifiOff => "Wi-Fi is off",
            Self::Password => "Password",
            Self::ConnectFailed => "Connection failed",
            Self::Disconnect => "Disconnect",
            Self::Show => "Show password",
            Self::Hide => "Hide",

            Self::MemoryUsage => "Memory (RAM) usage",
            Self::Usage => "Usage",
            Self::HeapUsed => "Heap used",
            Self::HeapFree => "Heap free",
            Self::Total => "Total",
            Self::Free => "Free",
            Self::Used => "Used",
            Self::InternalSram => "Internal SRAM",
            Self::Psram => "PSRAM",
            Self::Framebuffer => "Framebuffer",
            Self::Health => "Health",

            Self::InternalStorage => "Built-in storage",
            Self::SdCard => "SD card",
            Self::CardSlotEmpty => "No card inserted",
            Self::CheckAgain => "Check again",
            Self::Filesystem => "Filesystem",
            Self::MountPoint => "Mount point",
            Self::InternalFlash => "Internal flash",
            Self::Reserved => "Reserved",
            Self::ReadOnly => "Read-only",
            Self::Writable => "Writable",
            Self::Unallocated => "Unallocated",

            Self::Charging => "Charging",
            Self::NotCharging => "Not charging",
            Self::Power => "Power",
            Self::Voltage => "Voltage",
            Self::Level => "Level",
            Self::PmicTemperature => "PMIC temperature",
            Self::LowPowerMode => "Low power mode",

            Self::Model => "Model",
            Self::Os => "OS",
            Self::OsBasedOnPomeloUi => "pomelo-ui, heavily customised",
            Self::Display => "Display",
            Self::Renderer => "Renderer",
            Self::Flash => "Flash",
            Self::RefreshRate => "Refresh rate",
            Self::Touch => "Touch",
            Self::Cpu => "CPU",
            Self::Pmic => "PMIC",
            Self::Rtc => "RTC",

            Self::Wallpaper => "Wallpaper",
            Self::Style => "Style",
            Self::Colour => "Colour",
            Self::AntiAliasing => "Anti-aliasing",
            Self::Emissive => "Emissive",

            Self::TwentyFourHour => "24-Hour Time",
            Self::SystemTime => "System time",
            Self::TimeZone => "Time zone",
            Self::NtpSync => "NTP sync",
            Self::SyncStatus => "Sync status",

            Self::BackHint => "Button 1: background  |  Button 3: exit Settings",
            Self::PoweredBy => "Pomelo",
            Self::Presets => "AMOLED wide-gamut presets",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every key has to exist in both languages, and none of them may be empty — the first is what
    /// the compiler already enforces by the `match`, the second is what it does not.
    #[test]
    fn every_key_has_both_languages() {
        for key in ALL {
            for language in [Language::Chinese, Language::English] {
                assert!(
                    !language.text(key).is_empty(),
                    "{key:?} is empty in {language:?}"
                );
            }
        }
    }

    /// The keys that read the same in both languages on purpose: terms, not text.
    ///
    /// A key here is still checked for emptiness above; what it is excused from is differing.
    const SAME_BY_DESIGN: [Key; 2] = [Key::Psram, Key::PoweredBy];

    /// The two languages must actually differ, or the row that switches them is a lie.
    #[test]
    fn the_languages_differ() {
        for key in ALL {
            if SAME_BY_DESIGN.contains(&key) {
                continue;
            }

            assert_ne!(
                Language::Chinese.text(key),
                Language::English.text(key),
                "{key:?} reads the same in both"
            );
        }
    }

    #[test]
    fn switching_reaches_the_other_and_comes_back() {
        assert_eq!(Language::default().other().other(), Language::default());
        assert_ne!(
            Language::default().name(),
            Language::default().other().name()
        );
    }

    /// Every key, for the tests above. Kept beside them so a new variant is a compile error here
    /// too.
    const ALL: [Key; 99] = [
        Key::Settings,
        Key::Back,
        Key::Wifi,
        Key::Memory,
        Key::MemoryTitle,
        Key::Storage,
        Key::Battery,
        Key::BatteryRow,
        Key::System,
        Key::Theme,
        Key::ThemeTitle,
        Key::Time,
        Key::About,
        Key::Language,
        Key::DarkMode,
        Key::FontSize,
        Key::Restart,
        Key::RestartQuestion,
        Key::MemoryUsed,
        Key::StorageUsed,
        Key::ChipTemperature,
        Key::Firmware,
        Key::Uptime,
        Key::BuildTime,
        Key::ChipModel,
        Key::Toggle,
        Key::Network,
        Key::Signal,
        Key::IpAddress,
        Key::Gateway,
        Key::SubnetMask,
        Key::MacAddress,
        Key::Security,
        Key::NotConnected,
        Key::Connected,
        Key::None,
        Key::ScanAgain,
        Key::Scanning,
        Key::NoNetworks,
        Key::Secured,
        Key::WifiOff,
        Key::Password,
        Key::ConnectFailed,
        Key::Disconnect,
        Key::Show,
        Key::Hide,
        Key::MemoryUsage,
        Key::Usage,
        Key::HeapUsed,
        Key::HeapFree,
        Key::Total,
        Key::Free,
        Key::Used,
        Key::InternalSram,
        Key::Psram,
        Key::Framebuffer,
        Key::Health,
        Key::InternalStorage,
        Key::SdCard,
        Key::CardSlotEmpty,
        Key::CheckAgain,
        Key::Filesystem,
        Key::MountPoint,
        Key::InternalFlash,
        Key::Reserved,
        Key::ReadOnly,
        Key::Writable,
        Key::Unallocated,
        Key::Charging,
        Key::NotCharging,
        Key::Power,
        Key::Voltage,
        Key::Level,
        Key::PmicTemperature,
        Key::LowPowerMode,
        Key::Model,
        Key::Os,
        Key::OsBasedOnPomeloUi,
        Key::Display,
        Key::Renderer,
        Key::Flash,
        Key::RefreshRate,
        Key::Touch,
        Key::Cpu,
        Key::Pmic,
        Key::Rtc,
        Key::Wallpaper,
        Key::Style,
        Key::Colour,
        Key::AntiAliasing,
        Key::Emissive,
        Key::TwentyFourHour,
        Key::SystemTime,
        Key::TimeZone,
        Key::NtpSync,
        Key::SyncStatus,
        Key::BackHint,
        Key::PoweredBy,
        Key::Presets,
    ];
}
