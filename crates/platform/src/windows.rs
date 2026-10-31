//! Windows battery monitoring and Lenovo Other Method WMI conservation control.
use crate::*;
mod power;
use serde::{Deserialize, Serialize};
use wmi::{WMIConnection, WMIError};
const CHARGE_MODE: u32 = 0x03010001;
fn error(e: WMIError) -> Error {
    match e {
        WMIError::HResultError { hres } if [0x80070005, 0x80041003].contains(&(hres as u32)) => {
            Error::PermissionDenied("WMI access denied".into())
        }
        WMIError::HResultError { hres } => {
            Error::BackendUnavailable(format!("WMI HRESULT 0x{:08x}", hres as u32))
        }
        _ => Error::PlatformError("WMI query or schema conversion failed".into()),
    }
}
#[derive(Deserialize)]
struct Computer {
    #[serde(rename = "Manufacturer")]
    vendor: String,
    #[serde(rename = "Model")]
    model: String,
}
#[derive(Deserialize)]
struct Os {
    #[serde(rename = "Version")]
    version: String,
}
#[derive(Deserialize)]
struct Battery {
    #[serde(rename = "EstimatedChargeRemaining")]
    percent: Option<u8>,
    #[serde(rename = "BatteryStatus")]
    state: Option<u16>,
    #[serde(rename = "EstimatedRunTime")]
    remaining: Option<u32>,
}
#[derive(Deserialize)]
struct CapData {
    #[serde(rename = "IDs")]
    id: u32,
    #[serde(rename = "MinValue")]
    min: u32,
    #[serde(rename = "MaxValue")]
    max: u32,
    #[serde(rename = "Step")]
    step: u32,
}
#[derive(Deserialize)]
#[serde(rename = "LENOVO_OTHER_METHOD")]
struct OtherMethod {
    #[serde(rename = "__Path")]
    path: String,
}
#[derive(Serialize)]
struct GetInput {
    #[serde(rename = "IDs")]
    id: u32,
}
#[derive(Serialize)]
struct SetInput {
    #[serde(rename = "IDs")]
    id: u32,
    value: u32,
}
#[derive(Deserialize)]
struct GetOutput {
    #[serde(rename = "Value")]
    value: u32,
}
#[derive(Deserialize)]
struct SetOutput {
    #[serde(rename = "ReturnValue")]
    code: u32,
}
struct Monitor;
impl BatteryMonitor for Monitor {
    fn batteries(&self) -> Result<Vec<BatteryStatus>> {
        let connection = WMIConnection::new().map_err(error)?;
        let ac = power::on_ac()?;
        let rows: Vec<Battery> = connection
            .raw_query("SELECT EstimatedChargeRemaining, BatteryStatus, EstimatedRunTime FROM Win32_Battery")
            .map_err(error)?;
        Ok(rows
            .into_iter()
            .enumerate()
            .map(|(i, b)| BatteryStatus {
                id: format!("BAT{i}"),
                percent: b.percent.filter(|v| *v <= 100),
                state: match b.state {
                    Some(3) => "full",
                    Some(6..=9) => "charging",
                    Some(4) => "low",
                    Some(5) => "critical",
                    _ => "unknown",
                }
                .into(),
                health_percent: None,
                on_ac: ac,
                hardware_start: None,
                hardware_stop: None,
                telemetry: BatteryTelemetry {
                    remaining_minutes: b.remaining.filter(|v| *v < 71_582_788),
                    ..BatteryTelemetry::default()
                },
            })
            .collect())
    }
}
fn probe_lenovo(connection: &WMIConnection) -> Result<()> {
    let class = connection
        .get_object("LENOVO_OTHER_METHOD")
        .map_err(error)?;
    for name in ["GetFeatureValue", "SetFeatureValue"] {
        if class.get_method(name).map_err(error)?.is_none() {
            return Err(Error::Unsupported(format!("WMI method {name} missing")));
        }
    }
    let data: Vec<CapData> = connection
        .raw_query("SELECT IDs, MinValue, MaxValue, Step FROM LENOVO_CAPABILITY_DATA_01")
        .map_err(error)?;
    if !data
        .iter()
        .any(|d| d.id == CHARGE_MODE && d.min == 0 && d.max == 1 && d.step == 1)
    {
        return Err(Error::Unsupported(
            "no boolean 0x03010001 capability record".into(),
        ));
    }
    let object: OtherMethod = connection
        .raw_query::<OtherMethod>("SELECT __Path FROM LENOVO_OTHER_METHOD")
        .map_err(error)?
        .into_iter()
        .next()
        .ok_or_else(|| Error::BackendUnavailable("WMI provider has no instance".into()))?;
    let out: GetOutput = connection
        .exec_instance_method::<OtherMethod, _>(
            &object.path,
            "GetFeatureValue",
            GetInput { id: CHARGE_MODE },
        )
        .map_err(error)?;
    if out.value > 1 {
        return Err(Error::Unsupported(
            "unknown Lenovo charge-mode encoding".into(),
        ));
    }
    Ok(())
}
pub fn discover() -> Result<Platform> {
    let connection = WMIConnection::new().map_err(error)?;
    let computer: Computer = connection
        .raw_query::<Computer>("SELECT Manufacturer, Model FROM Win32_ComputerSystem")
        .map_err(error)?
        .into_iter()
        .next()
        .ok_or_else(|| Error::BackendUnavailable("computer model missing".into()))?;
    let os: Os = connection
        .raw_query::<Os>("SELECT Version FROM Win32_OperatingSystem")
        .map_err(error)?
        .into_iter()
        .next()
        .ok_or_else(|| Error::BackendUnavailable("Windows version missing".into()))?;
    let device = DeviceInfo {
        os: "windows".into(),
        os_version: os.version,
        architecture: std::env::consts::ARCH.into(),
        vendor: computer.vendor,
        model: computer.model,
    };
    let mut interfaces = BTreeMap::from([("root\\cimv2:Win32_Battery".into(), "available".into())]);
    match WMIConnection::with_namespace_path("root\\WMI") {
        Ok(wmi) => {
            // Only class names from a fixed allowlist; never enumerate instance IDs.
            for class in [
                "LENOVO_OTHER_METHOD",
                "LENOVO_CAPABILITY_DATA_00",
                "LENOVO_CAPABILITY_DATA_01",
                "BatteryStatus",
                "BatteryFullChargedCapacity",
            ] {
                interfaces.insert(
                    format!("root\\WMI:{class}"),
                    if wmi.get_object(class).is_ok() {
                        "available".into()
                    } else {
                        "absent or inaccessible".into()
                    },
                );
            }
            if device.vendor.to_lowercase().contains("lenovo") {
                match probe_lenovo(&wmi) {
                    Ok(()) => {
                        interfaces.insert("WMI:LENOVO_OTHER_METHOD".into(), "available".into());
                        interfaces.insert("WMI:capdata:0x03010001".into(), "available".into());
                    }
                    Err(e) => {
                        interfaces.insert("WMI:Lenovo probe".into(), e.to_string());
                    }
                }
            }
        }
        Err(e) => {
            interfaces.insert("root\\WMI".into(), error(e).to_string());
        }
    }
    if device.vendor.to_lowercase().contains("lenovo") {
        let exported = WMIConnection::with_namespace_path("root\\Lenovo")
            .and_then(|wmi| wmi.get_object("Lenovo_Battery"));
        interfaces.insert(
            "root\\Lenovo:Lenovo_Battery".into(),
            if exported.is_ok() {
                "available (Vantage information export; not a threshold setter)".into()
            } else {
                "absent or inaccessible".into()
            },
        );
    }
    let monitor = Monitor;
    let batteries = monitor.batteries()?;
    if batteries.len() != 1 {
        interfaces.remove("WMI:capdata:0x03010001");
        interfaces.insert(
            "WMI:Lenovo battery scope".into(),
            "Lenovo conservation control requires exactly one present battery".into(),
        );
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
pub fn controller(device: &DeviceInfo) -> Result<Box<dyn ChargeController>> {
    if !device.vendor.to_lowercase().contains("lenovo") {
        return Err(Error::Unsupported(
            "no controller for this Windows vendor".into(),
        ));
    }
    let caps = discover()?.report.capabilities;
    batterybridge_core::policy::require(&caps.charge_limit)?;
    let connection = WMIConnection::with_namespace_path("root\\WMI").map_err(error)?;
    probe_lenovo(&connection)?;
    let object: OtherMethod = connection
        .raw_query::<OtherMethod>("SELECT __Path FROM LENOVO_OTHER_METHOD")
        .map_err(error)?
        .into_iter()
        .next()
        .ok_or_else(|| Error::BackendUnavailable("Lenovo WMI instance missing".into()))?;
    Ok(Box::new(Controller {
        connection,
        path: object.path,
        caps,
    }))
}
struct Controller {
    connection: WMIConnection,
    path: String,
    caps: Capabilities,
}
impl ChargeController for Controller {
    fn capabilities(&self) -> Capabilities {
        self.caps.clone()
    }
    fn apply(&mut self, start: Option<u8>, stop: Option<u8>) -> Result<()> {
        Command::Limit { start, stop }.validate()?;
        batterybridge_core::policy::require(&self.caps.charge_limit)?;
        if start.is_some() || stop.is_some_and(|v| v != 80) {
            return Err(Error::Unsupported(
                "this Lenovo WMI backend accepts only limit 80 or off".into(),
            ));
        }
        let result: SetOutput = self
            .connection
            .exec_instance_method::<OtherMethod, _>(
                &self.path,
                "SetFeatureValue",
                SetInput {
                    id: CHARGE_MODE,
                    value: u32::from(stop.is_some()),
                },
            )
            .map_err(error)?;
        if result.code > 1 {
            return Err(Error::HardwareError(format!(
                "Lenovo WMI returned {}",
                result.code
            )));
        }
        // The getter can describe inhibition rather than configured mode, so it
        // cannot provide reliable readback of the requested numerical limit.
        Ok(())
    }
    /// Conservation mode is a firmware setting with no daemon-held companion.
    /// Its survival across power-off is not established here (unknown), and a
    /// service stop never reaches this call on Windows: Task Scheduler ends the
    /// process without notice. `Overrides` therefore has nothing to give back.
    fn release(&mut self, scope: Release) -> Result<()> {
        match scope {
            Release::Overrides => Ok(()),
            Release::Everything => self.apply(None, None),
        }
    }
}
