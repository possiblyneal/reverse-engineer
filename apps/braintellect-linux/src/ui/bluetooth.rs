//! The Bluetooth-host seam of Pair and connect: what the window needs from the
//! host's Bluetooth stack, and the flow over it. [`super::bluez`] is the only
//! implementation; the flow is tested here with a fake host.

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
    /// A device the host does not know is already forgotten.
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
    /// The B-2 is paired but the link to it could not be opened: it is off, asleep
    /// or out of range.
    ConnectFailed(String),
    /// The link looked like a stale bond, so the bond was removed, and pairing again
    /// failed: the host no longer has a bond, and Pair and connect is needed.
    BondLost(Box<PairFailure>),
    /// The link failed like a stale bond, the B-2 was paired again, and the link
    /// still could not be opened.
    ConnectFailedAfterRepair(String),
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
                "Could not connect to the B-2: {why}. Check it is switched on and close, \
                 then try again."
            ),
            Self::BondLost(why) => format!(
                "The B-2's pairing was out of date and was removed, and pairing again \
                 failed: {}. Switch the B-2 on and press Pair and connect.",
                why.message().trim_end_matches('.')
            ),
            Self::ConnectFailedAfterRepair(why) => format!(
                "Could not connect to the B-2, even after pairing it again: {why}. \
                 Check it is switched on and close, then try again."
            ),
        }
    }
}

/// Pair the B-2 (at `known`, else found and paired) and open the link with `open`.
/// If opening fails like a stale bond, remove the bond, find, pair and trust the B-2
/// again and open once more; a second failure is reported, never retried.
///
/// # Errors
/// [`PairFailure`] for the pairing; [`PairFailure::ConnectFailed`] for a link that
/// failed without looking like a stale bond; [`PairFailure::BondLost`] when the bond
/// was removed and pairing again failed; [`PairFailure::ConnectFailedAfterRepair`]
/// for the link after pairing again.
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
    let address = try_pair(host).map_err(|failure| PairFailure::BondLost(Box::new(failure)))?;
    open(&address)
        .map(|stream| (address, stream))
        .map_err(|e| PairFailure::ConnectFailedAfterRepair(e.into_message()))
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

/// Whether a connect that failed with `e` still reached the B-2. Only a page that
/// went unanswered (`EHOSTDOWN` after about 5 s, measured with the B-2 off) means it
/// is off or out of range; a refusal or a key error comes from the device.
#[must_use]
pub fn answered(e: &std::io::Error) -> bool {
    !matches!(
        e.raw_os_error(),
        Some(libc::EHOSTDOWN | libc::EHOSTUNREACH | libc::ETIMEDOUT)
    )
}
