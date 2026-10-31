use crate::*;
use std::{
    fs,
    path::{Path, PathBuf},
};

const ROOT: &str = "/sys/class/power_supply";
fn text(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_owned())
}
fn number(path: impl AsRef<Path>) -> Option<u32> {
    text(path)?.parse().ok()
}
fn battery_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(root) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(Error::io("enumerate power supplies", e)),
    };
    let mut paths: Vec<_> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            text(p.join("type")).as_deref() == Some("Battery")
                && text(p.join("present")).as_deref() != Some("0")
        })
        .collect();
    paths.sort();
    Ok(paths)
}
#[derive(Clone)]
pub struct SysfsMonitor {
    root: PathBuf,
}
impl SysfsMonitor {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}
impl BatteryMonitor for SysfsMonitor {
    fn batteries(&self) -> Result<Vec<BatteryStatus>> {
        let ac = fs::read_dir(&self.root).ok().and_then(|entries| {
            let values: Vec<_> = entries
                .flatten()
                .filter(|e| text(e.path().join("type")).as_deref() != Some("Battery"))
                .filter_map(|e| number(e.path().join("online")))
                .collect();
            if values.is_empty() {
                None
            } else {
                Some(values.contains(&1))
            }
        });
        battery_paths(&self.root)?
            .iter()
            .enumerate()
            .map(|(index, p)| {
                let capacity = number(p.join("capacity"))
                    .filter(|v| *v <= 100)
                    .map(|v| v as u8);
                let health = ["energy", "charge"].iter().find_map(|prefix| {
                    let full = number(p.join(format!("{prefix}_full")))? as u64;
                    let design = number(p.join(format!("{prefix}_full_design")))? as u64;
                    (design > 0).then(|| ((full * 100 / design).min(100)) as u8)
                });
                Ok(BatteryStatus {
                    id: format!("BAT{index}"),
                    percent: capacity,
                    state: text(p.join("status"))
                        .unwrap_or_else(|| "unknown".into())
                        .to_lowercase(),
                    health_percent: health,
                    on_ac: ac,
                    hardware_start: number(p.join("charge_control_start_threshold"))
                        .and_then(|v| u8::try_from(v).ok()),
                    hardware_stop: number(p.join("charge_control_end_threshold"))
                        .and_then(|v| u8::try_from(v).ok()),
                    telemetry: telemetry(p),
                })
            })
            .collect()
    }
}
pub fn discover() -> Result<Platform> {
    let root = Path::new(ROOT);
    let monitor = SysfsMonitor::new(root.into());
    let batteries = monitor.batteries()?;
    let device = DeviceInfo {
        os: "linux".into(),
        os_version: text("/proc/sys/kernel/osrelease").unwrap_or_else(|| "unknown".into()),
        architecture: std::env::consts::ARCH.into(),
        vendor: text("/sys/class/dmi/id/sys_vendor").unwrap_or_else(|| "unknown".into()),
        model: text("/sys/class/dmi/id/product_name").unwrap_or_else(|| "unknown".into()),
    };
    let mut interfaces = BTreeMap::from([(
        ROOT.into(),
        if root.exists() {
            "available".into()
        } else {
            "absent".into()
        },
    )]);
    // Control targets the first present battery; report attributes for every pack.
    for (index, path) in battery_paths(root)?.iter().enumerate() {
        for key in [
            "charge_control_start_threshold",
            "charge_control_end_threshold",
        ] {
            let state = if number(path.join(key)).is_some() {
                "available"
            } else {
                "absent or unreadable"
            };
            interfaces.insert(format!("BAT{index}/{key}"), state.into());
            if index == 0 {
                interfaces.insert(key.into(), state.into());
            }
        }
    }
    let report = resolve(Facts {
        device: device.clone(),
        batteries,
        interfaces,
    });
    Ok(Platform {
        monitor: Box::new(monitor),
        device,
        report,
    })
}
pub fn controller() -> Result<Box<dyn ChargeController>> {
    let path = battery_paths(Path::new(ROOT))?
        .into_iter()
        .next()
        .ok_or_else(|| Error::BackendUnavailable("no battery".into()))?;
    Ok(Box::new(SysfsController::new(path)?))
}
pub struct SysfsController {
    path: PathBuf,
    caps: Capabilities,
}
impl SysfsController {
    pub fn new(path: PathBuf) -> Result<Self> {
        let end = number(path.join("charge_control_end_threshold")).is_some();
        if !end {
            return Err(Error::Unsupported(
                "kernel end threshold absent or unreadable".into(),
            ));
        }
        let mut caps = Capabilities::unsupported("not exposed by generic threshold backend");
        caps.charge_limit = Capability::supported("kernel end threshold");
        caps.stop_threshold = caps.charge_limit.clone();
        caps.temporary_full_charge = caps.charge_limit.clone();
        if number(path.join("charge_control_start_threshold")).is_some() {
            caps.start_threshold = caps.charge_limit.clone();
        }
        Ok(Self { path, caps })
    }
    fn write(&self, key: &str, value: u8) -> Result<()> {
        let path = self.path.join(key);
        if number(&path) == Some(value as u32) {
            return Ok(());
        }
        fs::write(&path, format!("{value}\n"))
            .map_err(|e| Error::io("write sysfs threshold", e))?;
        if number(path) != Some(value as u32) {
            return Err(Error::HardwareError(
                "kernel threshold readback mismatch".into(),
            ));
        }
        Ok(())
    }
}
impl ChargeController for SysfsController {
    fn capabilities(&self) -> Capabilities {
        self.caps.clone()
    }
    fn apply(&mut self, start: Option<u8>, stop: Option<u8>) -> Result<()> {
        Command::Limit { start, stop }.validate()?;
        if text(self.path.join("type")).as_deref() != Some("Battery")
            || text(self.path.join("present")).as_deref() == Some("0")
        {
            return Err(Error::BackendUnavailable(
                "selected battery is no longer present".into(),
            ));
        }
        if start.is_some() {
            batterybridge_core::policy::require(&self.caps.start_threshold)?;
        }
        let stop = stop.unwrap_or(100);
        let start = self
            .caps
            .start_threshold
            .available()
            .then_some(start.unwrap_or_else(|| {
                if stop == 100 {
                    99
                } else {
                    stop.saturating_sub(5)
                }
            }));
        apply_threshold_pair(self, start, stop)
    }
    /// The only mechanism is the kernel threshold pair. Whether the platform
    /// keeps it while the OS is off depends on the driver and firmware and is
    /// not documented by the power_supply ABI, so it is treated as unknown.
    /// There is no daemon-held state: `Overrides` has nothing to give back.
    fn release(&mut self, scope: Release) -> Result<()> {
        match scope {
            Release::Overrides => Ok(()),
            Release::Everything => self.apply(None, None),
        }
    }
}

const START: &str = "charge_control_start_threshold";
const STOP: &str = "charge_control_end_threshold";
trait ThresholdInterface {
    fn threshold(&self, key: &str) -> Result<u8>;
    fn set_threshold(&self, key: &str, value: u8) -> Result<()>;
}
impl ThresholdInterface for SysfsController {
    fn threshold(&self, key: &str) -> Result<u8> {
        number(self.path.join(key))
            .filter(|v| *v <= 100)
            .map(|v| v as u8)
            .ok_or_else(|| Error::BackendUnavailable(format!("invalid or unreadable {key}")))
    }
    fn set_threshold(&self, key: &str, value: u8) -> Result<()> {
        self.write(key, value)
    }
}
fn write_pair(io: &impl ThresholdInterface, start: Option<u8>, stop: u8) -> Result<()> {
    // Always keep start < stop, including during a rollback from a partial write.
    if let Some(start) = start
        && io.threshold(START)? >= stop
    {
        io.set_threshold(START, start)?;
    }
    io.set_threshold(STOP, stop)?;
    if let Some(start) = start {
        io.set_threshold(START, start)?;
    }
    Ok(())
}
fn apply_threshold_pair(io: &impl ThresholdInterface, start: Option<u8>, stop: u8) -> Result<()> {
    let previous_start = start.map(|_| io.threshold(START)).transpose()?;
    let previous_stop = io.threshold(STOP)?;
    if let Err(error) = write_pair(io, start, stop) {
        if let Err(recovery) = write_pair(io, previous_start, previous_stop) {
            return Err(Error::HardwareError(format!(
                "threshold update failed ({error}); rollback failed ({recovery})"
            )));
        }
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_file_adapter_and_safe_threshold_order() {
        let root = std::env::temp_dir().join(format!("batterybridge-sysfs-{}", std::process::id()));
        fs::create_dir_all(root.join("BAT0")).unwrap();
        let b = root.join("BAT0");
        for (k, v) in [
            ("type", "Battery"),
            ("capacity", "74"),
            ("status", "Charging"),
            ("charge_control_start_threshold", "90"),
            ("charge_control_end_threshold", "95"),
        ] {
            fs::write(b.join(k), v).unwrap();
        }
        let m = SysfsMonitor::new(root.clone());
        assert_eq!(m.batteries().unwrap()[0].percent, Some(74));
        let mut c = SysfsController::new(b.clone()).unwrap();
        c.apply(Some(75), Some(80)).unwrap();
        assert_eq!(number(b.join("charge_control_end_threshold")), Some(80));
        assert!(c.apply(Some(90), Some(80)).is_err());
        c.release(Release::Overrides).unwrap();
        assert_eq!(number(b.join("charge_control_end_threshold")), Some(80));
        c.release(Release::Everything).unwrap();
        assert_eq!(number(b.join("charge_control_end_threshold")), Some(100));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn default_band_and_top_up_do_not_install_a_zero_start_threshold() {
        let root = std::env::temp_dir().join(format!("bb-start-threshold-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        for (k, v) in [
            ("type", "Battery"),
            ("charge_control_start_threshold", "75"),
            ("charge_control_end_threshold", "80"),
        ] {
            fs::write(root.join(k), v).unwrap();
        }
        let mut controller = SysfsController::new(root.clone()).unwrap();
        controller.apply(None, Some(80)).unwrap();
        assert_eq!(
            number(root.join("charge_control_start_threshold")),
            Some(75)
        );
        controller.apply(None, None).unwrap();
        assert_eq!(
            number(root.join("charge_control_start_threshold")),
            Some(99)
        );
        fs::remove_dir_all(root).unwrap();
    }

    struct Pair {
        values: std::cell::RefCell<(u8, u8)>,
        fail_stop: Option<u8>,
    }
    impl ThresholdInterface for Pair {
        fn threshold(&self, key: &str) -> Result<u8> {
            let v = self.values.borrow();
            Ok(if key == START { v.0 } else { v.1 })
        }
        fn set_threshold(&self, key: &str, value: u8) -> Result<()> {
            if key == STOP && self.fail_stop == Some(value) {
                return Err(Error::PermissionDenied(
                    "injected end-threshold failure".into(),
                ));
            }
            let mut v = self.values.borrow_mut();
            if key == START {
                assert!(value < v.1, "start must stay below stop");
                v.0 = value;
            } else {
                assert!(v.0 < value, "stop must stay above start");
                v.1 = value;
            }
            Ok(())
        }
    }
    #[test]
    fn threshold_transitions_obey_kernel_order_and_rollback_first_write() {
        let io = Pair {
            values: std::cell::RefCell::new((90, 95)),
            fail_stop: Some(80),
        };
        assert!(apply_threshold_pair(&io, Some(75), 80).is_err());
        assert_eq!(
            *io.values.borrow(),
            (90, 95),
            "a failed end write must restore start too"
        );
        let io = Pair {
            values: std::cell::RefCell::new((90, 95)),
            fail_stop: None,
        };
        apply_threshold_pair(&io, Some(75), 80).unwrap();
        assert_eq!(*io.values.borrow(), (75, 80));
        apply_threshold_pair(&io, Some(90), 95).unwrap();
        assert_eq!(*io.values.borrow(), (90, 95));
    }
}

fn telemetry(path: &Path) -> BatteryTelemetry {
    let direction: Option<i64> = match text(path.join("status")).as_deref() {
        Some("Charging") => Some(1),
        Some("Discharging") => Some(-1),
        _ => None,
    };
    let signed = |name| {
        let direction = direction?;
        text(path.join(name))
            .and_then(|s| s.parse::<i64>().ok())
            .and_then(|v| v.checked_abs())
            .and_then(|v| i32::try_from(v / 1000 * direction).ok())
    };
    BatteryTelemetry {
        temperature_deci_c: text(path.join("temp"))
            .and_then(|v| v.parse().ok())
            .filter(|v| (-400..=1200).contains(v)),
        voltage_mv: number(path.join("voltage_now")).map(|v| v / 1000),
        current_ma: signed("current_now"),
        power_mw: signed("power_now"),
        cycle_count: number(path.join("cycle_count")),
        raw_percent: ["energy", "charge"].iter().find_map(|prefix| {
            let now = u64::from(number(path.join(format!("{prefix}_now")))?);
            let max = u64::from(number(path.join(format!("{prefix}_full")))?);
            (max > 0).then(|| (now * 100 / max).min(100) as u8)
        }),
        remaining_minutes: direction
            .and_then(|direction| {
                number(path.join(if direction > 0 {
                    "time_to_full_now"
                } else {
                    "time_to_empty_now"
                }))
            })
            .map(|v| v / 60),
    }
}

#[cfg(test)]
mod telemetry_tests {
    use super::*;
    #[test]
    fn unknown_direction_is_absent_instead_of_fabricated_zero() {
        let root = std::env::temp_dir().join(format!(
            "batterybridge-unknown-direction-{}",
            std::process::id()
        ));
        let path = root.join("BAT0");
        fs::create_dir_all(&path).unwrap();
        for (name, value) in [
            ("type", "Battery"),
            ("current_now", "1200000"),
            ("power_now", "14400000"),
            ("time_to_empty_now", "3600"),
        ] {
            fs::write(path.join(name), value).unwrap();
        }
        let monitor = SysfsMonitor::new(root.clone());
        for status in [None, Some("Unknown"), Some("Not charging"), Some("Full")] {
            if let Some(status) = status {
                fs::write(path.join("status"), status).unwrap();
            }
            let battery = monitor.batteries().unwrap().remove(0);
            assert_eq!(battery.telemetry.current_ma, None);
            assert_eq!(battery.telemetry.power_mw, None);
            assert_eq!(battery.telemetry.remaining_minutes, None);
        }
        fs::write(path.join("status"), "Charging").unwrap();
        let battery = monitor.batteries().unwrap().remove(0);
        assert_eq!(battery.telemetry.current_ma, Some(1200));
        assert_eq!(battery.telemetry.power_mw, Some(14400));
        fs::write(path.join("current_now"), "0").unwrap();
        fs::write(path.join("power_now"), "0").unwrap();
        let battery = monitor.batteries().unwrap().remove(0);
        assert_eq!(battery.telemetry.current_ma, Some(0));
        assert_eq!(battery.telemetry.power_mw, Some(0));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn sysfs_units_directions_and_capacity_ratio_are_not_invented() {
        let path =
            std::env::temp_dir().join(format!("batterybridge-telemetry-{}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        for (name, value) in [
            ("temp", "315"),
            ("status", "Discharging"),
            ("voltage_now", "12500000"),
            ("current_now", "1200000"),
            ("power_now", "15000000"),
            ("cycle_count", "123"),
            ("energy_now", "42000000"),
            ("energy_full", "60000000"),
            ("time_to_empty_now", "3600"),
        ] {
            fs::write(path.join(name), value).unwrap();
        }
        let t = telemetry(&path);
        assert_eq!(t.temperature_deci_c, Some(315));
        assert_eq!(t.voltage_mv, Some(12500));
        assert_eq!(t.current_ma, Some(-1200));
        assert_eq!(t.power_mw, Some(-15000));
        assert_eq!(t.raw_percent, Some(70));
        assert_eq!(t.remaining_minutes, Some(60));
        fs::write(path.join("energy_full"), "0").unwrap();
        fs::write(path.join("temp"), "invalid").unwrap();
        let t = telemetry(&path);
        assert_eq!(t.raw_percent, None);
        assert_eq!(t.temperature_deci_c, None);
        fs::remove_dir_all(path).unwrap();
    }
}
