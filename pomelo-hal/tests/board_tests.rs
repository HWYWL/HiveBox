//! Integration tests for the HAL facade and the desktop simulator backends.
//!
//! These run on the host, where the board under test is [`Board::simulated`].

use pomelo_hal::{
    Board, HalError, HttpState, ScanState, StorageBackend, VolumeKind, WifiState,
};

/// Write a minimal 16-bit mono PCM WAV (0.1 s @ 8 kHz) to a temp file and
/// return its path, so the audio backend has a real file to parse.
fn write_temp_wav() -> String {
    let data_len: u32 = 1600; // 8000 Hz * 2 bytes * 0.1 s
    let mut b: Vec<u8> = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVE");
    b.extend_from_slice(b"fmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes()); // PCM
    b.extend_from_slice(&1u16.to_le_bytes()); // channels
    b.extend_from_slice(&8000u32.to_le_bytes()); // sample rate
    b.extend_from_slice(&16000u32.to_le_bytes()); // byte rate
    b.extend_from_slice(&2u16.to_le_bytes()); // block align
    b.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    b.resize(b.len() + data_len as usize, 0);

    let path = std::env::temp_dir().join("pomelo_hal_sim_audio.wav");
    std::fs::write(&path, &b).unwrap();
    path.to_string_lossy().to_string()
}

#[test]
fn board_constructs_and_exposes_every_subsystem() {
    let board = Board::simulated();
    // Every accessor must be reachable without panicking or deadlocking.
    assert!(board.power().battery_percent().is_ok());
    assert_eq!(board.wifi().status().state, WifiState::Disconnected);
    assert!(!board.audio().is_playing());
    assert!(!board.mic().is_recording());
    assert!(board.imu().read_accel().is_ok());
    assert_eq!(board.input().poll_action(), None);
    board.input().push_action(pomelo_hal::InputAction::Back);
    assert_eq!(board.input().poll_action(), Some(pomelo_hal::InputAction::Back));
    assert!(!board.storage().volumes().unwrap().is_empty());
    board.tick();
}

#[test]
fn sim_storage_reports_the_built_in_volume_and_the_card() {
    let board = Board::simulated();
    board.storage().init().unwrap();
    board.storage().refresh().unwrap();

    let volumes = board.storage().volumes().unwrap();
    assert_eq!(volumes.len(), 2, "the built-in partition and the card: {volumes:?}");

    let (internal, card) = (&volumes[0], &volumes[1]);
    assert_eq!(internal.kind, VolumeKind::Internal);
    assert_eq!(card.kind, VolumeKind::Removable);
    assert_eq!(internal.mount_point, "/internal");
    assert_eq!(card.mount_point, "/sdcard");

    for volume in &volumes {
        assert!(volume.total_bytes > 0, "{volume:?} reports no size");
        assert!(
            volume.free_bytes <= volume.total_bytes,
            "{volume:?} is more free than it is big"
        );
        assert!((0.0..=100.0).contains(&volume.used_percent()));
    }
}

/// A card that appears after boot is a card the next `volumes` shows.
///
/// This is the whole reason `refresh` exists: there is no card-detect pin on the reference board, so
/// "is there one in the slot?" is asked by asking, and the answer changes between one call and the
/// next.
#[test]
fn sim_storage_notices_a_card_that_is_inserted() {
    use pomelo_hal::sim::SimStorage;

    let mut storage = SimStorage::without_card();
    assert!(!storage.has_card());

    let only_internal = storage.volumes().unwrap();
    assert_eq!(only_internal.len(), 1);
    assert_eq!(only_internal[0].kind, VolumeKind::Internal);

    storage.insert();
    storage.refresh().unwrap();

    assert!(storage.has_card());
    let both = storage.volumes().unwrap();
    assert_eq!(both.len(), 2);
    assert_eq!(both[1].kind, VolumeKind::Removable);
}

#[test]
fn sim_power_reports_valid_ranges() {
    let board = Board::simulated();
    board.power().init().unwrap();

    let pct = board.power().battery_percent().unwrap();
    assert!(pct <= 100, "battery percent must be <= 100, got {pct}");

    let voltage = board.power().battery_voltage_mv().unwrap();
    assert!(
        (3300..=4200).contains(&voltage),
        "voltage must map into 3300..=4200 mV, got {voltage}"
    );

    let _charging = board.power().is_charging().unwrap();
}

#[test]
fn sim_wifi_scan_lifecycle() {
    let board = Board::simulated();
    board.wifi().init().unwrap();
    assert!(board.wifi().is_enabled());
    assert_eq!(board.wifi().scan_state(), ScanState::Idle);

    board.wifi().scan_start().unwrap();
    assert_eq!(board.wifi().scan_state(), ScanState::Scanning);

    // Results are not ready mid-scan.
    assert!(matches!(board.wifi().scan_results(), Err(HalError::Busy)));

    // Cooperative ticks drive the simulated scan to completion.
    for _ in 0..3 {
        board.wifi().tick();
    }
    assert_eq!(board.wifi().scan_state(), ScanState::Done);

    let aps = board.wifi().scan_results().unwrap();
    assert!(aps.len() >= 3, "expected several simulated APs");
    assert!(
        aps.iter().any(|ap| ap.ssid == "Pomelo-OS"),
        "expected the Pomelo-OS AP in scan results"
    );
    // Bars are derived from RSSI and must stay within the icon range.
    for ap in &aps {
        assert!(ap.signal_bars() <= 4);
    }
}

#[test]
fn sim_wifi_connect_requires_valid_credentials() {
    let board = Board::simulated();

    // Unknown SSID is rejected.
    assert!(matches!(
        board.wifi().connect("Nope", "secret"),
        Err(HalError::InvalidArg)
    ));

    // A secured AP requires a password.
    assert!(matches!(
        board.wifi().connect("Pomelo-OS", ""),
        Err(HalError::InvalidArg)
    ));

    // A valid connection flips the status to Connected.
    board.wifi().connect("Pomelo-OS", "hunter2").unwrap();
    let status = board.wifi().status();
    assert_eq!(status.state, WifiState::Connected);
    assert_eq!(status.ssid, "Pomelo-OS");
    assert_eq!(status.ip, "192.168.1.108");

    board.wifi().disconnect().unwrap();
    assert_eq!(board.wifi().status().state, WifiState::Disconnected);

    // Disabling the radio rejects scanning.
    board.wifi().set_enabled(false).unwrap();
    assert!(matches!(
        board.wifi().scan_start(),
        Err(HalError::NotInitialized)
    ));
}

/// The credentials a test remembers.
fn credentials(ssid: &str, autoconnect: bool) -> pomelo_hal::wifi_credentials::WifiCredentials {
    pomelo_hal::wifi_credentials::WifiCredentials {
        enabled: true,
        ssid: ssid.to_string(),
        password: "hunter2".to_string(),
        autoconnect,
    }
}

/// What the file says is what the board does when it comes up.
#[test]
fn sim_wifi_boot_connects_to_the_remembered_network() {
    let board = Board::simulated();

    board
        .wifi()
        .remember(&credentials("Pomelo-OS", true))
        .unwrap();
    board.wifi().autoconnect().unwrap();

    let status = board.wifi().status();
    assert_eq!(status.state, WifiState::Connected);
    assert_eq!(status.ssid, "Pomelo-OS");
}

/// `autoconnect = false` is a network to keep, not one to join unasked.
#[test]
fn sim_wifi_boot_leaves_a_network_alone_when_the_file_says_to() {
    let board = Board::simulated();

    board
        .wifi()
        .remember(&credentials("Pomelo-OS", false))
        .unwrap();
    board.wifi().autoconnect().unwrap();

    assert_eq!(board.wifi().status().state, WifiState::Disconnected);
    assert_eq!(
        board.wifi().saved().map(|saved| saved.ssid),
        Some("Pomelo-OS".to_string()),
        "still remembered, and still not joined"
    );
}

/// A board that has never been on a network comes up on nothing at all — but on.
///
/// The radio and the network are two answers, and this is the board out of the box: there is nothing
/// to join because nothing has been remembered, and that is not a reason to leave the radio down. The
/// switch is where a person starts.
#[test]
fn sim_wifi_boot_without_a_file_joins_nothing() {
    let board = Board::simulated();

    board.wifi().autoconnect().unwrap();

    assert_eq!(board.wifi().status().state, WifiState::Disconnected);
    assert!(
        board.wifi().is_enabled(),
        "the radio is up, and unconnected rather than off"
    );
}

/// A file that says the switch was off keeps the radio down across a reboot.
///
/// The other half of the default: on is what happens when nothing was said, and a finger that turned
/// the radio off said something.
#[test]
fn sim_wifi_boot_respects_a_switch_left_off() {
    let board = Board::simulated();

    let mut credentials = credentials("Pomelo-OS", true);
    credentials.enabled = false;
    board.wifi().remember(&credentials).unwrap();

    board.wifi().autoconnect().unwrap();

    assert!(!board.wifi().is_enabled(), "the radio stays down");
    assert_eq!(board.wifi().status().state, WifiState::Disconnected);
}

#[test]
fn sim_audio_playback_lifecycle() {
    let board = Board::simulated();
    let path = write_temp_wav();

    let meta = board.audio().play(&path).unwrap();
    assert_eq!(meta.sample_rate, 8000);
    assert_eq!(meta.channels, 1);
    assert_eq!(meta.bits_per_sample, 16);
    assert!(board.audio().is_playing());

    board.audio().pause();
    assert!(!board.audio().is_playing());

    board.audio().resume();
    assert!(board.audio().is_playing());

    board.audio().stop();
    assert!(!board.audio().is_playing());
    assert_eq!(board.audio().position_secs(), 0.0);

    // A missing file must surface as an error rather than a panic.
    assert!(board.audio().play("/no/such/file.wav").is_err());
}

#[test]
fn sim_mic_capture() {
    let board = Board::simulated();
    assert!(!board.mic().is_recording());

    // Reading before starting must fail.
    let mut buf = [0i16; 64];
    assert!(board.mic().read(&mut buf).is_err());

    board.mic().record_start().unwrap();
    assert!(board.mic().is_recording());

    let n = board.mic().read(&mut buf).unwrap();
    assert_eq!(n, buf.len());
    assert!(
        buf.iter().any(|&s| s != 0),
        "simulated microphone should produce non-silent samples"
    );

    board.mic().record_stop().unwrap();
    assert!(!board.mic().is_recording());
}

#[test]
fn sim_imu_reads_finite_values() {
    let board = Board::simulated();

    let accel = board.imu().read_accel().unwrap();
    let magnitude = accel.magnitude();
    assert!(
        (9.0..=11.0).contains(&magnitude),
        "gravity magnitude should be near 9.81, got {magnitude}"
    );

    let gyro = board.imu().read_gyro().unwrap();
    assert!(gyro.magnitude() < 1.0);

    let temp = board.imu().temperature_c().unwrap();
    assert!((20.0..=45.0).contains(&temp), "unexpected temp {temp}");
}

#[test]
fn board_event_listener_and_tick_change_detection() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    let board = Board::simulated();
    let event_count = Arc::new(AtomicUsize::new(0));

    let count_clone = Arc::clone(&event_count);
    board.on_event(move |_event| {
        count_clone.fetch_add(1, Ordering::SeqCst);
    });

    // First tick detects initial state and emits events (Power & Wifi)
    board.tick();
    assert!(
        event_count.load(Ordering::SeqCst) >= 2,
        "initial tick should emit power and wifi status events"
    );

    // Explicit emit
    board.emit_event(pomelo_hal::SystemEvent::InputAction(pomelo_hal::InputAction::Back));
    assert!(event_count.load(Ordering::SeqCst) >= 3);
}

// =============================================================================
// The HTTP client
// =============================================================================

/// A request asked for and answered: the whole of what the HTTP client promises.
///
/// The four states in order, and the rule that ties them together: the answer is collected *once*,
/// by the frame that saw `Done`.
#[test]
fn sim_http_request_lifecycle() {
    let board = Board::simulated();

    assert_eq!(board.http().state(), HttpState::Idle, "nothing asked for yet");

    board.http().start("https://example.com/forecast").unwrap();
    assert_eq!(board.http().state(), HttpState::Running);

    // Nothing to take mid-flight: the answer is not late, it is not there yet.
    assert!(matches!(board.http().take_result(), Err(HalError::Busy)));

    // Cooperative ticks drive the simulated request to completion.
    for _ in 0..3 {
        board.tick();
    }
    assert_eq!(board.http().state(), HttpState::Done);

    let response = board.http().take_result().unwrap();
    assert!(response.ok());
    assert_eq!(response.status, 200);

    // Taken once, and the backend is free again afterwards.
    assert_eq!(board.http().state(), HttpState::Idle);
    assert!(matches!(board.http().take_result(), Err(HalError::Busy)));
}

/// What the page asked for is what the client was asked for.
///
/// The URL carries the whole request — the city's coordinates, the days of forecast — so a page that
/// built the wrong one is a page whose answer is for somewhere else, and this is where that is
/// caught. `last_url` is the simulator's own, which is why the assertion is made against a client
/// rather than through the board: nothing above the trait gets to depend on it.
#[test]
fn sim_http_remembers_the_url_it_was_given() {
    use pomelo_hal::sim::SimHttp;
    use pomelo_hal::HttpBackend;

    let mut http = SimHttp::answering("{}");
    assert_eq!(http.last_url(), None);

    let url = "https://api.example.com/v1/forecast?latitude=52.52&longitude=13.41";
    http.start(url).unwrap();
    assert_eq!(http.last_url(), Some(url));
}

/// A request in flight is the only one in flight.
#[test]
fn sim_http_takes_one_request_at_a_time() {
    let board = Board::simulated();

    board.http().start("https://example.com/a").unwrap();

    assert!(
        matches!(board.http().start("https://example.com/b"), Err(HalError::Busy)),
        "a second request would overtake the first"
    );
}

/// What is not a URL is refused as an argument, not reported as a network fault.
#[test]
fn sim_http_refuses_a_url_it_cannot_fetch() {
    let board = Board::simulated();

    for url in ["", "example.com", "ftp://example.com/a", "https:///a"] {
        assert!(
            matches!(board.http().start(url), Err(HalError::InvalidArg)),
            "{url:?} should be refused"
        );
    }

    assert_eq!(
        board.http().state(),
        HttpState::Idle,
        "a refused request leaves nothing behind"
    );
}

/// A board off the network fails, and says *which* way it failed.
///
/// The state is where a fault is reported, and it is not a result: there is nothing to collect, so
/// asking for one has to be an error rather than an empty answer that a page would draw as weather.
#[test]
fn sim_http_offline_reports_the_fault_and_has_no_result() {
    use pomelo_hal::sim::SimHttp;
    use pomelo_hal::HttpFault;

    let board = Board::simulated().with_http(Box::new(SimHttp::offline()));

    board.http().start("https://example.com/forecast").unwrap();
    for _ in 0..3 {
        board.tick();
    }

    assert_eq!(board.http().state(), HttpState::Failed(HttpFault::NoNetwork));
    assert!(matches!(board.http().take_result(), Err(HalError::Busy)));
}

/// A body and a status arrive together, and are the server's rather than the client's.
#[test]
fn sim_http_hands_over_the_body_and_the_status() {
    use pomelo_hal::sim::SimHttp;

    let board = Board::simulated()
        .with_http(Box::new(SimHttp::answering("<html>gone</html>").with_status(404)));

    board.http().start("https://example.com/forecast").unwrap();
    for _ in 0..3 {
        board.tick();
    }

    let response = board.http().take_result().unwrap();
    assert_eq!(response.status, 404);
    assert!(!response.ok());
    assert_eq!(response.text(), Some("<html>gone</html>"));
}

/// A request that failed does not hold the slot: the next one is taken.
///
/// The one case a page cannot work around: a page that drew a fault has no answer to collect and
/// nothing to cancel, so a client that went on refusing `start` would be a page that could never try
/// again — and "turn the Wi-Fi on and press refresh" is the whole of what a fault line is for.
#[test]
fn sim_http_a_failed_request_frees_the_slot_for_the_next_one() {
    use pomelo_hal::sim::SimHttp;
    use pomelo_hal::HttpFault;

    let board = Board::simulated().with_http(Box::new(SimHttp::offline()));

    board.http().start("https://example.com/a").unwrap();
    for _ in 0..3 {
        board.tick();
    }
    assert_eq!(board.http().state(), HttpState::Failed(HttpFault::NoNetwork));

    board.http().start("https://example.com/b").unwrap();
    assert_eq!(board.http().state(), HttpState::Running, "the second try");

    // And an answer nobody collected still is busy: that is a page that lost track of a request,
    // which is a different mistake and must not be swallowed.
    let board = Board::simulated();
    board.http().start("https://example.com/c").unwrap();
    for _ in 0..3 {
        board.tick();
    }
    assert_eq!(board.http().state(), HttpState::Done);
    assert!(matches!(
        board.http().start("https://example.com/d"),
        Err(HalError::Busy)
    ));
}

/// Cancelling abandons the request, and abandoning nothing is not an error.
#[test]
fn sim_http_cancel_leaves_it_idle() {
    let board = Board::simulated();

    board.http().cancel().unwrap();

    board.http().start("https://example.com/a").unwrap();
    board.http().cancel().unwrap();
    assert_eq!(board.http().state(), HttpState::Idle);

    // The slot is free again, so the next request is taken rather than refused.
    board.http().start("https://example.com/b").unwrap();
    assert_eq!(board.http().state(), HttpState::Running);
}

/// Bytes that are not text are not text.
///
/// The body of an answer is bytes, and a page that wanted a `String` has to be told when the answer
/// is not one — the alternative is a lossy conversion that turns somebody else's binary into this
/// page's words.
#[test]
fn http_response_text_is_none_for_bytes_that_are_not_utf8() {
    use pomelo_hal::HttpResponse;

    let response = HttpResponse {
        status: 200,
        body: vec![0xff, 0xfe, 0x00],
    };
    assert_eq!(response.text(), None);

    let response = HttpResponse {
        status: 200,
        body: b"{\"a\":1}".to_vec(),
    };
    assert_eq!(response.text(), Some("{\"a\":1}"));
}

/// A file sink puts the bytes in the file, and the response comes back with an empty body.
///
/// This is the whole of the difference between the two sinks, and the reason a caller has to remember
/// which it asked for: the answer to a glyph-table request is two megabytes and belongs on the card, so
/// there is nothing in the response to hand over and the file is the only place the bytes are.
#[test]
fn sim_http_a_file_sink_writes_the_body_where_it_was_asked_to() {
    use pomelo_hal::sim::SimHttp;
    use pomelo_hal::{HttpBackend, Sink};

    let path = std::env::temp_dir().join("pomelo_hal_sim_http_file_sink.bin");
    let _ = std::fs::remove_file(&path);
    let path = path.to_string_lossy().into_owned();

    let mut http = SimHttp::answering("glyphs");
    http.start_to("https://example.com/font.bin", Sink::File(path.clone()))
        .unwrap();

    // The sink is kept, not just used: it is the only thing that says where the answer went, and a test
    // that could only see the body would pass on a page that wrote to the wrong path.
    assert_eq!(http.last_sink(), Some(&Sink::File(path.clone())));

    for _ in 0..8 {
        http.tick();
    }

    let response = http.take_result().unwrap();
    assert_eq!(response.status, 200);
    assert!(
        response.body.is_empty(),
        "a file sink's bytes are in the file, not the buffer"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "glyphs");

    let _ = std::fs::remove_file(&path);
}

/// A file sink that cannot be written is a fault of the board, not of the network.
///
/// The distinction is the one a person acts on: nothing was wrong with the request, and what they do
/// about it is look at the card rather than at the Wi-Fi.
#[test]
fn sim_http_a_file_sink_that_cannot_be_written_is_a_storage_fault() {
    use pomelo_hal::sim::SimHttp;
    use pomelo_hal::{HttpBackend, HttpFault, Sink};

    // Under a directory that does not exist, which is what a card that is not in the slot looks like
    // from here.
    let path = std::env::temp_dir()
        .join("pomelo_hal_no_such_directory")
        .join("font.bin");

    let mut http = SimHttp::answering("glyphs");
    http.start_to(
        "https://example.com/font.bin",
        Sink::File(path.to_string_lossy().into_owned()),
    )
    .unwrap();

    for _ in 0..8 {
        http.tick();
    }

    assert_eq!(http.state(), HttpState::Failed(HttpFault::Storage));
    assert!(http.take_result().is_err(), "a failed request has no answer");
}

/// A file sink with no file in it is an argument error, not a fault.
///
/// It is a page that built a path wrong, and telling it "the card is full" would send whoever reads the
/// message looking in the wrong place.
#[test]
fn sim_http_refuses_a_file_sink_with_no_path() {
    use pomelo_hal::sim::SimHttp;
    use pomelo_hal::{HttpBackend, Sink};

    let mut http = SimHttp::answering("glyphs");

    assert!(matches!(
        http.start_to("https://example.com/font.bin", Sink::File(String::new())),
        Err(HalError::InvalidArg)
    ));
    assert_eq!(http.state(), HttpState::Idle);
}

/// A slot sink puts the bytes where a partition would be, and the response comes back with an empty
/// body — the same shape as a file's, because neither of those two is the buffer.
///
/// The slot is a stand-in for the second app partition, which a desktop process does not have. What can
/// be shown here is that the bytes a page asked to *install* reached the destination rather than the
/// answer, which is the half of an update that can be tested off the board.
#[test]
fn sim_http_an_ota_sink_puts_the_body_in_the_slot() {
    use pomelo_hal::sim::SimHttp;
    use pomelo_hal::{HttpBackend, Sink};

    let mut http = SimHttp::answering("a whole image");
    assert!(http.slot().is_empty(), "nothing has been installed yet");

    http.start_to("https://example.com/fw.bin", Sink::Ota)
        .unwrap();

    // Kept and not merely used: the sink is what says *where* the answer was aimed, and which slot is
    // not a caller's to name — see `Sink::Ota`.
    assert_eq!(http.last_sink(), Some(&Sink::Ota));

    for _ in 0..8 {
        http.tick();
    }

    let response = http.take_result().unwrap();
    assert_eq!(response.status, 200);
    assert!(
        response.body.is_empty(),
        "a slot sink's bytes are in the slot, not the buffer"
    );
    assert_eq!(http.slot(), "a whole image".as_bytes());
}

/// A slot is written to by a *success* and by nothing else.
///
/// The rule the device's client keeps, and the reason it is worth keeping here: a slot holding the
/// `404` page's HTML is worse than one holding the last good image, because the next boot starts from
/// whatever the boot partition points at. So the body of an answer that is not a success is dropped,
/// and the slot keeps what it had.
#[test]
fn sim_http_an_ota_sink_is_not_written_from_a_failure() {
    use pomelo_hal::sim::SimHttp;
    use pomelo_hal::{HttpBackend, Sink};

    let mut http = SimHttp::answering("the previous image").then((404u16, "<html>gone</html>"));

    http.start_to("https://example.com/fw.bin", Sink::Ota)
        .unwrap();
    for _ in 0..8 {
        http.tick();
    }
    let response = http.take_result().unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(http.slot(), "the previous image".as_bytes());

    // The next request is answered with the not-found page, which is an answer and not a fault: the
    // status is carried up, and it is the *slot* the status decides for.
    http.start_to("https://example.com/fw.bin", Sink::Ota)
        .unwrap();
    for _ in 0..8 {
        http.tick();
    }

    let response = http.take_result().unwrap();
    assert_eq!(response.status, 404, "the status is the answer's to carry");
    assert_eq!(
        http.slot(),
        "the previous image".as_bytes(),
        "and the status is the slot's to judge, so nothing was written"
    );
}

