//! The clinician-facing session window. [`state`] is the pure model (actions and
//! engine events in, what the window shows out) and is tested headlessly;
//! [`worker`] runs a session on its own thread; [`app`] only draws the model and
//! turns clicks into model calls.

pub mod app;
pub mod bluetooth;
pub mod bluez;
pub mod history;
pub mod state;
pub mod worker;
