//! Reports the clone's Circle-Balance chart values over one original exercise's stored LMH
//! series. Nothing is asserted: no capture of the original's chart inputs exists yet
//! (`docs/reverse-engineering/braintellect2/audio-feedback.md`, "Circle-Balance design").
//!
//! `S62_LMH` names the TSV `scripts/lmh-extract.py` writes from a decoded type-9 XML; it is
//! vendor-derived and stays under `tmp/`.

use std::fs;

use braintellect_linux::feedback::circle_balance::{CircleBalanceDesign, Lmh, LmhBand, LmhPoint};

#[test]
#[ignore = "needs S62_LMH, a stored exercise's LMH series under tmp/"]
fn report_circle_balance_over_a_stored_exercise() {
    let path = std::env::var("S62_LMH").expect("S62_LMH");
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut duration_s = None;
    let mut lmh = Lmh::default();
    for line in text.lines() {
        let c: Vec<&str> = line.split('\t').collect();
        if c[0] == "duration" {
            duration_s = Some(c[1].parse().expect("duration"));
            continue;
        }
        let channel: usize = c[0].parse().expect("channel");
        let band = match c[1] {
            "High" => LmhBand::High,
            "Low" => LmhBand::Low,
            other => panic!("band {other}"),
        };
        let point = LmhPoint {
            x_ms: c[2].parse().expect("x"),
            y: c[3].parse::<f64>().expect("y") as f32,
        };
        lmh.series_mut(channel, band).push(point);
    }
    let duration_s = duration_s.expect("duration row");
    let design = CircleBalanceDesign::circle_balance();
    for (input, value) in design.inputs.iter().zip(design.report(&lmh, duration_s)) {
        println!(
            "{:?} {:?} {:?} {:?} ch{}n{}: {value:?}",
            input.meter, input.graph, input.axis, input.band, input.channels.0, input.channels.1
        );
    }
}
