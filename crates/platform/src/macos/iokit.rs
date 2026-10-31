//! Minimal IOKit/CoreFoundation read boundary. Only named public battery fields are read.
use batterybridge_core::*;
use std::{
    ffi::{CString, c_char, c_void},
    ptr,
};
type Cf = *const c_void;
#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOServiceMatching(name: *const c_char) -> Cf;
    fn IOServiceGetMatchingService(port: u32, matching: Cf) -> u32;
    fn IOObjectRelease(object: u32) -> i32;
    fn IORegistryEntryCreateCFProperty(entry: u32, key: Cf, allocator: Cf, options: u32) -> Cf;
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFStringCreateWithCString(allocator: Cf, s: *const c_char, encoding: u32) -> Cf;
    fn CFRelease(object: Cf);
    fn CFGetTypeID(object: Cf) -> usize;
    fn CFNumberGetTypeID() -> usize;
    fn CFBooleanGetTypeID() -> usize;
    fn CFNumberGetValue(number: Cf, kind: isize, value: *mut c_void) -> bool;
    fn CFBooleanGetValue(boolean: Cf) -> bool;
}
struct Object(u32);
impl Drop for Object {
    fn drop(&mut self) {
        // SAFETY: Object owns one live IOKit reference.
        unsafe {
            IOObjectRelease(self.0);
        }
    }
}
struct Value(Cf);
impl Drop for Value {
    fn drop(&mut self) {
        // SAFETY: Value only wraps non-null owned CF references.
        unsafe {
            CFRelease(self.0);
        }
    }
}
fn service() -> Result<Object> {
    // SAFETY: static NUL-terminated service name; matching dictionary is consumed by IOKit.
    let object =
        unsafe { IOServiceGetMatchingService(0, IOServiceMatching(c"AppleSmartBattery".as_ptr())) };
    if object == 0 {
        return Err(Error::BackendUnavailable(
            "AppleSmartBattery service missing".into(),
        ));
    }
    Ok(Object(object))
}
fn property(object: &Object, name: &str) -> Option<Value> {
    let name = CString::new(name).ok()?;
    // SAFETY: CString remains alive; UTF-8 encoding constant; null allocator means default.
    let key = unsafe { CFStringCreateWithCString(ptr::null(), name.as_ptr(), 0x08000100) };
    if key.is_null() {
        return None;
    }
    let key = Value(key);
    // SAFETY: both handles are owned live references; result follows Create rule.
    let value = unsafe { IORegistryEntryCreateCFProperty(object.0, key.0, ptr::null(), 0) };
    if value.is_null() {
        None
    } else {
        Some(Value(value))
    }
}
fn number(object: &Object, name: &str) -> Option<i64> {
    let v = property(object, name)?;
    // SAFETY: non-null CF reference and type is checked before CFNumber conversion.
    unsafe {
        if CFGetTypeID(v.0) != CFNumberGetTypeID() {
            return None;
        }
        let mut out = 0i64;
        CFNumberGetValue(v.0, 4, (&mut out as *mut i64).cast()).then_some(out)
    }
}
fn boolean(object: &Object, name: &str) -> Option<bool> {
    let v = property(object, name)?;
    // SAFETY: live reference; boolean type checked before conversion.
    unsafe { (CFGetTypeID(v.0) == CFBooleanGetTypeID()).then(|| CFBooleanGetValue(v.0)) }
}
pub struct Monitor;
impl BatteryMonitor for Monitor {
    fn batteries(&self) -> Result<Vec<BatteryStatus>> {
        let obj = match service() {
            Ok(v) => v,
            Err(Error::BackendUnavailable(_)) => return Ok(vec![]),
            Err(e) => return Err(e),
        };
        let current = number(&obj, "CurrentCapacity");
        let max = number(&obj, "MaxCapacity");
        let percent = current
            .zip(max)
            .filter(|(_, m)| *m > 0)
            .map(|(v, m)| (v.saturating_mul(100) / m).clamp(0, 100) as u8);
        let health = number(&obj, "AppleRawMaxCapacity")
            .zip(number(&obj, "DesignCapacity"))
            .filter(|(_, d)| *d > 0)
            .map(|(v, d)| (v.saturating_mul(100) / d).clamp(0, 100) as u8);
        let state = if boolean(&obj, "FullyCharged") == Some(true) {
            "full"
        } else if boolean(&obj, "IsCharging") == Some(true) {
            "charging"
        } else if boolean(&obj, "ExternalConnected") == Some(false) {
            "discharging"
        } else {
            "not charging"
        };
        Ok(vec![BatteryStatus {
            id: "BAT0".into(),
            percent,
            state: state.into(),
            health_percent: health,
            on_ac: boolean(&obj, "ExternalConnected"),
            hardware_start: None,
            hardware_stop: None,
            telemetry: telemetry(&obj),
        }])
    }
}

unsafe extern "C" {
    fn sysctlbyname(
        name: *const c_char,
        old: *mut c_void,
        length: *mut usize,
        new: *mut c_void,
        new_length: usize,
    ) -> i32;
}
pub fn model() -> String {
    let mut bytes = [0u8; 256];
    let mut length = bytes.len();
    // SAFETY: fixed public sysctl name and bounded writable buffer. Null new
    // pointer makes this strictly read-only; returned length is checked below.
    let result = unsafe {
        sysctlbyname(
            c"hw.model".as_ptr(),
            bytes.as_mut_ptr().cast(),
            &mut length,
            ptr::null_mut(),
            0,
        )
    };
    if result != 0 || length > bytes.len() {
        return "unknown".into();
    }
    String::from_utf8_lossy(&bytes[..length])
        .trim_end_matches('\0')
        .to_owned()
}

fn telemetry(obj: &Object) -> BatteryTelemetry {
    let voltage_mv = number(obj, "Voltage")
        .and_then(|v| u32::try_from(v).ok())
        .filter(|v| *v > 0);
    let current_ma = number(obj, "InstantAmperage").and_then(|v| i32::try_from(v).ok());
    BatteryTelemetry {
        temperature_deci_c: number(obj, "Temperature").and_then(temperature_deci_c),
        voltage_mv,
        current_ma,
        power_mw: voltage_mv
            .zip(current_ma)
            .and_then(|(v, a)| i32::try_from(i64::from(v) * i64::from(a) / 1000).ok()),
        cycle_count: number(obj, "CycleCount").and_then(|v| u32::try_from(v).ok()),
        raw_percent: number(obj, "AppleRawCurrentCapacity")
            .zip(number(obj, "AppleRawMaxCapacity"))
            .filter(|(v, max)| *v >= 0 && *max > 0)
            .map(|(v, max)| (v.saturating_mul(100) / max).min(100) as u8),
        remaining_minutes: number(obj, "TimeRemaining")
            .and_then(|v| u32::try_from(v).ok())
            .filter(|v| *v < 65535),
    }
}

fn temperature_deci_c(centi_c: i64) -> Option<i16> {
    // AppleSmartBattery Temperature is centi-Celsius, as documented in Apple's
    // PowerManagement/pmconfigd/Resources/com.apple.bdc.metadata.plist (BDC_SBC).
    (-4000..=12000)
        .contains(&centi_c)
        .then_some((centi_c / 10) as i16)
}

#[cfg(test)]
mod telemetry_tests {
    use super::*;

    #[test]
    fn smart_battery_centi_celsius_converts_without_kelvin_offset() {
        for (raw, expected) in [
            (2700, 270),
            (3100, 310),
            (4000, 400),
            (-1000, -100),
            (12000, 1200),
        ] {
            assert_eq!(temperature_deci_c(raw), Some(expected));
        }
        for raw in [-4001, 12001, i64::MIN, i64::MAX] {
            assert_eq!(temperature_deci_c(raw), None);
        }
    }

    #[test]
    fn converted_sensor_values_drive_the_actual_heat_threshold_and_hysteresis() {
        #[derive(Default)]
        struct Controller(bool);
        impl ChargeController for Controller {
            fn capabilities(&self) -> Capabilities {
                let mut caps = Capabilities::unsupported("fixture");
                caps.charge_limit = Capability::supported("fixture");
                caps.heat_protection = caps.charge_limit.clone();
                caps
            }
            fn apply(&mut self, _: Option<u8>, _: Option<u8>) -> Result<()> {
                Ok(())
            }
            fn charging(&mut self, enabled: bool) -> Result<()> {
                self.0 = enabled;
                Ok(())
            }
            fn release(&mut self, _: Release) -> Result<()> {
                Ok(())
            }
        }
        let mut controller = Controller::default();
        let mut policy = policy::Policy::default();
        policy.config.care.heat_limit_c = Some(35);
        for (raw, enabled) in [
            (3100, true),
            (3500, false),
            (3400, false),
            (3300, true),
            (4000, false),
        ] {
            let battery = BatteryStatus {
                id: "BAT0".into(),
                percent: Some(50),
                state: "charging".into(),
                on_ac: Some(true),
                health_percent: None,
                hardware_start: None,
                hardware_stop: None,
                telemetry: BatteryTelemetry {
                    temperature_deci_c: temperature_deci_c(raw),
                    ..BatteryTelemetry::default()
                },
            };
            policy.tick(&mut controller, &battery).unwrap();
            assert_eq!(controller.0, enabled, "sensor value {raw}");
            assert!(!policy.runtime.temperature_unavailable);
        }
    }
}
