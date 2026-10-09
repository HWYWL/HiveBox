//! The player's own tests: what the controls do, through the widget tree.
//!
//! Nothing here names a platform layer, reads a panel or draws a pixel. `iced_test`'s simulator
//! builds the *real* widget tree, finds a control by the label it is drawn with, presses it, and
//! hands back the message that produced. What the player costs the board — frames, damage, a finger
//! on the button — is asserted in `firmware/panel-tests`, where the panel is.
//!
//! The track these tests play is generated into a directory of the test's own. The repository's
//! `assets/music` holds a user's music, and a suite that passes only because those files happen to
//! be there is testing the checkout, not the player.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use iced::Size;
use iced_test::Simulator;

use music_player::{Message, Page, PlaybackStatus, Player, SCREEN};
use pomelo_hal::Board;
use pomelo_material_symbols::Icon;

/// The board the player is given here: the HAL's desktop simulator, since what is being tested is
/// the interface and not the hardware.
fn board() -> Arc<Board> {
    Arc::new(Board::simulated())
}

/// The same board with the card taken out of the slot — the state a person meets, and the one the
/// player has to speak about rather than report as a missing file.
fn board_without_card() -> Arc<Board> {
    use pomelo_hal::sim::{
        SimAudio, SimImu, SimInput, SimMic, SimPower, SimStorage, SimWeb, SimWifi,
    };

    Arc::new(Board::from_backends(
        Box::new(SimPower::new()),
        Box::new(SimWifi::new()),
        Box::new(SimAudio::new()),
        Box::new(SimMic::new()),
        Box::new(SimImu::new()),
        Box::new(SimInput::new()),
        Box::new(SimStorage::without_card()),
        Box::new(SimWeb::new()),
    ))
}

/// A player with a scratch directory of its own.
fn player() -> Player {
    Player::new(board())
}

fn interface(player: &Player) -> Simulator<'_, Message> {
    Simulator::with_size(
        iced::Settings {
            // By *generic* family: the tester's default is "Fira Sans", which only exists when
            // iced's `fira-sans` feature is on — 441 KiB of glyphs this firmware has no room for.
            default_font: iced::Font::MONOSPACE,
            ..iced::Settings::default()
        },
        Size::new(SCREEN as f32, SCREEN as f32),
        player.view(),
    )
}

/// Presses `label` in a freshly built tree of `player`'s current state, and applies what came back.
fn press(player: &mut Player, label: &str) -> Result<(), iced_test::Error> {
    let mut ui = interface(player);
    let _ = ui.click(label)?;

    for message in ui.into_messages() {
        player.update(message);
    }

    Ok(())
}

fn scratch_dir() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);

    let dir = std::env::temp_dir().join(format!(
        "pomelo-music-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("a scratch directory for generated audio");

    dir
}

/// A valid three-second WAV in `dir`, and its path as the model wants it.
fn write_sample(dir: &Path) -> String {
    const SAMPLE_RATE: u32 = 22050;
    const SECONDS: u32 = 3;

    let samples = SAMPLE_RATE * SECONDS;
    let data_len = samples * 2;

    let mut bytes = Vec::with_capacity(44 + data_len as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVE");
    bytes.extend_from_slice(b"fmt ");
    bytes.extend_from_slice(&(16u32).to_le_bytes());
    bytes.extend_from_slice(&(1u16).to_le_bytes()); // PCM
    bytes.extend_from_slice(&(1u16).to_le_bytes()); // Mono
    bytes.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    bytes.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes()); // byte rate
    bytes.extend_from_slice(&(2u16).to_le_bytes()); // block align
    bytes.extend_from_slice(&(16u16).to_le_bytes()); // bits
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());

    for sample in 0..samples {
        let t = sample as f32 / SAMPLE_RATE as f32;
        let value = (12000.0 * (std::f32::consts::TAU * 440.0 * t).sin()) as i16;
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    let path = dir.join("sample.wav");
    std::fs::write(&path, &bytes).expect("the generated sample");

    path.to_str().expect("a utf-8 scratch path").to_string()
}

/// Hands the player exactly one track of its own, instead of whatever the model's search
/// directories happen to hold, and answers with the title it should now show.
fn with_one_track(player: &mut Player) -> String {
    let dir = scratch_dir();
    write_sample(&dir);

    let dir = dir.to_str().expect("a utf-8 scratch path").to_string();
    player.refresh_playlist_in(&[dir.as_str()]);

    "sample".to_string()
}

/// The app opens on the list, and nothing is playing: opening a player is not asking it to play.
///
/// This is the whole of why the list exists. The panel is looked at far more often than it is
/// listened to, and a board that starts a track because an app was opened is a board with a mind of
/// its own.
#[test]
fn the_player_opens_on_the_list_with_nothing_playing() {
    use pomelo_widgets::preferences::{FontSizeTier, Language, SystemPreferences, ThemeMode};

    let mut player = player();
    with_one_track(&mut player);

    assert_eq!(player.page(), Page::Library);
    assert_eq!(player.status(), PlaybackStatus::Stopped);

    player.set_preferences(SystemPreferences::new(
        Language::English,
        ThemeMode::Dark,
        FontSizeTier::Standard,
    ));
    assert_eq!(player.library_title(), "Music library");
    assert_eq!(player.track_count(), "1 track", "the head counts what it found");

    // Worded per language, because "1 首" and "1 tracks" are both the kind of thing that makes a
    // finished screen look like it was left half-done.
    player.set_preferences(SystemPreferences::new(
        Language::Chinese,
        ThemeMode::Dark,
        FontSizeTier::Standard,
    ));
    assert_eq!(player.library_title(), "音乐库");
    assert_eq!(player.track_count(), "1 首");
}

/// A tap on a row is the only way onto the playing screen, and it starts the track it names.
///
/// Choosing a track and playing it are one intent, so one tap is all of it: a screen that came up
/// with a stopped disc on it, one more press from the sound that was asked for, would be asking the
/// same question twice.
#[test]
fn a_tap_on_a_row_opens_the_track_and_plays_it() -> Result<(), iced_test::Error> {
    let mut player = player();
    let title = with_one_track(&mut player);

    press(&mut player, &title)?;

    assert_eq!(player.page(), Page::NowPlaying, "the row leads to the track");
    assert_eq!(player.status(), PlaybackStatus::Playing);
    assert_eq!(player.title(), title);
    Ok(())
}

/// Back steps from the track to the list, and only from the list does the press leave the app.
///
/// `go_back`'s answer is what the launcher reads: `true` means the press was the app's to keep.
#[test]
fn back_leaves_the_playing_screen_before_it_leaves_the_app() -> Result<(), iced_test::Error> {
    let mut player = player();
    let title = with_one_track(&mut player);
    press(&mut player, &title)?;

    assert!(
        player.go_back(),
        "the playing screen has the list behind it, so the press is consumed"
    );
    assert_eq!(player.page(), Page::Library);
    assert_eq!(
        player.status(),
        PlaybackStatus::Playing,
        "and going back does not stop the music: choosing the next track is why a person goes back"
    );

    assert!(
        !player.go_back(),
        "the list is where the app stops answering back; the launcher's own back leaves it"
    );
    Ok(())
}

/// The list wears no back button — the press that leaves the app is the launcher's — and the track
/// screen does, because the list is behind it.
#[test]
fn the_back_button_is_on_the_track_and_not_on_the_list() -> Result<(), iced_test::Error> {
    let mut player = player();
    let title = with_one_track(&mut player);

    {
        let mut ui = interface(&player);
        assert!(
            ui.find(Icon::ARROW_BACK.glyph()).is_err(),
            "there is nowhere to go back to from the list"
        );
    }

    press(&mut player, &title)?;

    let mut ui = interface(&player);
    assert!(
        ui.find(Icon::ARROW_BACK.glyph()).is_ok(),
        "the track screen can go back to the list"
    );
    Ok(())
}

#[test]
fn a_press_on_play_starts_the_track() -> Result<(), iced_test::Error> {
    let mut player = player();
    let title = with_one_track(&mut player);

    assert_eq!(player.title(), title);

    // Onto the playing screen first, then hold it: the button the press reaches is the one that
    // resumes a stopped track, which is also what it does from a pause.
    press(&mut player, &title)?;
    press(&mut player, Icon::PAUSE.glyph())?;
    assert_eq!(player.status(), PlaybackStatus::Paused);

    press(&mut player, Icon::PLAY_ARROW.glyph())?;

    assert_eq!(
        player.status(),
        PlaybackStatus::Playing,
        "the press has to reach the button"
    );
    assert!(player.is_animating(), "and a playing track wants frames");
    Ok(())
}

#[test]
fn play_then_pause_stops_asking_for_frames() -> Result<(), iced_test::Error> {
    let mut player = player();
    let title = with_one_track(&mut player);

    press(&mut player, &title)?;
    assert!(player.is_animating());

    // The icon changed with the state, which is what a person reads to know what the button will
    // do next -- so finding the pause glyph is also the assertion that the view followed the model.
    press(&mut player, Icon::PAUSE.glyph())?;

    assert_eq!(player.status(), PlaybackStatus::Paused);
    assert!(
        !player.is_animating(),
        "a paused track is not animating: the disc holds still and the subscription is empty"
    );
    Ok(())
}

/// Moving through the playlist *plays* what it moves to — that is the model's rule, and it is why
/// the two buttons are separate from play/pause rather than being a repeat of it.
#[test]
fn next_and_prev_move_the_playlist_and_play_what_they_land_on() -> Result<(), iced_test::Error> {
    let mut player = player();
    let title = with_one_track(&mut player);

    press(&mut player, &title)?;
    assert_eq!(player.page(), Page::NowPlaying);
    assert_eq!(player.status(), PlaybackStatus::Playing);

    press(&mut player, Icon::SKIP_NEXT.glyph())?;

    assert_eq!(player.status(), PlaybackStatus::Playing);
    assert_eq!(player.title(), "sample", "there is only the one track");
    assert!(player.is_animating());

    press(&mut player, Icon::SKIP_PREVIOUS.glyph())?;

    assert_eq!(player.status(), PlaybackStatus::Playing);
    assert_eq!(player.title(), "sample");
    Ok(())
}

/// The volume buttons are on the track screen, and a press on one steps the level.
///
/// The press is aimed at the glyph, which is the assertion that the two buttons are in the tree at
/// all — a row that only existed in the model would pass every test that asked the model directly.
#[test]
fn the_volume_buttons_step_the_level() -> Result<(), iced_test::Error> {
    let mut player = player();
    let title = with_one_track(&mut player);
    press(&mut player, &title)?;

    let before = player.volume();

    press(&mut player, Icon::VOLUME_DOWN.glyph())?;
    assert_eq!(player.volume(), before.saturating_sub(10));

    press(&mut player, Icon::VOLUME_UP.glyph())?;
    assert_eq!(player.volume(), before, "and the pair are inverse steps");
    Ok(())
}

#[test]
fn the_volume_steps_and_never_leaves_its_range() {
    let mut player = player();

    assert_eq!(
        player.volume(),
        75,
        "the volume is the board's, and 75 is what a board that has never been turned down comes up at"
    );

    player.update(Message::VolumeUp);
    assert_eq!(player.volume(), 85);

    for _ in 0..3 {
        player.update(Message::VolumeDown);
    }
    assert_eq!(player.volume(), 55);

    for _ in 0..20 {
        player.update(Message::VolumeDown);
    }
    assert_eq!(player.volume(), 0, "and never below zero");
}

#[test]
fn the_position_and_duration_are_readable() {
    let mut player = player();
    with_one_track(&mut player);

    assert_eq!(player.position_secs(), 0.0);
    assert!(
        (player.duration_secs() - 3.0).abs() < 0.01,
        "the generated track is three seconds, got {}",
        player.duration_secs()
    );
}

/// The frame messages are the model's clock: a tick hands the model how long the frame took, and
/// the disc turns by that.
///
/// The rate is a *rate* and not a number of degrees per frame — a window draws hundreds of times a
/// second and the panel as fast as it can paint, so a per-frame number is only a speed if you know
/// the frame rate. 200°/s is 33⅓ rpm, which is what a record does.
#[test]
fn the_disc_turns_by_elapsed_time_and_not_by_frame_count() {
    use std::time::{Duration, Instant};

    let mut player = player();
    with_one_track(&mut player);
    player.update(Message::PlayPause);

    let start = Instant::now();

    let before = player.rotation_angle();
    player.update(Message::Tick(start));
    assert_eq!(
        player.rotation_angle(),
        before,
        "the first frame has no predecessor, so it advances by nothing"
    );

    // A tenth of a second: 20 degrees of a record.
    player.update(Message::Tick(start + Duration::from_millis(100)));
    assert!((player.rotation_angle() - 20.0).abs() < 0.001);

    // And the same tenth of a second in two frames is the same turn, which is the whole claim: it
    // is a function of elapsed time and not of how many frames the platform drew.
    player.update(Message::Tick(start + Duration::from_millis(150)));
    player.update(Message::Tick(start + Duration::from_millis(200)));
    assert!((player.rotation_angle() - 40.0).abs() < 0.001);
}

#[test]
fn the_theme_can_be_switched() {
    use pomelo_widgets::preferences::ThemeMode;

    let mut p = player();
    assert_eq!(p.theme_mode(), ThemeMode::Dark);

    p.set_theme_mode(ThemeMode::Light);
    assert_eq!(p.theme_mode(), ThemeMode::Light);

    let dark_theme = {
        let mut pl = player();
        pl.set_theme_mode(ThemeMode::Dark);
        pl.theme()
    };
    let light_theme = p.theme();
    assert_ne!(dark_theme.palette().background, light_theme.palette().background);
}

#[test]
fn preferences_roundtrip() {
    use pomelo_widgets::preferences::{SystemPreferences, ThemeMode};

    let mut p = player();
    let mut prefs = SystemPreferences::default();
    prefs.theme = ThemeMode::Light;
    p.set_preferences(prefs);

    assert_eq!(p.preferences(), prefs);
    assert_eq!(p.theme_mode(), ThemeMode::Light);
}

/// With no card in the slot the player asks for one, in the language on the screen.
#[test]
fn an_empty_slot_asks_for_a_card() {
    use pomelo_widgets::preferences::{FontSizeTier, Language, SystemPreferences, ThemeMode};

    let mut player = Player::new(board_without_card());

    assert!(player.wants_card(), "an empty slot has to be noticed");
    assert_eq!(player.library_dir(), None);

    // Nothing to play, so the title band is where the reason goes — and the reason is the card, not
    // a file that was never there.
    player.refresh_playlist_in(&[]);
    assert_eq!(player.title(), player.empty_notice());

    player.set_preferences(SystemPreferences::new(
        Language::Chinese,
        ThemeMode::Dark,
        FontSizeTier::Standard,
    ));
    assert_eq!(player.empty_notice(), "请插入SD卡");
    assert!(
        player.empty_hint().contains("存储卡"),
        "got {}",
        player.empty_hint()
    );

    player.set_preferences(SystemPreferences::new(
        Language::English,
        ThemeMode::Dark,
        FontSizeTier::Standard,
    ));
    assert_eq!(player.empty_notice(), "Insert an SD card");
    assert!(
        player.empty_hint().contains("card"),
        "got {}",
        player.empty_hint()
    );
}

/// With a card in the slot the music is read from the folder on it, and the player says where that
/// is: the mount point is the board's to name, and the folder inside it is the player's.
#[test]
fn a_card_in_the_slot_is_where_the_music_is_read_from() {
    let player = Player::new(board());

    assert!(!player.wants_card());
    assert_eq!(player.library_dir(), Some("/sdcard/music"));
}

/// A card can arrive while the box is running and the app is not restarted when it does, so the
/// empty screen looks again when it is touched.
#[test]
fn a_tap_on_the_empty_screen_looks_again() -> Result<(), iced_test::Error> {
    let mut player = Player::new(board_without_card());
    player.refresh_playlist_in(&[]);

    let label = player.empty_notice();
    press(&mut player, &label)?;

    assert!(
        player.wants_card(),
        "the slot is still empty, and the player is still saying so"
    );

    Ok(())
}

/// And a scan a caller points somewhere of its own does not answer the card question: the tests
/// above play a generated track, but that is not the card's music and it does not fill the slot.
#[test]
fn a_scan_of_the_callers_own_directory_leaves_the_slot_empty() {
    let dir = scratch_dir();
    write_sample(&dir);
    let dir = dir.to_str().expect("a utf-8 scratch path");

    let mut player = Player::new(board_without_card());
    player.refresh_playlist_in(&[dir]);

    assert_eq!(player.title(), "sample", "the track the caller supplied");
    assert!(player.wants_card(), "the slot is still empty");
    assert_eq!(player.library_dir(), None);
}
