//! [`BluetoothHost`] over BlueZ's D-Bus API (`org.bluez`, system bus). This is
//! device code like `audio::sink::AlsaSink`: CI never runs it, and the flow it
//! serves is tested in `tests/ui_state.rs` with a fake host.

use std::collections::HashMap;
use std::thread::sleep;
use std::time::{Duration, Instant};

use zbus::blocking::fdo::ObjectManagerProxy;
use zbus::blocking::Connection;
use zbus::names::OwnedInterfaceName;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue};
use zbus::{interface, proxy};

use super::bluetooth::BluetoothHost;

/// The name the B-2 advertises.
const DEVICE_NAME: &str = "B-2";
/// How long discovery waits for the B-2 to answer.
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(15);
const DISCOVERY_POLL: Duration = Duration::from_millis(500);
const AGENT_PATH: &str = "/org/braintellect/agent";
/// Pairs without asking anyone: the B-2 confirms by Secure Simple Pairing
/// (`device-connection.md`).
const AGENT_CAPABILITY: &str = "NoInputNoOutput";

#[proxy(
    interface = "org.bluez.Adapter1",
    default_service = "org.bluez",
    gen_async = false
)]
trait Adapter {
    fn start_discovery(&self) -> zbus::Result<()>;
    fn stop_discovery(&self) -> zbus::Result<()>;
    fn remove_device(&self, device: &ObjectPath<'_>) -> zbus::Result<()>;
    #[zbus(property)]
    fn set_pairable(&self, value: bool) -> zbus::Result<()>;
}

#[proxy(
    interface = "org.bluez.Device1",
    default_service = "org.bluez",
    gen_async = false
)]
trait Device {
    fn pair(&self) -> zbus::Result<()>;
    #[zbus(property)]
    fn set_trusted(&self, value: bool) -> zbus::Result<()>;
}

#[proxy(
    interface = "org.bluez.AgentManager1",
    default_service = "org.bluez",
    default_path = "/org/bluez",
    gen_async = false
)]
trait AgentManager {
    fn register_agent(&self, agent: &ObjectPath<'_>, capability: &str) -> zbus::Result<()>;
    fn request_default_agent(&self, agent: &ObjectPath<'_>) -> zbus::Result<()>;
    fn unregister_agent(&self, agent: &ObjectPath<'_>) -> zbus::Result<()>;
}

/// An `org.bluez.Agent1` that accepts every pairing request.
struct Agent;

#[interface(name = "org.bluez.Agent1")]
impl Agent {
    fn release(&self) {}

    fn cancel(&self) {}

    fn request_confirmation(&self, _device: ObjectPath<'_>, _passkey: u32) {}

    fn request_authorization(&self, _device: ObjectPath<'_>) {}

    fn authorize_service(&self, _device: ObjectPath<'_>, _uuid: String) {}
}

/// One `org.bluez.Device1` as BlueZ lists it.
struct Device {
    path: OwnedObjectPath,
    address: String,
    paired: bool,
    /// BlueZ reports a signal strength only while discovery sees the device.
    in_range: bool,
}

type Interfaces = HashMap<OwnedInterfaceName, HashMap<String, OwnedValue>>;

pub struct Bluez {
    connection: Connection,
}

impl Bluez {
    /// Connect to BlueZ on the system bus.
    ///
    /// # Errors
    /// Fails if there is no system bus.
    pub fn connect() -> Result<Self, String> {
        Ok(Self {
            connection: Connection::system().map_err(text)?,
        })
    }

    fn objects(&self) -> Result<HashMap<OwnedObjectPath, Interfaces>, String> {
        ObjectManagerProxy::builder(&self.connection)
            .destination("org.bluez")
            .map_err(text)?
            .path("/")
            .map_err(text)?
            .build()
            .map_err(text)?
            .get_managed_objects()
            .map_err(text)
    }

    fn adapter_path(&self) -> Result<OwnedObjectPath, String> {
        self.objects()?
            .into_iter()
            .find(|(_, interfaces)| interfaces.contains_key("org.bluez.Adapter1"))
            .map(|(path, _)| path)
            .ok_or_else(|| "no Bluetooth adapter".to_owned())
    }

    fn adapter(&self) -> Result<AdapterProxy<'_>, String> {
        AdapterProxy::new(&self.connection, self.adapter_path()?).map_err(text)
    }

    /// The devices named [`DEVICE_NAME`].
    fn b2_devices(&self) -> Result<Vec<Device>, String> {
        Ok(self
            .objects()?
            .into_iter()
            .filter_map(|(path, interfaces)| {
                let device = interfaces.get("org.bluez.Device1")?;
                let name = string(device.get("Name")?)?;
                (name == DEVICE_NAME).then(|| Device {
                    address: device.get("Address").and_then(string).unwrap_or_default(),
                    paired: device.get("Paired").and_then(flag).unwrap_or(false),
                    in_range: device.contains_key("RSSI"),
                    path,
                })
            })
            .collect())
    }

    fn device(&self, address: &str) -> Result<Device, String> {
        self.b2_devices()?
            .into_iter()
            .find(|d| d.address == address)
            .ok_or_else(|| format!("{address} is no longer known to BlueZ"))
    }

    fn wait_for_b2(&self) -> Result<Option<String>, String> {
        let start = Instant::now();
        while start.elapsed() < DISCOVERY_TIMEOUT {
            if let Some(device) = self.b2_devices()?.into_iter().find(|d| d.in_range) {
                return Ok(Some(device.address));
            }
            sleep(DISCOVERY_POLL);
        }
        Ok(None)
    }
}

impl BluetoothHost for Bluez {
    fn paired(&mut self) -> Result<Option<String>, String> {
        Ok(self
            .b2_devices()?
            .into_iter()
            .find(|d| d.paired)
            .map(|d| d.address))
    }

    fn discover(&mut self) -> Result<Option<String>, String> {
        let adapter = self.adapter()?;
        adapter.set_pairable(true).map_err(text)?;
        adapter.start_discovery().map_err(text)?;
        let found = self.wait_for_b2();
        adapter.stop_discovery().map_err(text)?;
        found
    }

    fn pair(&mut self, address: &str) -> Result<(), String> {
        let device = self.device(address)?;
        self.adapter()?.set_pairable(true).map_err(text)?;
        let agent = ObjectPath::try_from(AGENT_PATH).map_err(text)?;
        self.connection
            .object_server()
            .at(AGENT_PATH, Agent)
            .map_err(text)?;
        let manager = AgentManagerProxy::new(&self.connection).map_err(text)?;
        manager
            .register_agent(&agent, AGENT_CAPABILITY)
            .map_err(text)?;
        let result = manager
            .request_default_agent(&agent)
            .map_err(text)
            .and_then(|()| {
                DeviceProxy::new(&self.connection, device.path)
                    .and_then(|proxy| proxy.pair())
                    .map_err(text)
            });
        // The pairing is over either way; the agent has nothing left to answer.
        let _ = manager.unregister_agent(&agent);
        let _ = self
            .connection
            .object_server()
            .remove::<Agent, _>(AGENT_PATH);
        result
    }

    fn trust(&mut self, address: &str) -> Result<(), String> {
        let device = self.device(address)?;
        DeviceProxy::new(&self.connection, device.path)
            .and_then(|proxy| proxy.set_trusted(true))
            .map_err(text)
    }

    fn remove(&mut self, address: &str) -> Result<(), String> {
        let device = self.device(address)?;
        self.adapter()?.remove_device(&device.path).map_err(text)
    }
}

/// BlueZ's error text without the D-Bus wrapper's type name.
fn text(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn string(value: &OwnedValue) -> Option<String> {
    String::try_from(value.clone()).ok()
}

fn flag(value: &OwnedValue) -> Option<bool> {
    bool::try_from(value).ok()
}
