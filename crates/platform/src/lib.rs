//! Device discovery, capability resolution and native battery adapters.
//!
//! Read-only discovery is separate from controller construction. Linux sysfs,
//! macOS IOKit/SMC and Windows WMI adapters implement the shared core traits.
use batterybridge_core::*;
use std::collections::BTreeMap;

pub mod client;
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(windows)]
pub mod windows;

pub struct Platform {
    pub monitor: Box<dyn BatteryMonitor>,
    pub device: DeviceInfo,
    pub report: ProbeReport,
}
impl Platform {
    pub fn discover() -> Result<Self> {
        #[cfg(target_os = "linux")]
        {
            linux::discover()
        }
        #[cfg(target_os = "macos")]
        {
            macos::discover()
        }
        #[cfg(windows)]
        {
            windows::discover()
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            Err(Error::Unsupported("operating system".into()))
        }
    }
    pub fn controller(&self) -> Result<Box<dyn ChargeController>> {
        batterybridge_core::policy::require(&self.report.capabilities.charge_limit)?;
        #[cfg(target_os = "linux")]
        {
            linux::controller()
        }
        #[cfg(target_os = "macos")]
        {
            macos::controller()
        }
        #[cfg(windows)]
        {
            windows::controller(&self.device)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            Err(Error::Unsupported("operating system".into()))
        }
    }
    pub fn backend(&self) -> String {
        self.report
            .candidates
            .iter()
            .find(|c| c.selected)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| "monitor only".into())
    }
}
impl DeviceInfoProvider for Platform {
    fn device_info(&self) -> Result<DeviceInfo> {
        Ok(self.device.clone())
    }
}
impl PowerMonitor for Platform {
    fn on_ac(&self) -> Result<Option<bool>> {
        Ok(self.monitor.batteries()?.first().and_then(|b| b.on_ac))
    }
}

/// Sanitized discovery facts, also consumed by fixtures. No serials, UUIDs, or WMI paths.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Facts {
    pub device: DeviceInfo,
    pub batteries: Vec<BatteryStatus>,
    pub interfaces: BTreeMap<String, String>,
}
pub fn resolve(facts: Facts) -> ProbeReport {
    let mut caps = Capabilities::unsupported("no compatible charge-control interface detected");
    let has = |s: &str| facts.interfaces.get(s).is_some_and(|v| v == "available");
    let (name, mut selected, mut reason) = match facts.device.os.as_str() {
        "linux" => {
            let end = has("charge_control_end_threshold");
            if end {
                caps.charge_limit = Capability::supported(
                    "kernel end threshold; writes require device permissions",
                );
                caps.stop_threshold = caps.charge_limit.clone();
                caps.temporary_full_charge =
                    Capability::supported("temporary 100% threshold, restored by daemon");
                if has("charge_control_start_threshold") {
                    caps.start_threshold = Capability::supported("kernel start threshold");
                }
            }
            (
                "Linux sysfs",
                end,
                if end {
                    "standard threshold attributes are readable"
                } else {
                    "kernel exposes no end threshold for selected battery"
                },
            )
        }
        "macos" => {
            let gate = matches!(facts.device.architecture.as_str(), "aarch64" | "x86_64")
                && (has("SMC:CHTE") || has("SMC:CH0C"));
            if gate {
                caps.charge_limit = Capability::experimental(
                    "SMC binary gate; daemon enforces hysteresis while awake; firmware may deny writes",
                );
                caps.stop_threshold = caps.charge_limit.clone();
                caps.start_threshold = caps.charge_limit.clone();
                caps.temporary_full_charge =
                    Capability::experimental("daemon temporarily releases gate until full");
                caps.manual_charging = caps.charge_limit.clone();
                caps.sleep_prevention = Capability::supported(
                    "scoped system-sleep assertion while charging or discharging on AC; holds closed-lid sleep until the limit",
                );
                if facts
                    .batteries
                    .first()
                    .is_some_and(|b| b.telemetry.temperature_deci_c.is_some())
                {
                    caps.heat_protection = Capability::experimental(
                        "battery temperature sensor and SMC charging gate",
                    );
                }
                if has("SMC:CHIE") || has("SMC:CH0J") || has("SMC:CH0I") {
                    caps.force_discharge = Capability::experimental(
                        "SMC adapter gate; daemon restores adapter at target",
                    );
                    caps.adapter_disable = caps.force_discharge.clone();
                }
            }
            let bclm = facts.device.architecture == "x86_64" && has("SMC:BCLM");
            if bclm {
                if !gate {
                    caps = Capabilities::unsupported("not exposed by Intel BCLM");
                }
                caps.charge_limit = Capability::experimental(
                    "Intel SMC BCLM firmware threshold, 50..100%; device write compatibility is unverified",
                );
                caps.stop_threshold = caps.charge_limit.clone();
                caps.temporary_full_charge =
                    Capability::experimental("temporary 100% threshold, restored by daemon");
                caps.allowed_limits = Some((50..=100).collect());
            }
            if has("SMC:ACLC") && (gate || bclm) {
                caps.magsafe_led = Capability::experimental(
                    "MagSafe ACLC LED override; requires compatible connected adapter",
                );
            }
            (
                if bclm {
                    "Intel SMC (BCLM)"
                } else if facts.device.architecture == "x86_64" {
                    "Intel SMC"
                } else {
                    "Apple Silicon SMC"
                },
                gate || bclm,
                if bclm {
                    "Intel BCLM firmware threshold detected; write compatibility is unverified"
                } else if gate {
                    "SMC charging gate detected; write compatibility is unverified for this device"
                } else {
                    "no accessible known SMC gate"
                },
            )
        }
        "windows" => {
            let available = facts.device.vendor.to_lowercase().contains("lenovo")
                && has("WMI:LENOVO_OTHER_METHOD")
                && has("WMI:capdata:0x03010001")
                && facts.batteries.len() == 1;
            if available {
                caps.charge_limit = Capability::experimental(
                    "Lenovo WMI conservation mode, fixed 80%; device write compatibility is unverified",
                );
                caps.stop_threshold = caps.charge_limit.clone();
                caps.allowed_limits = Some(vec![80]);
                // Getter is unreliable on affected firmware; no top-up until a
                // reliable full-charge restore/readback contract is established.
            }
            (
                "Lenovo WMI (fixed 80%)",
                available,
                if available {
                    "Lenovo vendor, WMI methods and settable capability record all match"
                } else {
                    "requires Lenovo and readable/settable 0x03010001 capability data; generic Windows has no charge-limit API"
                },
            )
        }
        _ => ("unsupported", false, "unsupported OS"),
    };
    if facts.device.os == "macos"
        && matches!(facts.device.architecture.as_str(), "aarch64" | "x86_64")
        && !selected
    {
        let denial = facts.interfaces.iter().find(|(name, value)| {
            (name.starts_with("SMC:") || name.as_str() == "AppleSMC")
                && value.starts_with("permission denied:")
        });
        if let Some((_, reason)) = denial {
            let unknown = Capability::unknown(reason.clone());
            caps.charge_limit = unknown.clone();
            caps.start_threshold = unknown.clone();
            caps.stop_threshold = unknown.clone();
            caps.temporary_full_charge = unknown.clone();
            caps.force_discharge = unknown;
        }
    }
    if facts.batteries.is_empty() {
        selected = false;
        reason = "no present battery";
        caps = Capabilities::unsupported(reason);
    }
    ProbeReport {
        schema_version: 1,
        device: facts.device,
        battery_ids: facts.batteries.into_iter().map(|b| b.id).collect(),
        interfaces: facts.interfaces,
        candidates: vec![Candidate {
            name: name.into(),
            selected,
            reason: reason.into(),
        }],
        capabilities: caps,
    }
}

/// A sleeping service rejects hardware commands until wake completion.
pub enum PowerEvent {
    #[cfg(target_os = "macos")]
    Sleep(macos::power::SleepAck),
    Wake,
}
pub fn watch_power(
    callback: impl Fn(PowerEvent) + Send + 'static,
) -> Result<std::sync::Arc<std::sync::atomic::AtomicBool>> {
    #[cfg(target_os = "macos")]
    {
        macos::power::watch(callback)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = callback;
        Ok(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
            false,
        )))
    }
}
