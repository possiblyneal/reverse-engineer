//! Sensor status ("Ready" / "Adjust Fit" / "Not Available") through the parity
//! harness: synthetic level streams drive each fault test (`sensor-status.md`).

mod harness;

use braintellect_linux::sensor::SensorStatus::{AdjustFit, NotAvailable, Ready};
use harness::sensor;

#[test]
fn healthy_levels_read_ready() {
    let steps = sensor::run(true, &[(0, sensor::healthy()), (500, sensor::healthy())]);
    sensor::expect(&steps, &[Ready, Ready]).unwrap();
}

#[test]
fn engine_off_reads_not_available() {
    let steps = sensor::run(false, &[(0, sensor::faulty_outlier())]);
    sensor::expect(&steps, &[NotAvailable]).unwrap();
}

#[test]
fn each_fault_test_reaches_adjust_fit_after_the_delay_and_recovers() {
    for faulty in [
        sensor::faulty_outlier(),
        sensor::faulty_ceiling(),
        sensor::faulty_pair(),
    ] {
        let steps = sensor::run(
            true,
            &[
                (0, sensor::healthy()),
                (100, faulty.clone()),
                (1099, faulty.clone()),
                (1100, faulty.clone()),
                (1200, sensor::healthy()),
                (1300, faulty.clone()),
                (1400, faulty),
            ],
        );
        sensor::expect(
            &steps,
            &[Ready, Ready, Ready, AdjustFit, Ready, Ready, Ready],
        )
        .unwrap();
    }
}
