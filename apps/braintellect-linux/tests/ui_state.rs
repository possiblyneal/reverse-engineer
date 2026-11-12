//! The session UI's state model: actions and engine events in, what the window
//! shows out. No window is drawn.

use braintellect_linux::acquisition::stream::StreamStats;
use braintellect_linux::audio::tone::{Availability, Tone, DEFAULT_TONE, TONES};
use braintellect_linux::live::{Health, Phase, SessionProperties, Snapshot};
use braintellect_linux::ui::bluetooth::{connect_b2, BluetoothHost, OpenError, PairFailure};
use braintellect_linux::ui::state::{Command, Event, Stage, UiModel};

fn ready() -> UiModel {
    UiModel::new("00:11:22:33:44:55".to_owned(), Availability::Ready)
}

fn snapshot(phase: Phase, health: Health) -> Snapshot {
    Snapshot {
        phase,
        health,
        frames: 500,
        stats: StreamStats {
            packet_counter_errors: 2,
            sample_validation_errors: 1,
            lost_frames: 3,
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
        Availability::Missing("the instrument bank is missing".to_owned()),
    );

    assert!(!model.view().can_start);
    assert!(model.view().needs_import);
    assert_eq!(model.start(), None);

    model.apply(Event::Imported);
    assert!(model.view().can_start);
    assert!(!model.view().needs_import);
}

#[test]
fn the_sensors_can_be_checked_before_any_tone_is_imported() {
    let mut model = UiModel::new("m".to_owned(), Availability::Missing("missing".to_owned()));

    assert!(model.view().can_check);
    assert_eq!(model.check_sensors(), Some(Command::CheckSensors));
}

#[test]
fn a_failed_import_says_why_and_keeps_the_offer() {
    let mut model = UiModel::new("m".to_owned(), Availability::Missing("missing".to_owned()));
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

#[test]
fn the_sensor_status_shows_before_a_session_and_is_not_available_while_one_runs() {
    use braintellect_linux::fit::SensorFit::{Good, NotTouching};

    let mut model = ready();
    assert_eq!(model.view().sensor, "Not Available");

    model.apply(Event::Sensor([Good, Good, Good, NotTouching]));
    assert_eq!(
        model.view().sensor,
        "FP1 good, FP2 good, T3 good, T4 not touching"
    );
    model.apply(Event::Sensor([Good; 4]));
    assert_eq!(model.view().sensor, "FP1 good, FP2 good, T3 good, T4 good");

    model.start().expect("starts");
    assert_eq!(model.view().sensor, "Not Available");
    model.apply(Event::Update(snapshot(Phase::Running, Health::Streaming)));
    assert_eq!(model.view().sensor, "Not Available");

    // A status that arrives during the run does not show until the run ends.
    model.apply(Event::Sensor([Good; 4]));
    assert_eq!(model.view().sensor, "Not Available");
    model.apply(Event::Update(snapshot(
        Phase::Saved { id: 1 },
        Health::Streaming,
    )));
    assert_eq!(model.view().sensor, "FP1 good, FP2 good, T3 good, T4 good");
}

#[test]
fn the_sensor_check_warns_while_the_link_loses_frames() {
    use braintellect_linux::fit::SensorFit::Good;

    let mut model = ready();
    model.check_sensors();
    model.apply(Event::Sensor([Good; 4]));
    model.apply(Event::LinkLoss(0));
    assert_eq!(model.view().connection, "Connected, checking sensors");
    model.apply(Event::LinkLoss(1));
    assert_eq!(model.view().connection, "Bluetooth link losing 1% of data");
    model.apply(Event::LinkLoss(42));
    assert_eq!(model.view().connection, "Bluetooth link losing 42% of data");
    model.apply(Event::LinkLoss(0));
    assert_eq!(model.view().connection, "Connected, checking sensors");

    // The figure belongs to that check; the next one starts clean.
    model.apply(Event::LinkLoss(42));
    model.apply(Event::SensorCheckEnded(None));
    assert_eq!(model.view().connection, "Checking whether the B-2 is on");
    model.check_sensors();
    model.apply(Event::Sensor([Good; 4]));
    assert_eq!(model.view().connection, "Connected, checking sensors");
}

#[test]
fn the_sensor_check_runs_before_a_session_and_ends_on_request_or_failure() {
    use braintellect_linux::fit::SensorFit::Good;

    let mut model = ready();
    assert!(model.view().can_check);
    assert!(!model.view().checking);

    assert_eq!(model.check_sensors(), Some(Command::CheckSensors));
    assert!(model.view().checking);
    assert!(!model.view().can_check);
    assert_eq!(model.check_sensors(), None);
    assert_eq!(model.view().connection, "Connecting to 00:11:22:33:44:55");
    model.apply(Event::Sensor([Good; 4]));
    assert_eq!(model.view().sensor, "FP1 good, FP2 good, T3 good, T4 good");
    assert_eq!(model.view().connection, "Connected, checking sensors");

    assert_eq!(model.stop_check(), Some(Command::StopCheck));
    model.apply(Event::SensorCheckEnded(None));
    assert!(!model.view().checking);
    assert_eq!(model.view().connection, "Checking whether the B-2 is on");
    assert_eq!(model.view().sensor, "Not Available");
    assert_eq!(model.stop_check(), None);

    // A check that cannot connect says why and leaves the status unavailable.
    model.check_sensors();
    model.apply(Event::SensorCheckEnded(Some("cannot connect".to_owned())));
    assert!(!model.view().checking);
    assert!(model.view().notice.contains("cannot connect"));

    // No check during a session.
    model.start().expect("starts");
    assert!(!model.view().can_check);
    assert_eq!(model.check_sensors(), None);
}

fn tone_key(key: &str) -> &'static Tone {
    TONES.iter().find(|t| t.key == key).expect("tone")
}

/// Only the default tone and the harp are imported.
fn with_harp() -> UiModel {
    let mut model = ready();
    model.set_tones(
        TONES
            .iter()
            .map(|t| match t.key {
                "strings" | "harp" => Availability::Ready,
                _ => Availability::Missing(format!("{} is not imported", t.label)),
            })
            .collect(),
    );
    model
}

#[test]
fn the_setup_screen_lists_every_tone_and_marks_the_missing_ones_unavailable() {
    let view = with_harp().view();

    let listed: Vec<(&str, bool, bool)> = view
        .tones
        .iter()
        .map(|t| (t.label.as_str(), t.available, t.selected))
        .collect();
    assert_eq!(
        listed,
        [
            ("Acoustic strings (original default)", true, true),
            ("Piano", false, false),
            ("Celeste", false, false),
            ("Harp", true, false),
            ("Acoustic guitar", false, false),
            ("Xylophone", false, false),
        ]
    );
    assert_eq!(view.tones[1].status, "Piano is not imported");
    assert!(view.can_start, "missing tones do not stop a session");
}

#[test]
fn the_default_tone_is_the_original_bank() {
    assert_eq!(ready().tone(), &DEFAULT_TONE);
}

#[test]
fn an_available_tone_can_be_chosen_and_an_unavailable_one_is_refused() {
    let mut model = with_harp();

    assert!(model.select_tone(tone_key("harp")));
    assert_eq!(model.tone().key, "harp");
    assert!(model.view().tones[3].selected);

    assert!(!model.select_tone(tone_key("xylophone")));
    assert_eq!(model.tone().key, "harp");
}

#[test]
fn a_session_can_start_on_a_recorded_tone_while_the_original_bank_is_missing() {
    let mut model = UiModel::new("m".to_owned(), Availability::Missing("missing".to_owned()));
    let mut states = vec![Availability::Missing("missing".to_owned()); TONES.len()];
    states[3] = Availability::Ready;
    model.set_tones(states);

    assert!(!model.view().can_start);
    assert!(model.select_tone(tone_key("harp")));
    assert!(model.view().can_start);
    assert!(matches!(model.start(), Some(Command::Start(_))));
}

#[test]
fn the_tone_cannot_change_while_a_session_runs() {
    let mut model = with_harp();
    model.start();

    assert!(!model.select_tone(tone_key("harp")));
    assert_eq!(model.tone(), &DEFAULT_TONE);
}

fn unpaired() -> UiModel {
    UiModel::without_address(Availability::Ready)
}

#[test]
fn without_an_address_the_window_offers_pair_and_connect_and_a_known_address_does_not() {
    let model = unpaired();
    assert!(model.view().can_pair);
    assert_eq!(model.view().address, "");
    assert!(!model.view().can_check, "nothing to check without a B-2");
    assert!(!model.view().can_start);
    assert_eq!(model.view().connection, "No B-2 paired");

    let known = ready();
    assert!(!known.view().can_pair);
    assert_eq!(known.view().address, "00:11:22:33:44:55");
    assert_eq!(known.view().connection, "Checking whether the B-2 is on");
}

#[test]
fn pair_and_connect_pairs_then_checks_the_sensors_at_the_found_address() {
    let mut model = unpaired();

    assert_eq!(model.pair_and_connect(), Some(Command::PairAndConnect));
    assert_eq!(model.stage(), &Stage::Pairing);
    assert!(!model.view().can_pair, "no second pairing while one runs");
    assert_eq!(model.pair_and_connect(), None);

    model.apply(Event::Paired("AA:BB:CC:DD:EE:FF".to_owned()));

    assert_eq!(model.stage(), &Stage::Setup);
    assert_eq!(model.view().address, "AA:BB:CC:DD:EE:FF");
    assert!(!model.view().can_pair);
    assert!(
        model.view().checking,
        "the sensor check follows the pairing"
    );
    assert_eq!(model.address(), Some("AA:BB:CC:DD:EE:FF"));
}

#[test]
fn a_b2_that_is_not_found_says_so_and_can_be_retried() {
    let mut model = unpaired();
    model.pair_and_connect();

    model.apply(Event::PairFailed(PairFailure::NotFound));

    assert_eq!(model.stage(), &Stage::Setup);
    assert!(model.view().notice.contains("No B-2 found"));
    assert!(model.view().can_pair);
    assert_eq!(model.pair_and_connect(), Some(Command::PairAndConnect));
}

#[test]
fn a_refused_pairing_says_why_and_can_be_retried() {
    let mut model = unpaired();
    model.pair_and_connect();

    model.apply(Event::PairFailed(PairFailure::Refused(
        "Authentication Rejected".to_owned(),
    )));

    assert!(model.view().notice.contains("refused"));
    assert!(model.view().notice.contains("Authentication Rejected"));
    assert!(model.view().can_pair);
}

/// A scripted Bluetooth host that records the calls the flow makes.
struct FakeHost {
    paired: Result<Option<String>, String>,
    found: Result<Option<String>, String>,
    pair: Result<(), String>,
    trust: Result<(), String>,
    calls: Vec<String>,
}

impl FakeHost {
    fn new() -> Self {
        Self {
            paired: Ok(None),
            found: Ok(Some(B2.to_owned())),
            pair: Ok(()),
            trust: Ok(()),
            calls: Vec::new(),
        }
    }
}

const B2: &str = "AA:BB:CC:DD:EE:FF";

impl BluetoothHost for FakeHost {
    fn paired(&mut self) -> Result<Option<String>, String> {
        self.calls.push("paired".to_owned());
        self.paired.clone()
    }
    fn discover(&mut self) -> Result<Option<String>, String> {
        self.calls.push("discover".to_owned());
        self.found.clone()
    }
    fn pair(&mut self, address: &str) -> Result<(), String> {
        self.calls.push(format!("pair {address}"));
        self.pair.clone()
    }
    fn trust(&mut self, address: &str) -> Result<(), String> {
        self.calls.push(format!("trust {address}"));
        self.trust.clone()
    }
    fn remove(&mut self, address: &str) -> Result<(), String> {
        self.calls.push(format!("remove {address}"));
        self.paired = Ok(None);
        Ok(())
    }
}

/// A connect that fails with `failures` in turn, then succeeds; it counts its tries.
fn scripted_open(
    failures: Vec<OpenError>,
    tries: &mut usize,
) -> impl FnMut(&str) -> Result<&'static str, OpenError> + '_ {
    let mut failures = failures.into_iter();
    move |_| {
        *tries += 1;
        failures.next().map_or(Ok("stream"), Err)
    }
}

fn stale() -> OpenError {
    OpenError::Authentication("Connection refused".to_owned())
}

#[test]
fn a_stale_bond_is_removed_then_the_b2_is_paired_again_and_connected() {
    let mut host = FakeHost::new();
    host.paired = Ok(Some(B2.to_owned()));
    let mut tries = 0;

    let result = connect_b2(&mut host, None, scripted_open(vec![stale()], &mut tries));

    assert_eq!(result, Ok((B2.to_owned(), "stream")));
    assert_eq!(tries, 2);
    assert_eq!(
        host.calls,
        [
            "paired".to_owned(),
            format!("trust {B2}"),
            format!("remove {B2}"),
            "paired".to_owned(),
            "discover".to_owned(),
            format!("pair {B2}"),
            format!("trust {B2}"),
        ]
    );
}

#[test]
fn a_known_address_that_fails_to_connect_is_repaired_the_same_way() {
    let mut host = FakeHost::new();
    host.paired = Ok(Some(B2.to_owned()));
    let mut tries = 0;

    let result = connect_b2(
        &mut host,
        Some(B2),
        scripted_open(vec![stale()], &mut tries),
    );

    assert_eq!(result, Ok((B2.to_owned(), "stream")));
    assert_eq!(host.calls[0], format!("remove {B2}"));
    assert!(host.calls.contains(&format!("pair {B2}")));
}

#[test]
fn a_second_failure_is_reported_and_not_retried() {
    let mut host = FakeHost::new();
    let mut tries = 0;

    let result = connect_b2(
        &mut host,
        Some(B2),
        scripted_open(vec![stale(), stale(), stale()], &mut tries),
    );

    assert_eq!(
        result,
        Err(PairFailure::ConnectFailedAfterRepair(
            "Connection refused".to_owned()
        ))
    );
    assert_eq!(tries, 2, "one retry, no loop");
    assert_eq!(
        host.calls
            .iter()
            .filter(|c| c.starts_with("remove"))
            .count(),
        1
    );
}

#[test]
fn a_failure_that_is_not_authentication_leaves_the_bond_alone() {
    let mut host = FakeHost::new();
    let mut tries = 0;

    let result = connect_b2(
        &mut host,
        Some(B2),
        scripted_open(
            vec![OpenError::Other("Host is down".to_owned())],
            &mut tries,
        ),
    );

    assert_eq!(
        result,
        Err(PairFailure::ConnectFailed("Host is down".to_owned()))
    );
    assert_eq!(tries, 1);
    assert!(host.calls.is_empty());
}

#[test]
fn a_connect_that_works_pairs_once_and_removes_nothing() {
    let mut host = FakeHost::new();
    let mut tries = 0;

    let result = connect_b2(&mut host, None, scripted_open(vec![], &mut tries));

    assert_eq!(result, Ok((B2.to_owned(), "stream")));
    assert_eq!(tries, 1);
    assert!(!host.calls.iter().any(|c| c.starts_with("remove")));
}

#[test]
fn a_failed_reconnect_tells_the_operator_what_to_do() {
    let message = PairFailure::ConnectFailedAfterRepair("Connection refused".to_owned()).message();

    assert!(message.contains("Connection refused"));
    assert!(message.contains("pairing it again"), "{message}");
}

#[test]
fn a_first_connect_failure_does_not_claim_a_repair() {
    let message = PairFailure::ConnectFailed("Host is down".to_owned()).message();

    assert!(message.contains("Host is down"));
    assert!(!message.contains("pairing it again"), "{message}");
}

#[test]
fn a_stale_bond_that_cannot_be_paired_again_is_a_lost_bond() {
    let mut host = FakeHost::new();
    host.found = Ok(None);
    let mut tries = 0;

    let result = connect_b2(
        &mut host,
        Some(B2),
        scripted_open(vec![stale()], &mut tries),
    );

    assert_eq!(
        result,
        Err(PairFailure::BondLost(Box::new(PairFailure::NotFound)))
    );
    assert_eq!(tries, 1);
}

#[test]
fn a_lost_bond_message_reads_as_one_sentence_per_instruction() {
    let message = PairFailure::BondLost(Box::new(PairFailure::NotFound)).message();

    assert!(!message.contains(".."), "{message}");
    assert!(message.ends_with("press Pair and connect."), "{message}");
}

#[test]
fn a_lost_bond_forgets_the_address_and_offers_pairing() {
    let mut model = UiModel::new(B2.to_owned(), Availability::Ready);

    model.apply(Event::PairFailed(PairFailure::BondLost(Box::new(
        PairFailure::NotFound,
    ))));

    assert_eq!(model.address(), None);
    assert!(model.view().can_pair);
}

#[test]
fn another_pair_failure_keeps_the_address() {
    let mut model = UiModel::new(B2.to_owned(), Availability::Ready);

    model.apply(Event::PairFailed(PairFailure::NotFound));

    assert_eq!(model.address(), Some(B2));
}

/// Pair and connect with a link that opens at once: only the pairing is under test.
fn pair(host: &mut FakeHost) -> Result<String, PairFailure> {
    connect_b2(host, None, |_| Ok(())).map(|(address, ())| address)
}

#[test]
fn a_found_b2_is_paired_trusted_and_reported() {
    let mut host = FakeHost::new();

    assert_eq!(pair(&mut host), Ok(B2.to_owned()));
    assert_eq!(
        host.calls,
        [
            "paired",
            "discover",
            &format!("pair {B2}"),
            &format!("trust {B2}")
        ]
    );
}

#[test]
fn an_already_paired_b2_is_trusted_without_pairing_again() {
    let mut host = FakeHost::new();
    host.paired = Ok(Some(B2.to_owned()));

    assert_eq!(pair(&mut host), Ok(B2.to_owned()));
    assert_eq!(host.calls, ["paired", &format!("trust {B2}")]);
}

#[test]
fn no_b2_in_range_is_reported_as_not_found() {
    let mut host = FakeHost::new();
    host.found = Ok(None);

    assert_eq!(pair(&mut host), Err(PairFailure::NotFound));
    assert_eq!(host.calls, ["paired", "discover"]);
}

#[test]
fn a_refused_pairing_is_reported_with_the_reason_and_not_trusted() {
    let mut host = FakeHost::new();
    host.pair = Err("Authentication Rejected".to_owned());

    assert_eq!(
        pair(&mut host),
        Err(PairFailure::Refused("Authentication Rejected".to_owned()))
    );
    assert!(!host.calls.iter().any(|c| c.starts_with("trust")));
}

#[test]
fn a_bluetooth_host_that_cannot_be_reached_is_reported_as_unavailable() {
    let mut host = FakeHost::new();
    host.paired = Err("no adapter".to_owned());

    assert_eq!(
        pair(&mut host),
        Err(PairFailure::Unavailable("no adapter".to_owned()))
    );
}

#[test]
fn a_failed_preview_says_why() {
    let mut model = ready();
    model.apply(Event::PreviewFailed("no audio output".to_owned()));

    assert!(model.view().notice.contains("no audio output"));
}

#[test]
fn the_idle_status_says_whether_the_b2_answered_the_probe() {
    let mut model = ready();
    assert_eq!(model.view().connection, "Checking whether the B-2 is on");
    model.apply(Event::Presence(false));
    assert_eq!(model.view().connection, "B-2 off or out of range");
    model.apply(Event::Presence(true));
    assert_eq!(model.view().connection, "B-2 on, ready to connect");
}

#[test]
fn only_an_answer_from_the_device_means_the_b2_is_on() {
    use braintellect_linux::ui::bluetooth::answered;
    use std::io::Error;

    assert!(!answered(&Error::from_raw_os_error(libc::EHOSTDOWN)));
    assert!(!answered(&Error::from_raw_os_error(libc::ENETDOWN)));
    assert!(answered(&Error::from_raw_os_error(libc::ECONNREFUSED)));
    assert!(answered(&Error::from_raw_os_error(libc::EBADE)));
}
