use std::time::Duration;

use astarte_device_sdk::{
    event::FromEventError, types::TypeError, AstarteType, DeviceEvent, FromEvent,
};
use log::{error, warn};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeEvent {
    Ota(OtaRequest),
    Command(Commands),
    Telemetry(TelemetryEvent),
    Led(LedEvent),
    #[cfg(feature = "forwarder")]
    Session(edgehog_forwarder::astarte::SessionInfo),
}

impl FromEvent for RuntimeEvent {
    type Err = FromEventError;

    fn from_event(event: DeviceEvent) -> Result<Self, Self::Err> {
        match event.interface.as_str() {
            "io.edgehog.devicemanager.OTARequest" => {
                OtaRequest::from_event(event).map(RuntimeEvent::Ota)
            }
            "io.edgehog.devicemanager.Commands" => {
                Commands::from_event(event).map(RuntimeEvent::Command)
            }
            "io.edgehog.devicemanager.config.Telemetry" => {
                TelemetryEvent::from_event(event).map(RuntimeEvent::Telemetry)
            }
            "io.edgehog.devicemanager.LedBehavior" => {
                LedEvent::from_event(event).map(RuntimeEvent::Led)
            }
            #[cfg(feature = "forwarder")]
            "io.edgehog.devicemanager.ForwarderSessionRequest" => {
                edgehog_forwarder::astarte::SessionInfo::from_event(event)
                    .map(RuntimeEvent::Session)
            }
            _ => Err(FromEventError::Interface(event.interface)),
        }
    }
}

#[derive(Debug, Clone, FromEvent, PartialEq, Eq)]
#[from_event(interface = "io.edgehog.devicemanager.OTARequest", path = "/request")]
pub struct OtaRequest {
    pub operation: OtaOperation,
    pub url: String,
    pub uuid: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OtaOperation {
    Update,
    Cancel,
}

impl TryFrom<AstarteType> for OtaOperation {
    type Error = TypeError;

    fn try_from(value: AstarteType) -> Result<Self, Self::Error> {
        let value = String::try_from(value)?;

        match value.as_str() {
            "Update" => Ok(Self::Update),
            "Cancel" => Ok(Self::Cancel),
            _ => {
                error!("unrecognize Ota operation value {value}");

                Err(TypeError::Conversion)
            }
        }
    }
}

#[derive(Debug, Clone, FromEvent, PartialEq, Eq)]
#[from_event(
    interface = "io.edgehog.devicemanager.Commands",
    aggregation = "individual"
)]
pub enum Commands {
    #[mapping(endpoint = "/request")]
    Request(CmdReq),
}

impl Commands {
    /// Returns `true` if the cmd req is [`Reboot`].
    ///
    /// [`Reboot`]: CmdReq::Reboot
    #[must_use]
    pub fn is_reboot(&self) -> bool {
        matches!(self, Self::Request(CmdReq::Reboot))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CmdReq {
    Reboot,
}

impl CmdReq {
    /// Returns `true` if the cmd req is [`Reboot`].
    ///
    /// [`Reboot`]: CmdReq::Reboot
    #[must_use]
    pub fn is_reboot(&self) -> bool {
        matches!(self, Self::Reboot)
    }
}

impl TryFrom<AstarteType> for CmdReq {
    type Error = TypeError;

    fn try_from(value: AstarteType) -> Result<Self, Self::Error> {
        let value = String::try_from(value)?;

        match value.as_str() {
            "Reboot" => Ok(CmdReq::Reboot),
            _ => {
                error!("unrecognize Commands request value {value}");

                Err(TypeError::Conversion)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelemetryEvent {
    pub interface: String,
    pub config: TelemetryConfig,
}

impl FromEvent for TelemetryEvent {
    type Err = FromEventError;

    fn from_event(event: DeviceEvent) -> Result<Self, Self::Err> {
        let interface = TelemetryConfig::interface_from_path(&event.path).ok_or_else(|| {
            FromEventError::Path {
                interface: "io.edgehog.devicemanager.config.Telemetry",
                base_path: event.path.clone(),
            }
        })?;

        TelemetryConfig::from_event(event).map(|config| TelemetryEvent { interface, config })
    }
}

#[derive(Debug, Clone, FromEvent, PartialEq, Eq)]
#[from_event(
    interface = "io.edgehog.devicemanager.config.Telemetry",
    aggregation = "individual"
)]
pub enum TelemetryConfig {
    #[mapping(endpoint = "/request/%{interface_name}/enable", allow_unset = true)]
    Enable(Option<bool>),
    #[mapping(
        endpoint = "/request/%{interface_name}/periodSeconds",
        allow_unset = true
    )]
    Period(Option<TelemetryPeriod>),
}

impl TelemetryConfig {
    fn interface_from_path(path: &str) -> Option<String> {
        path.strip_prefix('/')
            .and_then(|path| path.splitn(3, '/').skip(1).next())
            .map(str::to_string)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelemetryPeriod(pub Duration);

impl TryFrom<AstarteType> for TelemetryPeriod {
    type Error = TypeError;

    fn try_from(value: AstarteType) -> Result<Self, Self::Error> {
        let secs = i64::try_from(value).map(|i| match u64::try_from(i) {
            Ok(secs) => secs,
            Err(_) => {
                warn!("Telemetry period seconds value too big {i}, capping to u64::MAX");

                u64::MAX
            }
        })?;

        Ok(Self(Duration::from_secs(secs)))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedEvent {
    pub led_id: String,
    pub behavior: LedBehavior,
}

impl FromEvent for LedEvent {
    type Err = FromEventError;

    fn from_event(event: DeviceEvent) -> Result<Self, Self::Err> {
        let led_id =
            LedBehavior::led_id_from_path(&event.path).ok_or_else(|| FromEventError::Path {
                interface: "io.edgehog.devicemanager.LedBehavior",
                base_path: event.path.clone(),
            })?;

        LedBehavior::from_event(event).map(|behavior| LedEvent { led_id, behavior })
    }
}

#[derive(Debug, Clone, FromEvent, PartialEq, Eq)]
#[from_event(
    interface = "io.edgehog.devicemanager.LedBehavior",
    aggregation = "individual"
)]
pub enum LedBehavior {
    #[mapping(endpoint = "/%{led_id}/behavior")]
    Behavior(Blink),
}

impl LedBehavior {
    fn led_id_from_path(path: &str) -> Option<String> {
        path.strip_prefix('/')
            .and_then(|path| path.split_once('/').map(|(led_id, _)| led_id))
            .map(str::to_string)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Blink {
    Single,
    Double,
    Slow,
}

impl TryFrom<AstarteType> for Blink {
    type Error = TypeError;

    fn try_from(value: AstarteType) -> Result<Self, Self::Error> {
        let value = String::try_from(value)?;

        match value.as_str() {
            "Blink60Seconds" => Ok(Self::Single),
            "DoubleBlink60Seconds" => Ok(Self::Double),
            "SlowBlink60Seconds" => Ok(Self::Slow),
            _ => {
                error!("unrecognize LedBehavior behavior value {value}");

                Err(TypeError::Conversion)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use astarte_device_sdk::{DeviceEvent, Value};

    #[test]
    fn should_convert_ota_from_event() {
        let operation = "Update";
        let url = "http://example.com";
        let uuid = "04bf491c-af94-4e9d-813f-ebeebfb856a6";

        let mut data = HashMap::new();
        data.insert("operation".to_string(), operation.into());
        data.insert("url".to_string(), url.into());
        data.insert("uuid".to_string(), uuid.into());

        let event = DeviceEvent {
            interface: "io.edgehog.devicemanager.OTARequest".to_string(),
            path: "/request".to_string(),
            data: Value::Object(data),
        };

        let res = RuntimeEvent::from_event(event).unwrap();

        assert_eq!(
            res,
            RuntimeEvent::Ota(OtaRequest {
                operation: OtaOperation::Update,
                url: url.to_string(),
                uuid: uuid.to_string(),
            })
        );
    }

    #[test]
    fn shold_convert_command_from_event() {
        let event = DeviceEvent {
            interface: "io.edgehog.devicemanager.Commands".to_string(),
            path: "/request".to_string(),
            data: Value::Individual("Reboot".into()),
        };

        let res = RuntimeEvent::from_event(event).unwrap();

        assert_eq!(
            res,
            RuntimeEvent::Command(Commands::Request(CmdReq::Reboot))
        );
    }

    #[test]
    fn shold_convert_telemetry_from_event() {
        let event = DeviceEvent {
            interface: "io.edgehog.devicemanager.config.Telemetry".to_string(),
            path: "/request/foo/enable".to_string(),
            data: Value::Individual(true.into()),
        };

        let res = RuntimeEvent::from_event(event).unwrap();

        assert_eq!(
            res,
            RuntimeEvent::Telemetry {
                interface: "foo".to_string(),
                data: TelemetryConfig::Enable(Some(true))
            }
        );

        let event = DeviceEvent {
            interface: "io.edgehog.devicemanager.config.Telemetry".to_string(),
            path: "/request/foo/periodSeconds".to_string(),
            data: Value::Individual(AstarteType::LongInteger(42)),
        };

        let res = RuntimeEvent::from_event(event).unwrap();

        assert_eq!(
            res,
            RuntimeEvent::Telemetry {
                interface: "foo".to_string(),
                data: TelemetryConfig::Period(Some(TelemetryPeriod(Duration::from_secs(42))))
            }
        );
    }

    #[test]
    fn shold_convert_led_from_event() {
        let event = DeviceEvent {
            interface: "io.edgehog.devicemanager.LedBehavior".to_string(),
            path: "/42/behavior".to_string(),
            data: Value::Individual("Blink60Seconds".into()),
        };

        let res = RuntimeEvent::from_event(event).unwrap();

        assert_eq!(
            res,
            RuntimeEvent::LedBehavior {
                led_id: "42".into(),
                behavior: LedBehavior::Behavior(Blink::Single)
            }
        );

        let event = DeviceEvent {
            interface: "io.edgehog.devicemanager.LedBehavior".to_string(),
            path: "/42/behavior".to_string(),
            data: Value::Individual("DoubleBlink60Seconds".into()),
        };

        let res = RuntimeEvent::from_event(event).unwrap();

        assert_eq!(
            res,
            RuntimeEvent::LedBehavior {
                led_id: "42".into(),
                behavior: LedBehavior::Behavior(Blink::Double)
            }
        );

        let event = DeviceEvent {
            interface: "io.edgehog.devicemanager.LedBehavior".to_string(),
            path: "/42/behavior".to_string(),
            data: Value::Individual("SlowBlink60Seconds".into()),
        };

        let res = RuntimeEvent::from_event(event).unwrap();

        assert_eq!(
            res,
            RuntimeEvent::LedBehavior {
                led_id: "42".into(),
                behavior: LedBehavior::Behavior(Blink::Slow)
            }
        );
    }
}
