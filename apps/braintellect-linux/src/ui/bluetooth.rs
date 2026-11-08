//! The Bluetooth-host seam of Pair and connect: what the window needs from the
//! host's Bluetooth stack, and the flow over it. [`super::bluez`] is the only
//! implementation; the flow is tested here with a fake host.

use super::state::Event;

/// The host's Bluetooth stack, as far as finding and pairing the B-2 goes. Errors
/// are the host's own words.
pub trait BluetoothHost {
    /// The address of a B-2 the host has already paired.
    ///
    /// # Errors
    /// Fails if the host's Bluetooth cannot be reached.
    fn paired(&mut self) -> Result<Option<String>, String>;

    /// Look for a B-2 in range and return its address; discovery is over when this returns.
    ///
    /// # Errors
    /// Fails if the host's Bluetooth cannot be reached.
    fn discover(&mut self) -> Result<Option<String>, String>;

    /// Pair with the device at `address`, taking over any other host's bond.
    ///
    /// # Errors
    /// Fails if the device refuses or the pairing cannot complete.
    fn pair(&mut self, address: &str) -> Result<(), String>;

    /// Trust the device at `address` so the host connects to it without asking.
    ///
    /// # Errors
    /// Fails if the host's Bluetooth cannot be reached.
    fn trust(&mut self, address: &str) -> Result<(), String>;
}

/// Why Pair and connect stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairFailure {
    /// No B-2 answered: it is off, asleep or out of range.
    NotFound,
    /// The B-2 was found and pairing with it failed.
    Refused(String),
    /// The host's Bluetooth could not be used.
    Unavailable(String),
}

impl PairFailure {
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::NotFound => {
                "No B-2 found. Switch it on, bring it close and try again.".to_owned()
            }
            Self::Refused(why) => format!("Pairing with the B-2 was refused: {why}"),
            Self::Unavailable(why) => format!("Bluetooth is not available: {why}"),
        }
    }
}

/// Find the B-2, pair it unless the host already has, and trust it. The event is
/// [`Event::Paired`] with its address, or [`Event::PairFailed`].
pub fn pair_b2(host: &mut impl BluetoothHost) -> Event {
    match try_pair(host) {
        Ok(address) => Event::Paired(address),
        Err(failure) => Event::PairFailed(failure),
    }
}

fn try_pair(host: &mut impl BluetoothHost) -> Result<String, PairFailure> {
    let address = match host.paired().map_err(PairFailure::Unavailable)? {
        Some(address) => address,
        None => {
            let address = host
                .discover()
                .map_err(PairFailure::Unavailable)?
                .ok_or(PairFailure::NotFound)?;
            host.pair(&address).map_err(PairFailure::Refused)?;
            address
        }
    };
    host.trust(&address).map_err(PairFailure::Unavailable)?;
    Ok(address)
}
