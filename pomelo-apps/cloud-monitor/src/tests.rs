//! The page's own tests: what it asks the account for, and what it makes of the answer.
//!
//! Nothing here names a platform layer, reads a panel or draws a pixel. The account behind these tests is
//! the HAL's own simulator — two vendors, four machines between them, and a round trip that takes three
//! ticks — so what is asserted is the page's behaviour against an account that behaves the way the trait
//! says an account does: a request that is asked for, polled, and read exactly once.
//!
//! The half of that the pages beside it do not have is *stopping a machine*, and it is where most of
//! these tests are: an arming that expires, an arming a new pick puts down, and a list asked for again
//! once the provider has had time to move.

use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::Size;
use iced_test::Simulator;

use pomelo_hal::sim::SimCloud;
use pomelo_hal::{
    Board, CloudAction, CloudFault, CloudInstanceState, CloudKey, CloudProvider, CloudState,
    CloudVendor,
};
use pomelo_widgets::preferences::{Language, SystemPreferences, ThemeMode};

use crate::{CloudMonitor, Message, Notice, Words};

/// The design panel, which is what the layout is drawn for.
const PANEL: f32 = 480.0;

/// A screen tall enough to show the whole page without scrolling. The simulator has no scroll, so
/// anything below the fold is invisible to `find`. Generous rather than exact, for the channel map's
/// reason: a number that had to be re-tuned with every gap would turn a metric change into a failure.
const TALL: f32 = 1200.0;

/// How many ticks the simulator's round trip takes: its own constant, spelled out rather than exported,
/// so that a test can say how many frames of spinner a person is shown.
const LATENCY: usize = 3;

/// A board whose account is `cloud`, and the HAL's own simulators for everything else.
///
/// Written out at length because a board is a composition root's decision and a test is a composition
/// root: there is no `Board::simulated` with one backend chosen, and the one that has to be chosen here
/// is the account. See [`SimCloud::without_key`] and [`SimCloud::failing`].
fn board_of(cloud: SimCloud) -> Arc<Board> {
    use pomelo_hal::sim::{
        SimAudio, SimHttp, SimImu, SimInput, SimMic, SimNas, SimPower, SimStorage, SimSystem, SimWeb,
        SimWifi,
    };

    Arc::new(Board::from_backends(
        Box::new(SimPower::new()),
        Box::new(SimWifi::new()),
        Box::new(SimAudio::new()),
        Box::new(SimMic::new()),
        Box::new(SimImu::new()),
        Box::new(SimInput::new()),
        Box::new(SimStorage::new()),
        Box::new(SimWeb::new()),
        Box::new(SimSystem::new()),
        Box::new(SimNas::new()),
        Box::new(SimHttp::new()),
        Box::new(cloud),
    ))
}

/// A board with an account on it: two vendors set up, four machines between them.
fn board() -> Arc<Board> {
    board_of(SimCloud::new())
}

/// One frame of the page's subscription. The instant is read by one thing and one thing only — how long
/// a stop has been armed — and it is the message the subscription produces, so applying it by hand is
/// what a test without a loop does.
fn frame(app: &mut CloudMonitor) {
    app.update(Message::Tick(Instant::now()));
}

/// The round trip, to the end: the provider taking its time, and then the frame that reads the answer.
///
/// Both halves are needed and neither is optional. A page that never polled would sit on an answer for
/// ever, which is the whole of what [`CloudMonitor::busy`] is about.
fn settle(board: &Board, app: &mut CloudMonitor) {
    for _ in 0..LATENCY {
        board.tick();
    }
    frame(app);
}

/// The page, open on the account, with its first list behind it.
fn listed() -> (Arc<Board>, CloudMonitor) {
    let board = board();
    let mut app = CloudMonitor::new(Arc::clone(&board));

    settle(&board, &mut app);

    (board, app)
}

/// The page in English, which is the language the view tests assert labels in: the default is Chinese,
/// and a test that meant to check a label would otherwise pass or fail on a default it does not control.
fn english(app: &mut CloudMonitor) {
    app.set_preferences(SystemPreferences::new(Language::English, ThemeMode::Dark));
}

/// The page's widget tree on a screen `height` logical pixels tall.
///
/// The font is asked for by *generic* family, not by name: the tester's own default is "Fira Sans", which
/// only exists when iced's `fira-sans` feature is on — glyphs this firmware has no room for.
fn screen(app: &CloudMonitor, height: f32) -> Simulator<'_, Message> {
    Simulator::with_size(
        iced::Settings {
            default_font: iced::Font::MONOSPACE,
            ..iced::Settings::default()
        },
        Size::new(PANEL, height),
        app.view(),
    )
}

/// Opening the page is what asks the account — and the page says what the account says, not a guess.
///
/// The empty list before the first answer is the point of the test rather than an artefact of it: a page
/// that opened on "no instances" and only then asked would be telling a person their account is empty.
#[test]
fn opening_the_page_asks_for_the_list() {
    let board = board();
    let app = CloudMonitor::new(Arc::clone(&board));

    assert_eq!(board.cloud().state(), CloudState::Running, "the request is out");
    assert_eq!(app.state(), CloudState::Running);
    assert!(app.instances().is_empty(), "nothing has arrived yet");
    assert!(app.failed().is_empty(), "and nothing has failed yet");
    assert!(app.busy(), "so the page is showing a spinner");
    assert!(app.waiting(), "which is a reason to ask the platform for frames");
}

/// The list is read once, on the frame the account stops saying it is busy — not on every frame, because
/// taking an answer is a lock and a parse.
#[test]
fn the_list_is_read_when_the_account_is_done() {
    let board = board();
    let mut app = CloudMonitor::new(Arc::clone(&board));

    for _ in 0..LATENCY - 1 {
        board.tick();
    }
    frame(&mut app);
    assert!(app.busy(), "the provider takes three ticks and has had two");

    board.tick();
    frame(&mut app);

    assert!(!app.busy(), "and then it is over");
    assert_eq!(app.state(), CloudState::Idle);
    assert_eq!(app.vendor(), CloudVendor::Tencent, "the first vendor with a key");
    assert_eq!(app.instances().len(), 3, "both products in one list");
    assert_eq!(app.instances()[0].name, "web-01");
    assert_eq!(app.instances()[1].state, CloudInstanceState::Stopped);
    assert_eq!(app.instances()[2].name, "blog", "the other product is in it too");
    assert_eq!(app.instances()[2].provider, CloudProvider::Lighthouse);
    assert!(app.failed().is_empty(), "and both products answered");
    assert!(app.notice().is_none(), "a list that arrived says nothing else");
    assert!(!app.waiting(), "and there is nothing left to wait for");
}

/// One refresh is one request, and the list on the page stays up while the next one is on its way:
/// a page that cleared the list to ask for it again would blink at every press.
#[test]
fn a_refresh_keeps_the_list_up_while_the_next_one_is_asked_for() {
    let (board, mut app) = listed();

    app.update(Message::Refresh);
    assert!(app.busy());
    assert_eq!(app.instances().len(), 3, "the list stays until it is replaced");

    settle(&board, &mut app);
    assert_eq!(app.instances().len(), 3);
    assert_eq!(board.cloud().state(), CloudState::Idle);
}

/// A row is picked by id, and picking it again puts it down.
///
/// By id and not by row, because a power command replaces the list under the finger: an index would point
/// at whatever machine happens to be there when the answer comes back.
#[test]
fn a_row_is_picked_by_id_and_a_second_press_puts_it_down() {
    let (_, mut app) = listed();

    app.update(Message::Select(String::from("ins-e5f6a7b8")));
    assert_eq!(app.selected().unwrap().name, "db-01");

    app.update(Message::Select(String::from("ins-e5f6a7b8")));
    assert!(app.selected().is_none());

    // An id the list does not hold picks nothing: a machine that has gone away leaves no row standing.
    app.update(Message::Select(String::from("ins-nope")));
    assert!(app.selected().is_none());
}

/// Stopping asks twice. The first press arms, the second sends, and nothing has gone out in between.
#[test]
fn stopping_a_machine_asks_twice() {
    let (board, mut app) = listed();
    app.update(Message::Select(String::from("ins-a1b2c3d4")));

    app.update(Message::Power(CloudAction::Stop));
    assert!(app.armed(CloudAction::Stop), "the button is asking");
    assert!(!app.busy(), "and nothing has gone out");
    assert!(app.waiting(), "the arming is a reason for frames: it expires");

    app.update(Message::Power(CloudAction::Stop));
    assert!(!app.armed(CloudAction::Stop), "the second press is the one that sends");
    assert!(app.busy());

    settle(&board, &mut app);
    assert!(matches!(
        app.notice(),
        Some(Notice::Accepted {
            action: CloudAction::Stop,
            name
        }) if name == "web-01"
    ));
}

/// A start or a restart goes the first time: a machine that is already down is what the button was
/// pressed for, and there is nothing to lose twice.
#[test]
fn starting_and_restarting_go_the_first_time() {
    let (board, mut app) = listed();
    app.update(Message::Select(String::from("ins-e5f6a7b8")));

    app.update(Message::Power(CloudAction::Start));
    assert!(!app.armed(CloudAction::Start));
    assert!(app.busy());

    settle(&board, &mut app);
    assert!(matches!(
        app.notice(),
        Some(Notice::Accepted {
            action: CloudAction::Start,
            ..
        })
    ));
}

/// An armed stop puts itself down: a button touched while the panel is being set aside must not still be
/// armed when somebody looks at it again.
#[test]
fn an_armed_stop_expires() {
    let (_, mut app) = listed();
    app.update(Message::Select(String::from("ins-a1b2c3d4")));
    app.update(Message::Power(CloudAction::Stop));

    // The window is five seconds, and this is a clock ten seconds on rather than a real wait: what is
    // being tested is that the page reads the clock it is handed and not one of its own.
    app.update(Message::Tick(Instant::now() + Duration::from_secs(10)));
    assert!(!app.armed(CloudAction::Stop), "the arming is over");
    assert!(!app.busy(), "and nothing was sent");
}

/// Picking another row puts an arming down: a stop armed on one machine must not be sent to another.
#[test]
fn picking_another_row_puts_an_arming_down() {
    let (_, mut app) = listed();
    app.update(Message::Select(String::from("ins-a1b2c3d4")));
    app.update(Message::Power(CloudAction::Stop));
    assert!(app.armed(CloudAction::Stop));

    app.update(Message::Select(String::from("ins-e5f6a7b8")));
    assert!(!app.armed(CloudAction::Stop));
}

/// A receipt is not a result, so the list is asked for again a few ticks later — the only way the page
/// ever finds out that a command it was given a receipt for did not work.
#[test]
fn a_command_asks_for_the_list_again_after_it_lands() {
    let (board, mut app) = listed();
    app.update(Message::Select(String::from("ins-a1b2c3d4")));

    app.update(Message::Power(CloudAction::Stop));
    app.update(Message::Power(CloudAction::Stop));
    settle(&board, &mut app);
    assert!(!app.busy(), "the receipt is in");

    // The countdown starts on the frame the receipt was read — and that same frame spends one of it — so
    // this is two more: the first is still counting, the second asks. See `RELIST_AFTER`.
    frame(&mut app);
    assert!(!app.busy(), "the countdown is still running");

    frame(&mut app);
    assert!(app.busy(), "and then the list is asked for again");

    settle(&board, &mut app);
    assert_eq!(
        app.instances()[0].state,
        CloudInstanceState::Stopped,
        "the second list is the one that says how it went"
    );
}

/// A vendor with no key is not a vendor with an empty account, and the page says which of the two it is.
#[test]
fn a_vendor_with_no_key_says_where_the_key_goes() {
    let board = board_of(
        SimCloud::new()
            .without_key(CloudVendor::Tencent)
            .without_key(CloudVendor::Aliyun),
    );
    let mut app = CloudMonitor::new(Arc::clone(&board));

    assert_eq!(app.vendor(), CloudVendor::Tencent, "the one it opens on");
    assert!(!app.configured());
    assert!(!app.busy(), "nothing was asked, so nothing is spinning");
    assert!(app.instances().is_empty());
    assert!(app.notice().is_none(), "and it is not a failure");

    // Asking anyway is not a request: there is nothing to sign one with, and the card that says where
    // the key goes is already up.
    app.update(Message::Refresh);
    assert!(!app.busy());
    assert_eq!(app.state(), CloudState::Idle);
    assert_eq!(board.cloud().state(), CloudState::Idle);
}

/// The page opens on the vendor somebody has a key for: a person who typed one Alibaba key in is
/// looking for Alibaba's machines, and "no key" for the company they did not set up is not the screen
/// they came for.
#[test]
fn the_page_opens_on_the_vendor_that_has_a_key() {
    let board = board_of(SimCloud::new().without_key(CloudVendor::Tencent));
    let mut app = CloudMonitor::new(Arc::clone(&board));

    assert_eq!(app.vendor(), CloudVendor::Aliyun);
    assert!(app.configured());
    assert!(app.busy(), "which it can list");

    settle(&board, &mut app);
    assert_eq!(app.instances().len(), 1, "only the vendor with a key");
    assert_eq!(app.instances()[0].name, "api-01");
    assert_eq!(app.instances()[0].provider, CloudProvider::Ecs);
}

/// Switching vendors is switching lists: the rows of the company being left go, and the other's are
/// asked for rather than waited for.
#[test]
fn picking_another_vendor_replaces_the_list() {
    let (board, mut app) = listed();
    assert_eq!(app.instances().len(), 3);

    app.update(Message::Vendor(CloudVendor::Aliyun));
    assert_eq!(app.vendor(), CloudVendor::Aliyun);
    assert!(app.instances().is_empty(), "the last company's rows are gone");
    assert!(app.busy(), "and this one's list is on its way");

    settle(&board, &mut app);
    assert_eq!(app.instances().len(), 1);
    assert_eq!(app.instances()[0].name, "api-01");
    assert_eq!(app.instances()[0].provider, CloudProvider::Ecs);
}

/// A vendor this build has no client for says so and is not asked: an empty list would read as an
/// account with no machines, which is a different sentence, and a request would only come back
/// [`CloudFault::Unsupported`].
#[test]
fn a_vendor_with_no_client_is_not_asked() {
    let (board, mut app) = listed();

    app.update(Message::Vendor(CloudVendor::Aws));
    assert_eq!(app.vendor(), CloudVendor::Aws);
    assert!(!app.can_ask());
    assert!(!app.busy());
    assert_eq!(app.state(), CloudState::Idle);
    assert_eq!(board.cloud().state(), CloudState::Idle, "nothing went out");

    app.update(Message::Refresh);
    assert_eq!(board.cloud().state(), CloudState::Idle, "and nothing does");
}

/// An answer to a list nobody is looking at any more is dropped with the request: switching mid-flight
/// must not draw one company's machines under another company's name.
#[test]
fn switching_vendor_mid_request_abandons_the_one_in_flight() {
    let board = board();
    let mut app = CloudMonitor::new(Arc::clone(&board));
    assert!(app.busy());

    app.update(Message::Vendor(CloudVendor::Aliyun));
    settle(&board, &mut app);

    assert_eq!(app.instances().len(), 1);
    assert_eq!(app.instances()[0].provider, CloudProvider::Ecs);
}

/// A refusal is read once, kept until a list replaces it, and shown in the page's own words when the
/// provider wrote none — which is the half of error handling that has no text from anywhere else.
#[test]
fn a_fault_is_read_and_kept_until_a_list_replaces_it() {
    let board = board_of(SimCloud::new().failing(CloudFault::Unreachable));
    let mut app = CloudMonitor::new(Arc::clone(&board));

    settle(&board, &mut app);

    assert_eq!(app.state(), CloudState::Failed(CloudFault::Unreachable));
    assert!(app.instances().is_empty());

    let Some(Notice::Refused {
        fault,
        code,
        message,
    }) = app.notice()
    else {
        panic!("a request that did not come back should be reported");
    };
    assert_eq!(*fault, CloudFault::Unreachable);
    assert!(code.is_empty() && message.is_empty(), "the provider wrote nothing");

    let words = Words::of(Language::English);
    assert_eq!(
        words.refused(*fault, code, message),
        "Could not reach the provider"
    );
}

/// One request at a time: a second press while the list is on its way is the request already in
/// flight, not a second one behind it.
#[test]
fn a_second_refresh_while_a_list_is_in_flight_is_dropped() {
    let board = board();
    let mut app = CloudMonitor::new(Arc::clone(&board));

    app.update(Message::Refresh);
    assert!(app.busy());
    assert_eq!(board.cloud().state(), CloudState::Running);

    settle(&board, &mut app);
    assert_eq!(app.instances().len(), 3, "the first list is the one that arrived");
}

/// Leaving the page hands its slot back: an app the launcher has dropped is an answer nobody will read.
#[test]
fn dropping_the_page_abandons_the_request_in_flight() {
    let board = board();
    let app = CloudMonitor::new(Arc::clone(&board));
    assert_eq!(board.cloud().state(), CloudState::Running);

    drop(app);

    assert_eq!(board.cloud().state(), CloudState::Idle);
    // And the abandoned round trip is not still on its way: an answer that arrived later would be taken
    // by whoever asked next.
    for _ in 0..LATENCY {
        board.tick();
    }
    assert_eq!(board.cloud().state(), CloudState::Idle);
}

/// A key is read when the page opens and read again when an answer arrives, which is the only two
/// moments it can change underneath the page. Masked, and only masked: nothing here holds a secret.
#[test]
fn the_key_the_page_shows_is_the_masked_one() {
    let (_, app) = listed();

    let key: CloudKey = app.key(CloudVendor::Tencent);
    assert!(key.configured());
    assert_eq!(key.region, "ap-guangzhou");
    assert!(!key.masked.is_empty());

    // The other company's key is a different row of the table, and its own region.
    let aliyun: CloudKey = app.key(CloudVendor::Aliyun);
    assert!(aliyun.configured());
    assert_eq!(aliyun.region, "cn-hangzhou");
}

/// The words are in both languages, and the count is said the way each language says it.
#[test]
fn the_words_are_in_both_languages() {
    let english = Words::of(Language::English);
    let chinese = Words::of(Language::Chinese);

    assert_ne!(english.title, chinese.title);
    assert_eq!(english.vendor(CloudVendor::Aliyun), "Alibaba");
    assert_eq!(chinese.vendor(CloudVendor::Tencent), "腾讯云");
    assert_eq!(english.provider(CloudProvider::Ecs), "ECS");
    assert_eq!(chinese.provider(CloudProvider::Lighthouse), "轻量应用服务器");
    assert_eq!(
        english.fault(CloudFault::Unsupported),
        "This vendor is not supported yet"
    );

    assert_eq!(english.count(1), "1 instance");
    assert_eq!(english.count(3), "3 instances");
    assert!(chinese.count(3).contains('3'));

    assert_eq!(english.state(CloudInstanceState::Running), "Running");
    assert_eq!(chinese.state(CloudInstanceState::Running), "运行中");
    // The fifth state has no word in either language: a guess would be worse than a dash.
    assert_eq!(english.state(CloudInstanceState::Other), "—");
    assert_eq!(chinese.state(CloudInstanceState::Other), "—");

    // A command is named by the same word as the button that sends it.
    assert_eq!(english.action(CloudAction::Stop), english.stop);
    assert_eq!(english.action(CloudAction::List), english.refresh);
}

/// The page draws what the list says: the machines by name, what they are doing, and which product
/// each row came from — the one thing a merged list would otherwise lose.
#[test]
fn the_page_draws_the_machines_it_listed() -> Result<(), iced_test::Error> {
    let (_, mut app) = listed();
    english(&mut app);

    let mut ui = screen(&app, TALL);

    assert!(ui.find("Cloud").is_ok(), "the page says what it is");
    assert!(ui.find("Instances").is_ok(), "the card has a heading");
    assert!(ui.find("Tencent").is_ok(), "the list names the company it is");
    assert!(ui.find("Alibaba").is_ok(), "and the other companies can be reached");
    assert!(ui.find("AWS").is_ok(), "even the one with no client");
    assert!(ui.find("web-01").is_ok(), "a machine by name");
    assert!(ui.find("blog").is_ok(), "and the other product's machine");
    assert!(ui.find("Cloud servers").is_ok(), "a row tagged with its product");
    assert!(ui.find("Lightweight").is_ok(), "and the other product's tag");
    assert!(ui.find("Stopped").is_ok(), "and the one that is down");
    assert!(ui.find("Pick a machine to act on").is_ok(), "and what to do next");

    Ok(())
}

/// With a machine picked, the commands are on the page — and the arming is a word on the button, which is
/// the whole of how a person knows the first press was taken.
#[test]
fn the_page_draws_the_commands_for_the_machine_that_is_picked() -> Result<(), iced_test::Error> {
    let (_, mut app) = listed();
    english(&mut app);
    app.update(Message::Select(String::from("ins-a1b2c3d4")));

    // The tree is built, asked, and dropped before the page is touched again: a simulator borrows the
    // page it was drawn from, so the two screens are two borrows and not one.
    {
        let mut ui = screen(&app, TALL);
        assert!(ui.find("Stop").is_ok(), "a machine that is up can be stopped");
        assert!(ui.find("Restart").is_ok());
    }

    app.update(Message::Power(CloudAction::Stop));
    let mut ui = screen(&app, TALL);
    assert!(ui.find("Confirm stop").is_ok());

    Ok(())
}

/// The card that stands in for the list when there is no key to list with — with the chips still on
/// the page, which is the only way out of a vendor somebody has not set up.
#[test]
fn the_page_draws_where_the_key_goes() -> Result<(), iced_test::Error> {
    let board = board_of(
        SimCloud::new()
            .without_key(CloudVendor::Tencent)
            .without_key(CloudVendor::Aliyun),
    );
    let mut app = CloudMonitor::new(Arc::clone(&board));
    english(&mut app);

    let mut ui = screen(&app, TALL);
    assert!(ui.find("No API key yet for this vendor").is_ok());
    assert!(ui.find("Fill one in under Cloud on the web page").is_ok());
    assert!(ui.find("Alibaba").is_ok(), "and the other vendors are reachable");

    Ok(())
}

/// The card that stands in for a vendor this build has no client for: not a missing key, and not an
/// empty account.
#[test]
fn the_page_draws_that_a_vendor_has_no_client() -> Result<(), iced_test::Error> {
    let (_, mut app) = listed();
    english(&mut app);
    app.update(Message::Vendor(CloudVendor::Aws));

    let mut ui = screen(&app, TALL);
    assert!(ui.find("This vendor is not supported yet").is_ok());
    assert!(ui.find("This build has no client for it").is_ok());

    Ok(())
}
