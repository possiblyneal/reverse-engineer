//! The session UI's state model: actions and engine events in, what the window
//! shows out. No window is drawn.

use braintellect_linux::acquisition::stream::StreamStats;
use braintellect_linux::live::{Health, Phase, SessionProperties, Snapshot};
use braintellect_linux::ui::state::{Command, Event, Instrument, Stage, UiModel};

fn ready() -> UiModel {
    UiModel::new("00:11:22:33:44:55".to_owned(), Instrument::Ready)
}

fn snapshot(phase: Phase, health: Health) -> Snapshot {
    Snapshot {
        phase,
        health,
        frames: 500,
        stats: StreamStats {
            packet_counter_errors: 2,
            sample_validation_errors: 1,
        },
        rms_uv: [1.5, 2.5, 3.5, 4.5],
        triggers: 7,
        last_note: Some(52),
        control: 12,
        elapsed_s: 42,
    }
}

#[test]
fn a_session_cannot_start_until_the_instrument_is_imported() {
    let mut model = UiModel::new(
        "00:11:22:33:44:55".to_owned(),
        Instrument::Missing("the instrument bank is missing".to_owned()),
    );

    assert!(!model.view().can_start);
    assert!(model.view().needs_import);
    assert_eq!(model.start(), None);

    model.apply(Event::Imported);
    assert!(model.view().can_start);
    assert!(!model.view().needs_import);
}

#[test]
fn a_failed_import_says_why_and_keeps_the_offer() {
    let mut model = UiModel::new("m".to_owned(), Instrument::Missing("missing".to_owned()));
    model.apply(Event::ImportFailed("no banks in /x".to_owned()));

    assert!(model.view().needs_import);
    assert!(model.view().notice.contains("no banks in /x"));
}

#[test]
fn start_hands_the_properties_to_the_engine_and_stop_is_then_available() {
    let mut model = ready();
    model.properties.duration_s = 120;
    assert!(!model.view().can_stop);

    let command = model.start();

    let expected = SessionProperties {
        duration_s: 120,
        ..SessionProperties::default()
    };
    assert_eq!(command, Some(Command::Start(expected)));
    assert_eq!(model.stage(), &Stage::Connecting);
    assert!(model.view().can_stop);
    assert!(!model.view().can_start);
    assert_eq!(model.start(), None, "no second session while one runs");

    assert_eq!(model.stop(), Some(Command::Stop));
}

#[test]
fn stop_does_nothing_when_no_session_runs() {
    assert_eq!(ready().stop(), None);
}

#[test]
fn a_running_session_shows_the_signal_and_stream_health() {
    let mut model = ready();
    model.start();
    model.apply(Event::Update(snapshot(Phase::Running, Health::Streaming)));

    let view = model.view();
    assert_eq!(model.stage(), &Stage::Running);
    assert_eq!(view.connection, "Streaming");
    let channels: Vec<(&str, &str)> = view
        .channels
        .iter()
        .map(|c| (c.label.as_str(), c.level.as_str()))
        .collect();
    assert_eq!(
        channels,
        [
            ("FP1", "1.5 \u{b5}V"),
            ("FP2", "2.5 \u{b5}V"),
            ("T3", "3.5 \u{b5}V"),
            ("T4", "4.5 \u{b5}V"),
        ]
    );
    assert_eq!(
        view.frames,
        "500 frames, 2 counter errors, 1 validation error"
    );
    assert_eq!(view.feedback, "7 notes, last note 52");
    assert_eq!(view.control, "Control code 12");
    assert_eq!(view.elapsed, "0:42 of 4:00");
}

#[test]
fn a_silent_link_reads_as_no_data() {
    let mut model = ready();
    model.start();
    model.apply(Event::Update(snapshot(Phase::Running, Health::NoData)));
    assert_eq!(model.view().connection, "No data from the B-2");

    model.apply(Event::Update(snapshot(
        Phase::Running,
        Health::WaitingForData,
    )));
    assert_eq!(model.view().connection, "Connected, waiting for data");
}

#[test]
fn a_stored_session_is_confirmed_and_another_can_start() {
    let mut model = ready();
    model.start();
    model.apply(Event::Update(snapshot(
        Phase::Saved { id: 9 },
        Health::Streaming,
    )));

    assert_eq!(model.view().notice, "Session saved (record 9, 0:42).");
    assert!(model.view().can_start);
    assert!(!model.view().can_stop);
}

#[test]
fn a_failed_session_says_why_and_stores_nothing() {
    let mut model = ready();
    model.start();
    model.apply(Event::Failed("cannot connect: host is down".to_owned()));

    assert_eq!(
        model.view().notice,
        "Session not saved: cannot connect: host is down"
    );
    assert!(model.view().can_start);
}

#[test]
fn a_cancelled_connection_returns_to_setup() {
    let mut model = ready();
    model.start();
    assert_eq!(model.stop(), Some(Command::Stop));
    model.apply(Event::Cancelled);

    assert_eq!(model.stage(), &Stage::Setup);
    assert!(model.view().can_start);
}

#[test]
fn settings_that_cannot_run_are_refused_with_a_reason() {
    let mut model = ready();
    model.properties.duration_s = 0;
    assert_eq!(model.start(), None);
    assert_eq!(
        model.view().notice,
        "The session length must be at least 1 second."
    );

    model.properties.duration_s = 60;
    model.properties.high_pass_hz = 200.0;
    assert_eq!(model.start(), None);
    assert!(model.view().notice.contains("high-pass"));
    assert_eq!(model.stage(), &Stage::Setup);
}
