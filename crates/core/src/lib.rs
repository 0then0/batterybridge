//! Platform-independent charging policy, monitoring/controller traits and TOML persistence.
//!
//! Device-specific interfaces are implemented by `batterybridge-platform`.

#![forbid(unsafe_code)]

/// Validation, migration and filesystem persistence for the saved TOML policy.
pub mod config;
/// Charging hysteresis and temporary top-up/discharge state transitions.
pub mod policy;
pub mod schedule;
pub use batterybridge_protocol::*;

/// Read-only battery information, independent of charge-control availability.
pub trait BatteryMonitor {
    /// Read the currently detected batteries in platform discovery order.
    fn batteries(&self) -> Result<Vec<BatteryStatus>>;
}
/// Device metadata suitable for display and compatibility reports.
pub trait DeviceInfoProvider {
    fn device_info(&self) -> Result<DeviceInfo>;
}
/// AC connectivity for the selected device.
pub trait PowerMonitor {
    /// Return `None` when the power source cannot be determined.
    fn on_ac(&self) -> Result<Option<bool>>;
}

/// Only a privileged service owns a controller. Read-only clients never receive one.
pub trait ChargeController {
    /// Describe available operations and any restricted limit values.
    fn capabilities(&self) -> Capabilities;
    /// Apply validated percentages, or request normal charging with both `None`.
    /// For a native-threshold backend this writes a value the hardware keeps
    /// without the service; whether it survives power-off is device-dependent.
    fn apply(&mut self, start: Option<u8>, stop: Option<u8>) -> Result<()>;
    /// Enable or release forced discharge, when supported by the adapter.
    fn force_discharge(&mut self, enabled: bool) -> Result<()> {
        let _ = enabled;
        Err(Error::Unsupported("force discharge".into()))
    }
    /// SMC binary gates use daemon hysteresis; native thresholds ignore this call.
    fn charging(&mut self, enabled: bool) -> Result<()> {
        let _ = enabled;
        Ok(())
    }
    fn magsafe_led(&mut self, mode: LedMode) -> Result<()> {
        let _ = mode;
        Err(Error::Unsupported("MagSafe LED".into()))
    }
    fn prevent_sleep(&mut self, enabled: bool) -> Result<()> {
        let _ = enabled;
        Err(Error::Unsupported("sleep prevention".into()))
    }
    /// Undo restrictions on the hardware. The scope is explicit because a native
    /// threshold and a daemon-held override are different mechanisms. An
    /// implementation must attempt every step of the scope even when one fails
    /// (see [`merge_steps`]) and must not touch mechanisms outside it.
    fn release(&mut self, scope: Release) -> Result<()>;
}

/// What a [`ChargeController::release`] call gives back to the hardware.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Release {
    /// Controls that only exist while this process maintains them: forced
    /// discharge, a binary charging gate (reopened), LED and idle-sleep overrides.
    /// A native firmware/kernel threshold is left exactly as it is.
    Overrides,
    /// `Overrides` plus every native threshold: normal, unrestricted charging.
    Everything,
}

/// Combine independent cleanup steps. Every step has already run; this only
/// reports. Failed steps are named; the first failure's kind is preserved so
/// callers can still match on it.
pub fn merge_steps(steps: impl IntoIterator<Item = (&'static str, Result<()>)>) -> Result<()> {
    let mut failures = steps
        .into_iter()
        .filter_map(|(name, result)| result.err().map(|error| (name, error)));
    let Some((name, first)) = failures.next() else {
        return Ok(());
    };
    let rest: Vec<_> = failures.map(|(name, _)| name).collect();
    Err(first.map_detail(|detail| {
        if rest.is_empty() {
            format!("{name}: {detail}")
        } else {
            format!("{name}: {detail}; also failed: {}", rest.join(", "))
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merged_cleanup_names_each_failed_step_and_keeps_the_first_kind() {
        assert!(merge_steps([("a", Ok(())), ("b", Ok(()))]).is_ok());
        let one = merge_steps([("a", Ok(())), ("b", Err(Error::HardwareError("x".into())))]);
        assert_eq!(one, Err(Error::HardwareError("b: x".into())));
        let two = merge_steps([
            ("a", Err(Error::PermissionDenied("p".into()))),
            ("b", Err(Error::HardwareError("x".into()))),
        ]);
        assert_eq!(
            two,
            Err(Error::PermissionDenied("a: p; also failed: b".into()))
        );
    }
}
