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

    /// Forget the device at `address` and its bond, so a later pairing starts clean.
    ///
    /// # Errors
    /// Fails if the host's Bluetooth cannot be reached.
    fn remove(&mut self, address: &str) -> Result<(), String>;
}

/// Why opening the link to the B-2 failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenError {
    /// The B-2 refused or dropped the link the way a stale bond does
    /// (`device-connection.md`, "Connect symptoms").
    Authentication(String),
    /// Anything else, such as a B-2 that is asleep.
    Other(String),
}

impl OpenError {
    fn into_message(self) -> String {
        match self {
            Self::Authentication(why) | Self::Other(why) => why,
        }
    }
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
    /// The B-2 is paired but the link to it could not be opened, after pairing again
    /// where the first failure looked like a stale bond.
    ConnectFailed(String),
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
            Self::ConnectFailed(why) => format!(
                "Could not connect to the B-2, even after pairing it again: {why}. \
                 Switch it off and on, then try again."
            ),
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

/// Pair the B-2 (at `known`, else found and paired) and open the link with `open`.
/// If opening fails like a stale bond, remove the bond, find, pair and trust the B-2
/// again and open once more; a second failure is reported, never retried.
///
/// # Errors
/// [`PairFailure`] for the pairing, or [`PairFailure::ConnectFailed`] for the link.
pub fn connect_b2<S>(
    host: &mut impl BluetoothHost,
    known: Option<&str>,
    mut open: impl FnMut(&str) -> Result<S, OpenError>,
) -> Result<(String, S), PairFailure> {
    let address = match known {
        Some(address) => address.to_owned(),
        None => try_pair(host)?,
    };
    match open(&address) {
        Ok(stream) => return Ok((address, stream)),
        Err(OpenError::Other(why)) => return Err(PairFailure::ConnectFailed(why)),
        Err(OpenError::Authentication(_)) => {}
    }
    host.remove(&address).map_err(PairFailure::Unavailable)?;
    let address = try_pair(host)?;
    open(&address)
        .map(|stream| (address, stream))
        .map_err(|e| PairFailure::ConnectFailed(e.into_message()))
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
