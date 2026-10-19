//! 云服务 — the servers in a cloud account, and their power buttons.
//!
//! One company at a time, one list of its machines whichever product each of them belongs to, and
//! three buttons for the instance a finger has picked on. What it is *for* is the machine nobody is
//! sitting in front of: a test server that has been left running for a week, a build box that needs a
//! restart, the instance whose bill somebody wants stopped before a holiday.
//!
//! # A vendor, and then one list
//!
//! A key belongs to a company — 腾讯云, 阿里云, AWS — and a company sells servers through one API or
//! two: Tencent's elastic cloud server and its lightweight one are two APIs with two shapes of
//! answer, while Alibaba's ECS and AWS's EC2 are one each. That is a fact about the *vendor*, and it
//! used to be a fact about this page too: two chips, two lists, and a person who had to remember
//! which side their machine was on. It is not a useful fact to ask somebody to hold. So the page asks
//! one vendor at a time — see [`CloudMonitor::vendor`] — and the board asks every product that
//! vendor sells and merges the answers, giving one list whose rows each say which product they came
//! from. A power command goes back out through the row's own product: see
//! [`CloudInstance::provider`].
//!
//! AWS is the one vendor the page cannot ask: it is in the list, with a place to keep a key, and the
//! board answers [`CloudFault::Unsupported`] until its signature is written. Saying so is better
//! than an empty list that looks like an account with no machines.
//!
//! # Why no key is typed in here
//!
//! Because the key is three fields — a secret id, a secret, a region — and that is a form, on a page
//! with a keyboard layout and room for two lines of explanation under each box. This page has a
//! 480 px panel and a list to draw. So what it says about credentials is whether one is saved, and
//! where to go and save one — see [`Words::no_key`]. Writing a key is the management page's, and this
//! app never sees a secret at all: [`CloudKey`] is masked by the backend before it gets here.
//!
//! # The shape of a request, and why the page is drawn either way
//!
//! A list and a power command are both *ask, then poll* — see `CloudBackend` — because a request is a
//! TLS handshake and a signed round trip to a data centre, which is a second or two of somebody
//! else's time. So nothing here waits: [`CloudMonitor::update`] starts a request and returns, the
//! page is drawn from whatever the last answer left behind, and [`CloudMonitor::pull`] reads the
//! answer on the frame it arrives.
//!
//! # What a power command actually does
//!
//! Not much, and that is the honest answer: the provider answers with a receipt, and the machine is
//! *going* to change. So the page draws the receipt, then asks for the list again a few ticks later —
//! see [`RELIST_AFTER`] — which is the only way to find out that a command was accepted and then did
//! not work. Between the two, the row is drawn as amber, because that is what the provider says the
//! machine is doing.
//!
//! # The one button that asks twice
//!
//! Stopping a machine takes it away from whatever was logged into it, and on a touch panel a stop
//! button is one finger-width from a restart button. So [`Message::Power`] arms a stop and sends it on
//! the second press, with the arming expiring after [`CONFIRM_WINDOW`] — a confirmation that stayed
//! armed for ever would be a confirmation that stopped being one. See [`CloudMonitor::armed`].

mod style;
#[cfg(test)]
mod tests;
mod ui;

use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::theme::Palette;
use iced::{Element, Subscription, Theme};

use pomelo_hal::error::HalError;
use pomelo_hal::{
    Board, CloudAction, CloudAnswer, CloudFault, CloudInstance, CloudInstanceState, CloudKey,
    CloudProvider, CloudState, CloudTarget, CloudVendor,
};
use pomelo_widgets::preferences::{Language, SystemPreferences, ThemeMode};

pub use style::Sizes;

/// How many ticks after a power command the list is asked for again.
///
/// Three, and the same three the simulator's round trip takes: roughly as long as a provider needs
/// before it starts describing the machine as `STARTING`, so the list that comes back is a list that
/// has something new in it rather than the list that was there before.
const RELIST_AFTER: u32 = 3;

/// How long a stop stays armed before it puts itself down.
///
/// Five seconds: long enough to press the same button twice deliberately, short enough that a button
/// touched while putting the panel down is a button that has forgotten about it by the time anybody
/// looks again.
const CONFIRM_WINDOW: Duration = Duration::from_secs(5);

/// 云服务 — the servers in a cloud account, and their power buttons.
pub struct CloudMonitor {
    /// The board, and through it the account. Cloned rather than held by reference so the page can be
    /// built in a closure iced owns — see `main.rs`.
    board: Arc<Board>,
    /// The company whose list is on the page, and whose key the buttons would use.
    ///
    /// One at a time, because a key belongs to a company: the page opens on whichever vendor has a
    /// key — see [`CloudMonitor::new`] — and a finger switches it with [`Message::Vendor`].
    vendor: CloudVendor,
    /// The key of each vendor, indexed by [`CloudVendor::index`], as much of it as comes back out.
    ///
    /// One per company and not one per product: Tencent's two kinds of server are sold to one
    /// account, signed with one key. The table is read when the page opens and again every time an
    /// answer arrives, which are the only two moments it can change underneath this page: writing one
    /// is the *management* page's job, and a panel whose "no key saved" card survived somebody adding
    /// a key on the web page would be a panel telling a person to do something they have already done.
    keys: [CloudKey; 3],
    /// Every machine of the selected vendor, as the last list left them: every product's in one list.
    instances: Vec<CloudInstance>,
    /// The products whose list did not come back last time, so the page can say which half is missing
    /// rather than presenting half an account as the whole of it.
    failed: Vec<CloudProvider>,
    /// The machine the request in flight named, and no more than that.
    ///
    /// The id and not the whole question, because a receipt names an action and an id, and what a person
    /// needs to read is *which machine* they just stopped — which is the id's name in the list as it was
    /// when they asked. What was asked for is already in the receipt.
    asked: Option<String>,
    /// How the request is going, as the backend last reported it.
    state: CloudState,
    /// Whether the answer of the request that finished has been read. True when nothing is owed.
    collected: bool,
    /// What the last request that finished had to say, in the few words the page has room for.
    notice: Option<Notice>,
    /// Ticks left before the list is asked for again after a command.
    relist_in: u32,
    /// The instance a finger picked, by id rather than by row: an id is the one thing that survives the
    /// list being replaced under it, which is exactly what a power command does.
    selected: Option<String>,
    /// A power command waiting for a second press, and when it started waiting.
    armed: Option<(CloudAction, Instant)>,
    /// The theme, the language and the base font size: everything this page asks the box how to be.
    preferences: SystemPreferences,
}

/// What the last request that finished had to say.
///
/// Crate-internal rather than public: it is the page's own vocabulary for its footer, and the launcher
/// has no business reading it — what the launcher needs to know about this app is whether it is waiting,
/// which is [`CloudMonitor::waiting`].
#[derive(Debug, Clone)]
pub(crate) enum Notice {
    /// The provider took the command.
    Accepted {
        /// What was asked of the machine.
        action: CloudAction,
        /// The machine's name, if the list still knows it.
        name: String,
    },
    /// The request did not come back, or the provider refused it. The three fields are
    /// [`CloudAnswer::Refused`]'s, unchanged.
    Refused {
        fault: CloudFault,
        code: String,
        message: String,
    },
}

/// What the page is asked to do.
///
/// `PartialEq` because the launcher's own message enum derives it — a launcher message is compared to
/// another to tell a press from a repeat, and every app's message has to be comparable for that to work.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// A frame, and the only thing that moves a request along.
    Tick(Instant),
    /// Look at another company's machines.
    Vendor(CloudVendor),
    /// Pick an instance: a row of the list, or the row again to put it back down.
    Select(String),
    /// Ask for the list again.
    Refresh,
    /// Ask for a power command on the selected instance. For a stop, the second press is the one that
    /// sends it.
    Power(CloudAction),
}

impl CloudMonitor {
    /// The page, already asking for a list if there is a key to ask with.
    ///
    /// A page that opened on an empty list and waited for a finger would look like an account with no
    /// servers on it, which is a different thing from a page that has not looked yet.
    ///
    /// It opens on a vendor somebody has a key for and not on the first one in the list: a person who
    /// has typed one Alibaba key in is looking for Alibaba's machines, and a page that opened on
    /// Tencent's "no key" card would be a page telling them to go and do what they have just done.
    pub fn new(board: Arc<Board>) -> Self {
        let mut page = Self {
            board,
            vendor: CloudVendor::default(),
            keys: std::array::from_fn(|_| CloudKey::default()),
            instances: Vec::new(),
            failed: Vec::new(),
            asked: None,
            state: CloudState::Idle,
            collected: true,
            notice: None,
            relist_in: 0,
            selected: None,
            armed: None,
            preferences: SystemPreferences::default(),
        };

        page.keys = page.read_keys();
        page.vendor = CloudVendor::ALL
            .into_iter()
            .find(|vendor| page.key(*vendor).configured())
            .unwrap_or_default();

        if page.can_ask() {
            page.refresh();
        }

        page
    }

    /// The box's preferences, as they were when the page was last told about them.
    pub fn preferences(&self) -> SystemPreferences {
        self.preferences
    }

    /// Takes the box's preferences: the theme, the language and the base font size.
    ///
    /// Every measurement on the page is derived from these rather than from constants, which is why
    /// this is a setter the launcher calls on every app it holds and not a detail of drawing.
    pub fn set_preferences(&mut self, preferences: SystemPreferences) {
        self.preferences = preferences;
    }

    /// The measurements this page is drawn with.
    pub fn sizes(&self) -> Sizes {
        Sizes::new()
    }

    /// The company whose list is on the page.
    pub fn vendor(&self) -> CloudVendor {
        self.vendor
    }

    /// The key saved for `vendor`, as much of it as comes back out.
    ///
    /// One key per company, whichever of its products a machine belongs to.
    pub fn key(&self, vendor: CloudVendor) -> CloudKey {
        self.keys[vendor.index()].clone()
    }

    /// Whether the vendor on the page has a key saved.
    ///
    /// The *selected* vendor and not any of them: what this answers is whether the list under it can
    /// be asked for, and a key saved somewhere else is not a list this page can draw.
    pub fn configured(&self) -> bool {
        self.key(self.vendor).configured()
    }

    /// Whether there is anything this page could ask for: a key, and a client to sign with.
    ///
    /// Two questions and not one because they are answered by different facts — the credentials file,
    /// and this build — and a vendor that has the first and not the second would spin a request that
    /// can only come back [`CloudFault::Unsupported`]. See [`CloudVendor::is_wired_up`].
    pub(crate) fn can_ask(&self) -> bool {
        self.configured() && self.vendor.is_wired_up()
    }

    /// Every machine of the vendor on the page, as the last list left them.
    ///
    /// Every product in one list, each row saying which product it came from. Empty before the first
    /// answer, and empty after a list that came back with nothing in it — two different screens: see
    /// [`CloudMonitor::state`].
    pub fn instances(&self) -> &[CloudInstance] {
        &self.instances
    }

    /// The products whose list did not come back last time.
    ///
    /// Empty on a good day. When it is not, the page draws what did arrive *and* says which half is
    /// missing: half a list presented as a whole one is how somebody concludes a machine was deleted.
    pub fn failed(&self) -> &[CloudProvider] {
        &self.failed
    }

    /// How the request is going.
    pub fn state(&self) -> CloudState {
        self.state
    }

    /// Whether a request is in flight, and so whether the page is showing a spinner.
    ///
    /// A request that has finished but whose answer has not been read counts as in flight, because
    /// there is nothing else it can honestly be: the spinner is still what the page should draw.
    pub fn busy(&self) -> bool {
        matches!(self.state, CloudState::Running) || !self.collected
    }

    /// The instance a finger picked, if the list still holds it.
    ///
    /// Resolved against the list rather than held as a copy, so that a machine that has just been
    /// powered on is drawn from the *new* list rather than from the answer that arrived a second ago.
    pub fn selected(&self) -> Option<&CloudInstance> {
        let id = self.selected.as_deref()?;
        self.instances.iter().find(|instance| instance.id == id)
    }

    /// Whether `action` is armed, and so whether its button is asking rather than doing.
    pub fn armed(&self, action: CloudAction) -> bool {
        matches!(self.armed, Some((armed, _)) if armed == action)
    }

    /// What the last request that finished had to say, for the footer of the page.
    pub(crate) fn notice(&self) -> Option<&Notice> {
        self.notice.as_ref()
    }

    /// Whether the app has anything left to do that only frames can move along.
    ///
    /// The one question the launcher and `main.rs` both ask, and the reason it is a method rather than
    /// a constant `true`: a page that asked for frames for ever would keep a laptop's fan on and the
    /// board's radio awake for a list that is not moving. See [`CloudMonitor::subscription`].
    pub fn waiting(&self) -> bool {
        self.busy() || self.relist_in > 0 || self.armed.is_some()
    }

    /// Handles a message.
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Tick(now) => self.tick(now),
            Message::Vendor(vendor) => self.pick_vendor(vendor),
            Message::Select(id) => self.select(id),
            Message::Refresh => self.refresh(),
            Message::Power(action) => self.power(action),
        }
    }

    /// One frame: the clock a request is polled on, a list is asked for again on, and an armed stop
    /// puts itself down on.
    fn tick(&mut self, now: Instant) {
        if let Some((_, armed_at)) = self.armed {
            if now.duration_since(armed_at) >= CONFIRM_WINDOW {
                self.armed = None;
            }
        }

        // Before the countdown, so that a command answered on this frame has finished being busy by
        // the time the countdown decides whether it may ask for anything.
        self.pull();

        if self.relist_in == 0 {
            return;
        }

        // Held while something is in flight rather than skipped: a command that is still being
        // answered is a command whose effect is not in the list yet, and a list asked for now would
        // be the list that was there before it.
        if self.busy() {
            return;
        }

        self.relist_in -= 1;
        if self.relist_in == 0 {
            self.refresh();
        }
    }

    /// Reads what has arrived, and nothing else.
    ///
    /// Taking an answer is a lock and a parse, so it happens on the frame the backend stops saying
    /// `Running` and not on any frame before or after — and the keys are read again here, which is
    /// the second of the two moments they can change underneath this page.
    fn pull(&mut self) {
        let state = self.board.cloud().state();
        self.state = state;

        if matches!(state, CloudState::Running) || self.collected {
            return;
        }

        let asked = self.asked.take();
        self.collected = true;
        self.keys = self.read_keys();

        // Read before it is filed: taking the answer needs the backend locked, and filing it needs the
        // page mutably — two borrows that cannot be held at once, and the answer is a value rather than
        // a borrow of the backend either way.
        let answer = self.board.cloud().take_answer().ok();

        if let Some(answer) = answer {
            // A request that came to something is over, and the backend has gone back to idle by the
            // time this frame ends — so a page that kept saying `Done` until the next frame would be
            // reporting a request that finished a frame ago as though it were still arriving. A request
            // that *failed* keeps saying so: that one is not a wait, and [`CloudMonitor::state`] is
            // what a page says when it is drawing what happened.
            if matches!(self.state, CloudState::Done) {
                self.state = CloudState::Idle;
            }

            self.receive(answer, asked);
        }
    }

    /// Gives up the request in flight, if there is one, and puts the page back where a request starts.
    ///
    /// A no-op when there is nothing to give up — the backend answers `Ok` for that too — and the page
    /// is left saying `Idle` either way, which is what the next request needs.
    fn abandon(&mut self) {
        if self.busy() {
            let _ = self.board.cloud().cancel();
        }

        self.state = CloudState::Idle;
        self.collected = true;
        self.asked = None;
    }

    /// Files what an answer had to say.
    fn receive(&mut self, answer: CloudAnswer, asked: Option<String>) {
        match answer {
            CloudAnswer::Instances {
                instances, failed, ..
            } => {
                // One list for the whole account, so there is no "wrong product's answer" to guard
                // against any more: every list the page asks for is the merged one, and a row carries
                // the product its buttons go out through.
                self.instances = instances;
                self.failed = failed;

                // The picked row is picked by id, so a machine that has gone away leaves nothing
                // standing behind it.
                if self.selected().is_none() {
                    self.selected = None;
                }

                // A list that arrived is the end of whatever the last request had to say about not
                // arriving, and it stays on the page until that list is replaced.
                self.notice = None;
            }
            CloudAnswer::Accepted { action, .. } => {
                let name = asked
                    .map(|id| {
                        self.instances
                            .iter()
                            .find(|instance| instance.id == id)
                            .map_or(id, |instance| instance.name.clone())
                    })
                    .unwrap_or_default();

                self.notice = Some(Notice::Accepted { action, name });

                // The provider is already describing the machine as changing; the list on the page is
                // the one from before the command. Asking again is what turns a receipt into a result.
                self.relist_in = RELIST_AFTER;
            }
            CloudAnswer::Refused {
                fault,
                code,
                message,
            } => {
                // A board with no key is not a board with a list, and the card that replaces this one
                // is the screen that says what to do about it. Any other refusal leaves the list up:
                // a request that failed to refresh is not a reason to forget what was there.
                if fault == CloudFault::NoCredentials {
                    self.instances.clear();
                    self.failed.clear();
                    self.selected = None;
                }

                self.notice = Some(Notice::Refused {
                    fault,
                    code,
                    message,
                });
            }
        }
    }

    /// Switches to `vendor`, dropping what the last one left on the page.
    ///
    /// The list, the picked row and the footer all belonged to the vendor being left, and an answer
    /// still on its way is an answer about a list nobody is looking at any more — so the request is
    /// abandoned rather than waited for: the frame it arrives on would otherwise be a frame that drew
    /// one company's machines under another company's name.
    fn pick_vendor(&mut self, vendor: CloudVendor) {
        if vendor == self.vendor {
            return;
        }

        self.abandon();
        self.vendor = vendor;
        self.instances.clear();
        self.failed.clear();
        self.selected = None;
        self.notice = None;
        self.armed = None;
        self.relist_in = 0;

        if self.can_ask() {
            self.refresh();
        }
    }

    /// Picks an instance, or puts the picked one back down.
    fn select(&mut self, id: String) {
        // Picking a row is somebody deciding what to do with it, and a button still armed from the row
        // before is a button that would act on the wrong machine.
        self.armed = None;
        self.selected = if self.selected.as_deref() == Some(id.as_str()) {
            None
        } else {
            Some(id)
        };
    }

    /// Asks for every machine the selected vendor has.
    ///
    /// One request, not one per product: the board asks each product that vendor sells and merges
    /// what comes back, so the page has one answer to wait for and one list to draw.
    fn refresh(&mut self) {
        self.ask(
            CloudTarget::Vendor(self.vendor),
            CloudAction::List,
            String::new(),
            String::new(),
        );
    }

    /// Asks for a power command on the picked instance, the second time it is asked for.
    fn power(&mut self, action: CloudAction) {
        let Some(instance) = self.selected().cloned() else {
            return;
        };

        // A stop asks twice — see `Message::Power` — and the arming is what the button draws itself
        // from. Any other command goes the first time: a start on a machine that is already down, and
        // a restart on one that is up, are what the person pressed the button for.
        if action == CloudAction::Stop && !self.armed(action) {
            self.armed = Some((action, Instant::now()));
            return;
        }

        // Out through the product and data centre the row came from, which is the whole reason a row
        // carries them: StartInstances lives at two endpoints, and its region is the machine's, not
        // the key's.
        let region = instance.region;
        self.ask(
            CloudTarget::Machine(instance.provider),
            action,
            instance.id,
            region,
        );
    }

    /// Starts a request, if there is one to start.
    fn ask(
        &mut self,
        target: CloudTarget,
        action: CloudAction,
        instance: String,
        region: String,
    ) {
        // One request at a time, and the backend would refuse a second anyway. Dropped rather than
        // queued: a person who pressed a button while the page was spinning asked for what the page is
        // already doing.
        if self.busy() {
            return;
        }

        // Nothing to ask with. Not a failure, and not a request that would tell this page anything it
        // does not already know: the card that says where the key goes — or that this build has no
        // client for the vendor — is already up.
        if !self.can_ask() {
            self.state = CloudState::Idle;
            self.collected = true;
            return;
        }

        self.armed = None;

        match self.board.cloud().start(target, action, &instance, &region) {
            Ok(()) => {
                self.state = CloudState::Running;
                self.collected = false;
                self.notice = None;
                self.asked = Some(instance);
            }
            Err(error) => {
                // The HAL would not take the request at all, which is this page asking for something it
                // should not have. There is no answer coming, so the page stops spinning rather than
                // spinning for ever, and says so in the words the fault list already has.
                self.state = CloudState::Idle;
                self.collected = true;
                self.asked = None;
                self.notice = Some(Notice::Refused {
                    fault: if error == HalError::InvalidArg {
                        CloudFault::Args
                    } else {
                        CloudFault::Failed
                    },
                    code: String::new(),
                    message: String::new(),
                });
            }
        }
    }

    /// The keys as the backend has them, which is as much of them as ever comes out.
    fn read_keys(&self) -> [CloudKey; 3] {
        let cloud = self.board.cloud();
        // The vendors, one key each: the products share their company's.
        CloudVendor::ALL.map(|vendor| cloud.key(vendor))
    }

    /// Frames, and only while this page is waiting for one. See [`CloudMonitor::waiting`].
    ///
    /// On the board this is never asked for: the launcher merges the subscriptions of the apps it is
    /// actually running, and its own pump is what sends [`Message::Tick`] here. A window has no such
    /// pump, so the app takes the frames it is offered — while there is a reason to.
    pub fn subscription(&self) -> Subscription<Message> {
        if self.waiting() {
            iced::window::frames().map(Message::Tick)
        } else {
            Subscription::none()
        }
    }

    /// The theme, by the system's preference.
    pub fn theme(&self) -> Theme {
        match self.preferences.theme {
            ThemeMode::Dark => Theme::custom(
                "PomeloDark",
                Palette {
                    background: style::surface(ThemeMode::Dark),
                    ..Palette::DARK
                },
            ),
            ThemeMode::Light => Theme::custom(
                "PomeloLight",
                Palette {
                    background: style::surface(ThemeMode::Light),
                    ..Palette::LIGHT
                },
            ),
        }
    }

    /// The page.
    pub fn view(&self) -> Element<'_, Message> {
        ui::page(self)
    }
}

impl Drop for CloudMonitor {
    /// Leaving the page is leaving the request. An answer for a page nobody is looking at is a slot held
    /// on somebody else's network for nothing, and the launcher drops an app when its tile is closed —
    /// so this is where a page that was walked away from mid-request hands its slot back, rather than a
    /// hook the launcher would have to remember to call. See [`pomelo_hal::CloudBackend::cancel`].
    fn drop(&mut self) {
        self.abandon();
    }
}

/// The words on the page, in the language the system is set to.
///
/// One struct with both languages in it — the shape the rest of this project's pages use — so that a
/// page never has a sentence with a language of its own, and adding a word is adding a field the
/// compiler will insist be filled in twice.
pub(crate) struct Words {
    pub(crate) language: Language,
    pub(crate) title: &'static str,
    pub(crate) tencent: &'static str,
    pub(crate) aliyun: &'static str,
    pub(crate) aws: &'static str,
    pub(crate) cvm: &'static str,
    pub(crate) lighthouse: &'static str,
    pub(crate) ecs: &'static str,
    pub(crate) ec2: &'static str,
    pub(crate) refresh: &'static str,
    pub(crate) no_key: &'static str,
    pub(crate) no_key_note: &'static str,
    pub(crate) unsupported: &'static str,
    pub(crate) unsupported_note: &'static str,
    pub(crate) instances: &'static str,
    pub(crate) loading: &'static str,
    pub(crate) empty: &'static str,
    pub(crate) select_hint: &'static str,
    pub(crate) start: &'static str,
    pub(crate) stop: &'static str,
    pub(crate) reboot: &'static str,
    pub(crate) confirm_stop: &'static str,
}

impl Words {
    /// The page's words, for `language`.
    pub(crate) fn of(language: Language) -> Self {
        match language {
            Language::Chinese => Self {
                language,
                title: "云服务",
                tencent: "腾讯云",
                aliyun: "阿里云",
                aws: "AWS",
                cvm: "云服务器",
                lighthouse: "轻量应用服务器",
                ecs: "ECS",
                ec2: "EC2",
                refresh: "刷新",
                no_key: "这家云还没有保存密钥",
                no_key_note: "在网页端「云服务」里填写",
                unsupported: "这家云暂不支持",
                unsupported_note: "这个版本还没有它的客户端",
                instances: "实例",
                loading: "正在查询…",
                empty: "这个账号没有实例",
                select_hint: "选一台机器来操作",
                start: "开机",
                stop: "关机",
                reboot: "重启",
                confirm_stop: "确认关机",
            },
            Language::English => Self {
                language,
                title: "Cloud",
                tencent: "Tencent",
                aliyun: "Alibaba",
                aws: "AWS",
                cvm: "Cloud servers",
                lighthouse: "Lightweight",
                ecs: "ECS",
                ec2: "EC2",
                refresh: "Refresh",
                no_key: "No API key yet for this vendor",
                no_key_note: "Fill one in under Cloud on the web page",
                unsupported: "This vendor is not supported yet",
                unsupported_note: "This build has no client for it",
                instances: "Instances",
                loading: "Querying…",
                empty: "No instances on this account",
                select_hint: "Pick a machine to act on",
                start: "Start",
                stop: "Stop",
                reboot: "Restart",
                confirm_stop: "Confirm stop",
            },
        }
    }

    /// The name of a company, for the chip that picks it and the list card's heading.
    pub(crate) fn vendor(&self, vendor: CloudVendor) -> &'static str {
        match vendor {
            CloudVendor::Tencent => self.tencent,
            CloudVendor::Aliyun => self.aliyun,
            CloudVendor::Aws => self.aws,
        }
    }

    /// The name of a product, for a row's tag and the line that names a missing half.
    ///
    /// A product and not a company: a vendor's list is a merge of its products, and "where did this
    /// row come from" is the question that survived the merge.
    pub(crate) fn provider(&self, provider: CloudProvider) -> &'static str {
        match provider {
            CloudProvider::Cvm => self.cvm,
            CloudProvider::Lighthouse => self.lighthouse,
            CloudProvider::Ecs => self.ecs,
            CloudProvider::Ec2 => self.ec2,
        }
    }

    /// A list that is missing one product's machines, in one line.
    ///
    /// Said under the rows that did arrive rather than instead of them: the list is still true, it is
    /// just not the whole of the account, and the difference is a sentence.
    pub(crate) fn partial(&self, failed: &[CloudProvider]) -> String {
        let names: Vec<&str> = failed
            .iter()
            .map(|provider| self.provider(*provider))
            .collect();
        let names = names.join(" / ");

        match self.language {
            Language::Chinese => format!("列表不完整：{names} 没有返回"),
            Language::English => format!("Partial list: {names} did not come back"),
        }
    }

    /// What a machine is doing, as one word.
    ///
    /// A word and not the provider's own code: `RUNNING` is what an API returns and "运行中" is what a
    /// person reads. The last state is drawn as a dash because there is no word for it that would not be
    /// a guess — see [`CloudInstanceState::Other`].
    pub(crate) fn state(&self, state: CloudInstanceState) -> &'static str {
        match (self.language, state) {
            (Language::Chinese, CloudInstanceState::Running) => "运行中",
            (Language::Chinese, CloudInstanceState::Stopped) => "已关机",
            (Language::Chinese, CloudInstanceState::Starting) => "开机中",
            (Language::Chinese, CloudInstanceState::Stopping) => "关机中",
            (Language::Chinese, CloudInstanceState::Rebooting) => "重启中",
            (Language::English, CloudInstanceState::Running) => "Running",
            (Language::English, CloudInstanceState::Stopped) => "Stopped",
            (Language::English, CloudInstanceState::Starting) => "Starting",
            (Language::English, CloudInstanceState::Stopping) => "Stopping",
            (Language::English, CloudInstanceState::Rebooting) => "Restarting",
            (_, CloudInstanceState::Other) => "—",
        }
    }

    /// The name of a command, for the button that sends it and the line that reports it.
    ///
    /// [`CloudAction::List`] answers with [`Words::refresh`], which is what the same request is called
    /// on the button that asks for it by hand.
    pub(crate) fn action(&self, action: CloudAction) -> &'static str {
        match action {
            CloudAction::List => self.refresh,
            CloudAction::Start => self.start,
            CloudAction::Stop => self.stop,
            CloudAction::Reboot => self.reboot,
        }
    }

    /// How many machines the list is showing.
    ///
    /// A sentence rather than a number, because the two languages put the number in different places:
    /// Chinese counts with a measure word after it ("三台"), English puts the plural on the noun.
    pub(crate) fn count(&self, count: usize) -> String {
        match self.language {
            Language::Chinese => format!("共 {count} 台"),
            Language::English => match count {
                1 => String::from("1 instance"),
                other => format!("{other} instances"),
            },
        }
    }

    /// A command that was taken, in one line.
    pub(crate) fn accepted(&self, name: &str, action: &str) -> String {
        match self.language {
            Language::Chinese => format!("已受理：{name} {action}"),
            Language::English => format!("Accepted: {action} {name}"),
        }
    }

    /// A request that did not produce a list, in one line.
    ///
    /// The provider's own sentence when there is one, and this page's own words for the fault when
    /// there is not: what a person needs from a refusal is the provider's reason, and what they need
    /// from a fault is that the box knows what went wrong.
    pub(crate) fn refused(&self, fault: CloudFault, code: &str, message: &str) -> String {
        match (message.is_empty(), code.is_empty()) {
            (false, false) => format!("{message} ({code})"),
            (false, true) => message.to_string(),
            _ => self.fault(fault).to_string(),
        }
    }

    /// This page's own words for a fault: one line each, and each one a thing to do.
    pub(crate) fn fault(&self, fault: CloudFault) -> &'static str {
        match (self.language, fault) {
            (Language::Chinese, CloudFault::Failed) => "请求失败",
            (Language::Chinese, CloudFault::Args) => "请求有误",
            (Language::Chinese, CloudFault::NoCredentials) => "还没有密钥",
            (Language::Chinese, CloudFault::NoNetwork) => "没有网络",
            (Language::Chinese, CloudFault::ClockNotSet) => "还没有同步时间",
            (Language::Chinese, CloudFault::Unreachable) => "连不上服务器",
            (Language::Chinese, CloudFault::Tls) => "安全连接失败",
            (Language::Chinese, CloudFault::Http) => "返回内容无法识别",
            (Language::Chinese, CloudFault::Api) => "云端拒绝了请求",
            (Language::Chinese, CloudFault::Storage) => "板子没能保存结果",
            (Language::Chinese, CloudFault::Unsupported) => "这家云暂不支持",
            (Language::English, CloudFault::Failed) => "The request failed",
            (Language::English, CloudFault::Args) => "Bad request",
            (Language::English, CloudFault::NoCredentials) => "No API key",
            (Language::English, CloudFault::NoNetwork) => "Not on a network",
            (Language::English, CloudFault::ClockNotSet) => "The clock is not set",
            (Language::English, CloudFault::Unreachable) => "Could not reach the provider",
            (Language::English, CloudFault::Tls) => "The secure connection failed",
            (Language::English, CloudFault::Http) => "The answer could not be read",
            (Language::English, CloudFault::Api) => "The provider refused it",
            (Language::English, CloudFault::Storage) => "The board could not hold it",
            (Language::English, CloudFault::Unsupported) => "This vendor is not supported yet",
        }
    }
}
