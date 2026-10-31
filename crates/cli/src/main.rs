#![forbid(unsafe_code)]
use batterybridge_core::*;
use batterybridge_ipc as ipc;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "batterybridge",
    version,
    about = "Inspect battery status and manage device-supported charging limits"
)]
struct Args {
    #[command(subcommand)]
    command: CliCommand,
    /// Print the result as JSON.
    #[arg(long, global = true)]
    json: bool,
    /// Increase log verbosity (-v or -vv).
    #[arg(short,global=true,action=clap::ArgAction::Count)]
    verbose: u8,
}
#[derive(Subcommand)]
enum CliCommand {
    /// Show battery status and the service's charging policy.
    Status,
    /// List charging features available on this device.
    Capabilities,
    /// Generate a read-only device compatibility report.
    Probe,
    /// Display the service's saved TOML configuration.
    Config,
    /// Set a charge limit or restore normal charging. Requires the service.
    Limit {
        /// Stop percentage (1..100), or 'off'.
        value: String,
        /// Resume percentage, lower than the stop percentage.
        #[arg(long)]
        start: Option<u8>,
    },
    /// Temporarily charge to 100%, then restore the saved limit.
    TopUp,
    /// Charge to a temporary target and hold it until cancelled. Saved limit is unchanged.
    ChargeTo { target: u8 },
    /// Stop charging immediately, preserving the saved policy.
    Hold,
    /// Release hardware controls temporarily, preserving all saved settings.
    Pause,
    /// Restore the saved policy and cancel temporary operations.
    Resume,
    /// Cancel a temporary operation and restore the saved policy.
    Cancel,
    /// Restore normal charging and permanently disable all limits, care and schedules.
    Reset,
    /// Run a 100% -> 10% -> 100% cycle, rest for one hour, then restore the saved policy.
    Calibrate,
    /// Disable the adapter until 20%, or restore the saved policy ('on').
    Adapter { value: String },
    /// Display care settings, or replace them from a JSON file (including schedules).
    Care {
        #[arg(long)]
        file: Option<std::path::PathBuf>,
    },
    /// Discharge to a target when the device supports force discharge.
    Discharge {
        /// Target percentage (20..100).
        target: u8,
    },
}
fn run() -> Result<()> {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                match args.verbose {
                    0 => "warn",
                    1 => "info",
                    _ => "debug",
                }
                .into()
            }),
        )
        .with_writer(std::io::stderr)
        .init();
    let command = match args.command {
        CliCommand::Status => Command::Status,
        CliCommand::Capabilities => Command::Capabilities,
        CliCommand::Probe => Command::Probe,
        CliCommand::Config => Command::Config,
        CliCommand::Limit { value, start } => Command::Limit {
            stop: if value == "off" {
                None
            } else {
                Some(
                    value
                        .parse()
                        .map_err(|_| Error::InvalidLimit("expected 1..100 or off".into()))?,
                )
            },
            start,
        },
        CliCommand::TopUp => Command::TopUp,
        CliCommand::ChargeTo { target } => Command::ChargeTo { target },
        CliCommand::Hold => Command::StopCharging,
        CliCommand::Pause => Command::Pause,
        CliCommand::Resume => Command::Resume,
        CliCommand::Cancel => Command::Cancel,
        CliCommand::Reset => Command::Reset,
        CliCommand::Calibrate => Command::Calibrate,
        CliCommand::Adapter { value } => Command::Adapter {
            enabled: match value.as_str() {
                "on" => true,
                "off" => false,
                _ => return Err(Error::ProtocolError("adapter expects on or off".into())),
            },
        },
        CliCommand::Care { file } => {
            let Response::Config(config) =
                batterybridge_platform::client::query(Command::Config, ipc::call)?
            else {
                return Err(Error::ProtocolError("expected configuration".into()));
            };
            let Some(file) = file else {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&config.care)
                        .map_err(|e| Error::ProtocolError(e.to_string()))?
                );
                return Ok(());
            };
            let source = std::fs::read(file).map_err(|e| Error::io("read care settings", e))?;
            let settings = serde_json::from_slice(&source)
                .map_err(|e| Error::ConfigurationError(e.to_string()))?;
            Command::Care {
                settings,
                expected: config.care,
            }
        }
        CliCommand::Discharge { target } => Command::Discharge { target },
    };
    command.validate()?;
    output(
        batterybridge_platform::client::query(command, ipc::call)?,
        args.json,
    )
}
fn output(response: Response, json: bool) -> Result<()> {
    if json {
        let value = match response {
            Response::Probe(v) => serde_json::to_value(v),
            Response::Status(v) => serde_json::to_value(v),
            Response::Capabilities(v) => serde_json::to_value(v),
            Response::Config(v) => serde_json::to_value(v),
            v => serde_json::to_value(v),
        }
        .map_err(|e| Error::ProtocolError(e.to_string()))?;
        println!(
            "{}",
            serde_json::to_string_pretty(&value)
                .map_err(|e| Error::ProtocolError(e.to_string()))?
        );
        return Ok(());
    }
    match response {
        Response::Ok => println!("Applied."),
        Response::Error(e) => return Err(e),
        Response::Status(s) => {
            println!("BatteryBridge\n");
            if s.batteries.is_empty() {
                println!("Battery:      not detected");
            }
            for b in &s.batteries {
                println!(
                    "Battery {}: {}\nState:        {}\nPower source: {}\nHealth:       {}",
                    b.id,
                    percentage(b.percent),
                    b.state,
                    match b.on_ac {
                        Some(true) => "AC",
                        Some(false) => "Battery",
                        None => "unknown",
                    },
                    percentage(b.health_percent)
                );
                if b.hardware_stop.is_some() || b.hardware_start.is_some() {
                    println!(
                        "Device threshold: stop {}, start {} (read from the device)",
                        percentage(b.hardware_stop),
                        percentage(b.hardware_start)
                    );
                }
            }
            println!(
                "\nSaved charge limit (first battery, configuration): {}",
                s.config
                    .as_ref()
                    .map(|c| c
                        .charge_limit
                        .map(|v| format!("{v}%"))
                        .unwrap_or_else(|| "off".into()))
                    .unwrap_or_else(|| "unavailable".into())
            );
            println!(
                "Top up:       {}\n\nBackend:      {}\nDevice:       {} {}",
                if s.config.is_some() {
                    s.top_up_active.to_string()
                } else {
                    "unavailable".into()
                },
                s.backend,
                s.device.vendor,
                s.device.model
            );
            println!(
                "Runtime: {}",
                serde_json::to_string(&s.runtime)
                    .map_err(|e| Error::ProtocolError(e.to_string()))?
            );
            for b in &s.batteries {
                println!(
                    "Telemetry {}: {}",
                    b.id,
                    serde_json::to_string(&b.telemetry)
                        .map_err(|e| Error::ProtocolError(e.to_string()))?
                );
            }
            if let Some(e) = s.last_error {
                println!("\nService: {e}");
            }
        }
        Response::Capabilities(c) => {
            for (name, cap) in c.entries() {
                println!("{name:20} {:12} {}", state(cap.state), cap.reason);
            }
            if let Some(v) = c.allowed_limits {
                println!("Allowed limits: {v:?}");
            }
        }
        Response::Probe(p) => {
            println!(
                "BatteryBridge compatibility report v{}\nOS: {} {} ({})\nDevice: {} {}\nBatteries: {:?}",
                p.schema_version,
                p.device.os,
                p.device.os_version,
                p.device.architecture,
                p.device.vendor,
                p.device.model,
                p.battery_ids
            );
            for (key, value) in p.interfaces {
                println!("{key}: {value}");
            }
            for c in p.candidates {
                println!(
                    "Backend {}: {} ({})",
                    c.name,
                    if c.selected { "selected" } else { "inactive" },
                    c.reason
                );
            }
            output(Response::Capabilities(Box::new(p.capabilities)), false)?;
        }
        Response::Config(c) => println!(
            "{}",
            toml::to_string_pretty(&c).map_err(|e| Error::ConfigurationError(e.to_string()))?
        ),
    }
    Ok(())
}
fn percentage(p: Option<u8>) -> String {
    p.map(|v| format!("{v}%"))
        .unwrap_or_else(|| "unknown".into())
}
fn state(s: CapabilityState) -> &'static str {
    match s {
        CapabilityState::Supported => "supported",
        CapabilityState::Experimental => "experimental",
        CapabilityState::Unsupported => "unsupported",
        CapabilityState::Unknown => "unknown",
    }
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
