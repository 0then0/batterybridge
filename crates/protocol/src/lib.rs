//! Typed commands, responses, capabilities and configuration data shared by BatteryBridge clients and service.
//!
//! Requests carry a protocol version and validate charging ranges before execution.

#![forbid(unsafe_code)]

mod care;
pub use care::*;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Request version accepted by matching BatteryBridge clients and service.
pub const PROTOCOL_VERSION: u32 = 3;
/// Maximum JSON payload size in bytes, excluding the four-byte frame header.
pub const MAX_FRAME: usize = 64 * 1024;
pub type Result<T> = std::result::Result<T, Error>;

/// Typed failures from validation, discovery, IPC and hardware operations.
#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error, PartialEq, Eq)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum Error {
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error("service unavailable: {0}")]
    ServiceUnavailable(String),
    #[error("backend unavailable: {0}")]
    BackendUnavailable(String),
    #[error("hardware error: {0}")]
    HardwareError(String),
    #[error("invalid limit: {0}")]
    InvalidLimit(String),
    #[error("platform error: {0}")]
    PlatformError(String),
    #[error("configuration error: {0}")]
    ConfigurationError(String),
    #[error("protocol error: {0}")]
    ProtocolError(String),
}
impl Error {
    /// Rewrite the human-readable detail while keeping the error kind.
    pub fn map_detail(self, f: impl FnOnce(String) -> String) -> Self {
        match self {
            Self::Unsupported(d) => Self::Unsupported(f(d)),
            Self::PermissionDenied(d) => Self::PermissionDenied(f(d)),
            Self::ServiceUnavailable(d) => Self::ServiceUnavailable(f(d)),
            Self::BackendUnavailable(d) => Self::BackendUnavailable(f(d)),
            Self::HardwareError(d) => Self::HardwareError(f(d)),
            Self::InvalidLimit(d) => Self::InvalidLimit(f(d)),
            Self::PlatformError(d) => Self::PlatformError(f(d)),
            Self::ConfigurationError(d) => Self::ConfigurationError(f(d)),
            Self::ProtocolError(d) => Self::ProtocolError(f(d)),
        }
    }
    pub fn io(operation: &str, e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::PermissionDenied => Self::PermissionDenied(operation.into()),
            _ => Self::PlatformError(format!("{operation}: {e}")),
        }
    }
}

/// Interface availability and validation status for a charging operation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityState {
    Supported,
    Experimental,
    Unsupported,
    Unknown,
}
/// Availability of one operation with a user-visible explanation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Capability {
    pub state: CapabilityState,
    pub reason: String,
}
impl Default for Capability {
    fn default() -> Self {
        Self::unsupported("not exposed by this backend")
    }
}
impl Capability {
    pub fn supported(reason: impl Into<String>) -> Self {
        Self {
            state: CapabilityState::Supported,
            reason: reason.into(),
        }
    }
    pub fn experimental(reason: impl Into<String>) -> Self {
        Self {
            state: CapabilityState::Experimental,
            reason: reason.into(),
        }
    }
    pub fn unsupported(reason: impl Into<String>) -> Self {
        Self {
            state: CapabilityState::Unsupported,
            reason: reason.into(),
        }
    }
    pub fn unknown(reason: impl Into<String>) -> Self {
        Self {
            state: CapabilityState::Unknown,
            reason: reason.into(),
        }
    }
    pub fn available(&self) -> bool {
        matches!(
            self.state,
            CapabilityState::Supported | CapabilityState::Experimental
        )
    }
}
/// Charging operations available through the selected backend.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Capabilities {
    pub charge_limit: Capability,
    pub start_threshold: Capability,
    pub stop_threshold: Capability,
    pub force_discharge: Capability,
    pub adapter_disable: Capability,
    pub temporary_full_charge: Capability,
    #[serde(default)]
    pub manual_charging: Capability,
    #[serde(default)]
    pub heat_protection: Capability,
    #[serde(default)]
    pub magsafe_led: Capability,
    #[serde(default)]
    pub sleep_prevention: Capability,
    /// None means a continuous range 1..=100; Some restricts accepted limits.
    pub allowed_limits: Option<Vec<u8>>,
}
impl Capabilities {
    pub fn unsupported(reason: &str) -> Self {
        let c = Capability::unsupported(reason);
        Self {
            charge_limit: c.clone(),
            start_threshold: c.clone(),
            stop_threshold: c.clone(),
            force_discharge: c.clone(),
            adapter_disable: c.clone(),
            temporary_full_charge: c.clone(),
            manual_charging: c.clone(),
            heat_protection: c.clone(),
            magsafe_led: c.clone(),
            sleep_prevention: c,
            allowed_limits: None,
        }
    }
    pub fn entries(&self) -> [(&str, &Capability); 10] {
        [
            ("Charge limit", &self.charge_limit),
            ("Start threshold", &self.start_threshold),
            ("Stop threshold", &self.stop_threshold),
            ("Force discharge", &self.force_discharge),
            ("Adapter disable", &self.adapter_disable),
            ("Top up", &self.temporary_full_charge),
            ("Manual charging", &self.manual_charging),
            ("Heat protection", &self.heat_protection),
            ("MagSafe LED", &self.magsafe_led),
            ("Sleep prevention", &self.sleep_prevention),
        ]
    }
}
/// Read-only battery values; percentages use the range 0 through 100.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BatteryStatus {
    /// A local ordinal such as BAT0, never a serial number or WMI instance path.
    pub id: String,
    pub percent: Option<u8>,
    pub state: String,
    pub health_percent: Option<u8>,
    pub on_ac: Option<bool>,
    pub hardware_start: Option<u8>,
    pub hardware_stop: Option<u8>,
    #[serde(default)]
    pub telemetry: BatteryTelemetry,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceInfo {
    pub os: String,
    pub os_version: String,
    pub architecture: String,
    pub vendor: String,
    pub model: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Candidate {
    pub name: String,
    pub selected: bool,
    pub reason: String,
}
/// Versioned compatibility report containing allowlisted discovery results.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeReport {
    pub schema_version: u32,
    pub device: DeviceInfo,
    pub battery_ids: Vec<String>,
    pub interfaces: BTreeMap<String, String>,
    pub candidates: Vec<Candidate>,
    pub capabilities: Capabilities,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StartThreshold {
    pub enabled: bool,
    pub value: u8,
}
/// Persistent desired charging settings, separate from temporary service state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    pub charge_limit: Option<u8>,
    pub start_threshold: StartThreshold,
    #[serde(default)]
    pub care: CareSettings,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            version: 2,
            charge_limit: None,
            start_threshold: StartThreshold {
                enabled: false,
                value: 75,
            },
            care: CareSettings::default(),
        }
    }
}
/// Device information and service policy, or read-only local status when offline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub batteries: Vec<BatteryStatus>,
    pub device: DeviceInfo,
    pub backend: String,
    pub capabilities: Capabilities,
    /// `None` means all service policy state is unavailable, including the
    /// temporary-mode fields below. Ignore those fields when configuration is absent.
    pub config: Option<Config>,
    pub top_up_active: bool,
    pub discharge_target: Option<u8>,
    pub last_error: Option<Error>,
    #[serde(default)]
    pub runtime: RuntimeStatus,
}
/// Fixed operations accepted by the service; charging values are validated before use.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Status,
    Capabilities,
    Probe,
    Config,
    Limit {
        stop: Option<u8>,
        start: Option<u8>,
    },
    TopUp,
    Discharge {
        target: u8,
    },
    ChargeTo {
        target: u8,
    },
    StopCharging,
    Pause,
    Resume,
    Cancel,
    /// Release all controls and durably disable limits, care settings and schedules.
    Reset,
    Calibrate,
    Adapter {
        enabled: bool,
    },
    Care {
        settings: CareSettings,
        expected: CareSettings,
    },
}
impl Command {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Limit { stop, start } => {
                if let Some(v) = stop {
                    percent(*v)?;
                }
                if let Some(v) = start
                    && stop.is_none_or(|s| *v >= s)
                {
                    return Err(Error::InvalidLimit("start must be below stop".into()));
                }
            }
            Self::ChargeTo { target } => percent(*target)?,
            Self::Care { settings, .. } => settings.validate()?,
            Self::Discharge { target } => {
                percent(*target)?;
                if *target < 20 {
                    return Err(Error::InvalidLimit("discharge floor is 20%".into()));
                }
            }
            _ => {}
        }
        Ok(())
    }
}
pub fn percent(value: u8) -> Result<()> {
    if !(1..=100).contains(&value) {
        return Err(Error::InvalidLimit("expected 1..=100".into()));
    }
    Ok(())
}
/// A versioned command sent by a client to the service.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u32,
    pub command: Command,
}
impl Request {
    pub fn new(command: Command) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            command,
        }
    }
    pub fn validate(&self) -> Result<()> {
        if self.version != PROTOCOL_VERSION {
            return Err(Error::ProtocolError("incompatible protocol version".into()));
        }
        self.command.validate()
    }
}
/// A typed service result or validation/execution error.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Response {
    Status(Box<Snapshot>),
    Capabilities(Box<Capabilities>),
    Probe(Box<ProbeReport>),
    Config(Config),
    Ok,
    Error(Error),
}

impl CareSettings {
    pub fn validate(&self) -> Result<()> {
        if self.heat_limit_c.is_some_and(|c| !(25..=60).contains(&c)) {
            return Err(Error::ConfigurationError(
                "heat limit must be 25..60°C".into(),
            ));
        }
        if self.schedules.len() > 16 {
            return Err(Error::ConfigurationError("at most 16 schedules".into()));
        }
        let mut ids = std::collections::BTreeSet::new();
        for schedule in &self.schedules {
            if !ids.insert(schedule.id)
                || !(1..=253_402_300_799).contains(&schedule.next_at)
                || schedule
                    .anchor_at
                    .is_some_and(|at| !(1..=schedule.next_at).contains(&at))
            {
                return Err(Error::ConfigurationError(
                    "duplicate schedule id or invalid timestamp".into(),
                ));
            }
            match schedule.action {
                ScheduleAction::Limit { stop, start } => Command::Limit {
                    stop: Some(stop),
                    start,
                }
                .validate()?,
                ScheduleAction::ChargeTo { target } => Command::ChargeTo { target }.validate()?,
                ScheduleAction::Discharge { target } => Command::Discharge { target }.validate()?,
                _ => {}
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn care_validates_bounds_duplicate_ids_and_calibration_floor_stays_private() {
        let task = Schedule {
            id: 1,
            enabled: true,
            next_at: 1000,
            anchor_at: None,
            repeat: Repeat::Once,
            action: ScheduleAction::Discharge { target: 10 },
        };
        let mut care = CareSettings {
            schedules: vec![task],
            ..CareSettings::default()
        };
        assert!(care.validate().is_err());
        care.schedules[0].action = ScheduleAction::Calibrate;
        assert!(care.validate().is_ok());
        care.schedules.push(care.schedules[0].clone());
        assert!(care.validate().is_err());
        care.schedules.clear();
        care.heat_limit_c = Some(24);
        assert!(care.validate().is_err());
        care.heat_limit_c = Some(35);
        assert!(care.validate().is_ok());
        assert!(
            serde_json::from_str::<CareSettings>(r#"{"heat_limit_c":35,"smc_key":"ACLC"}"#)
                .is_err()
        );
    }

    #[test]
    fn validates_untrusted_requests() {
        assert!(
            Request::new(Command::Limit {
                stop: Some(0),
                start: None
            })
            .validate()
            .is_err()
        );
        assert!(
            Request::new(Command::Limit {
                stop: Some(80),
                start: Some(80)
            })
            .validate()
            .is_err()
        );
        assert!(
            Request::new(Command::Discharge { target: 10 })
                .validate()
                .is_err()
        );
        assert!(
            serde_json::from_str::<Command>(r#"{"command":"limit","stop":80,"shell":"rm"}"#)
                .is_err()
        );
    }
}
