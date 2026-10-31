//! Lifecycle sequences against fake backends that differ in *mechanism*, not vendor:
//! a native threshold, a daemon-held binary gate, and both together. These tests
//! establish software ordering and meaning of hardware commands. They say nothing
//! about what real firmware does while the computer is off.
use super::*;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

static COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    /// Kernel/firmware end threshold only (Linux sysfs, Lenovo WMI shape).
    Threshold,
    /// SMC-style binary gate plus discharge, LED and idle-sleep overrides.
    Gate,
    /// Intel Mac shape: firmware threshold together with a daemon-held gate.
    Both,
}
struct Hw {
    threshold: Option<u8>,
    gate_open: bool,
    discharging: bool,
    led: LedMode,
    awake: bool,
    ops: Vec<String>,
    failing: Vec<&'static str>,
}
impl Hw {
    fn new() -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self {
            threshold: None,
            gate_open: true,
            discharging: false,
            led: LedMode::System,
            awake: false,
            ops: vec![],
            failing: vec![],
        }))
    }
    fn refuse(&self, what: &'static str) -> Result<()> {
        if self.failing.contains(&what) {
            Err(Error::HardwareError(format!("{what} refused")))
        } else {
            Ok(())
        }
    }
    fn wrote_threshold_none(&self) -> bool {
        self.ops.iter().any(|op| op == "threshold=None")
    }
}
struct Backend {
    kind: Kind,
    hw: Rc<RefCell<Hw>>,
}
impl Backend {
    fn gated(&self) -> bool {
        self.kind != Kind::Threshold
    }
    fn native(&self) -> bool {
        self.kind != Kind::Gate
    }
}
impl ChargeController for Backend {
    fn capabilities(&self) -> Capabilities {
        let mut c = Capabilities::unsupported("fixture");
        c.charge_limit = Capability::supported("fixture");
        c.stop_threshold = c.charge_limit.clone();
        c.temporary_full_charge = c.charge_limit.clone();
        if self.gated() {
            c.start_threshold = c.charge_limit.clone();
            c.manual_charging = c.charge_limit.clone();
            c.force_discharge = c.charge_limit.clone();
            c.adapter_disable = c.charge_limit.clone();
            c.magsafe_led = c.charge_limit.clone();
            c.sleep_prevention = c.charge_limit.clone();
        }
        c
    }
    fn apply(&mut self, _: Option<u8>, stop: Option<u8>) -> Result<()> {
        let mut hw = self.hw.borrow_mut();
        if self.native() {
            hw.refuse("threshold")?;
            hw.ops.push(format!("threshold={stop:?}"));
            hw.threshold = stop;
        } else if stop.is_none() {
            hw.refuse("gate")?;
            hw.ops.push("gate=open".into());
            hw.gate_open = true;
        }
        Ok(())
    }
    fn charging(&mut self, enabled: bool) -> Result<()> {
        if !self.gated() {
            return Ok(());
        }
        let mut hw = self.hw.borrow_mut();
        hw.refuse("gate")?;
        hw.ops
            .push(format!("gate={}", if enabled { "open" } else { "closed" }));
        hw.gate_open = enabled;
        Ok(())
    }
    fn force_discharge(&mut self, enabled: bool) -> Result<()> {
        if !self.gated() {
            return Err(Error::Unsupported("force discharge".into()));
        }
        let mut hw = self.hw.borrow_mut();
        hw.refuse("discharge")?;
        hw.ops.push(format!("discharge={enabled}"));
        hw.discharging = enabled;
        Ok(())
    }
    fn magsafe_led(&mut self, mode: LedMode) -> Result<()> {
        let mut hw = self.hw.borrow_mut();
        hw.refuse("led")?;
        hw.ops.push(format!("led={mode:?}"));
        hw.led = mode;
        Ok(())
    }
    fn prevent_sleep(&mut self, enabled: bool) -> Result<()> {
        let mut hw = self.hw.borrow_mut();
        hw.refuse("awake")?;
        hw.ops.push(format!("awake={enabled}"));
        hw.awake = enabled;
        Ok(())
    }
    fn release(&mut self, scope: Release) -> Result<()> {
        let mut hw = self.hw.borrow_mut();
        hw.ops.push(format!("release:{scope:?}"));
        let mut steps = vec![];
        if self.gated() {
            let awake = hw.refuse("awake");
            if awake.is_ok() {
                hw.awake = false;
            }
            let led = hw.refuse("led");
            if led.is_ok() {
                hw.led = LedMode::System;
            }
            let discharge = hw.refuse("discharge");
            if discharge.is_ok() {
                hw.discharging = false;
            }
            let gate = hw.refuse("gate");
            if gate.is_ok() {
                hw.gate_open = true;
            }
            steps.extend([
                ("idle-sleep assertion", awake),
                ("MagSafe LED", led),
                ("forced discharge", discharge),
                ("charging gate", gate),
            ]);
        }
        if self.native() && scope == Release::Everything {
            let threshold = hw.refuse("threshold");
            if threshold.is_ok() {
                hw.ops.push("threshold=None".into());
                hw.threshold = None;
            }
            steps.push(("threshold", threshold));
        }
        merge_steps(steps)
    }
}
struct Pct(Rc<Cell<u8>>);
impl BatteryMonitor for Pct {
    fn batteries(&self) -> Result<Vec<BatteryStatus>> {
        Ok(vec![BatteryStatus {
            id: "BAT0".into(),
            percent: (self.0.get() <= 100).then_some(self.0.get()),
            state: "charging".into(),
            on_ac: Some(true),
            health_percent: None,
            hardware_start: None,
            hardware_stop: None,
            telemetry: BatteryTelemetry::default(),
        }])
    }
}
struct Rig {
    service: Service,
    kind: Kind,
    hw: Rc<RefCell<Hw>>,
    percent: Rc<Cell<u8>>,
    path: PathBuf,
}
impl Rig {
    fn new(kind: Kind, percent: u8) -> Self {
        let root = std::env::temp_dir().join(format!(
            "batterybridge-lifecycle-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let path = root.join("config.toml");
        let hw = Hw::new();
        let percent = Rc::new(Cell::new(percent));
        let service = build(kind, &path, &hw, &percent);
        Self {
            service,
            kind,
            hw,
            percent,
            path,
        }
    }
    /// A new process over the same saved config and the same hardware.
    fn restart(&self) -> Service {
        build(self.kind, &self.path, &self.hw, &self.percent)
    }
    fn send(&mut self, command: Command) -> Response {
        self.service.handle(Request::new(command))
    }
    fn expect_ok(&mut self, command: Command) {
        let response = self.send(command);
        assert!(matches!(response, Response::Ok), "{response:?}");
    }
    fn limit(&mut self, stop: u8) {
        self.expect_ok(Command::Limit {
            stop: Some(stop),
            start: None,
        });
    }
    fn saved(&self) -> Config {
        config::load(&self.path).unwrap()
    }
    fn status(&mut self) -> Snapshot {
        let Response::Status(snapshot) = self.send(Command::Status) else {
            panic!("status");
        };
        *snapshot
    }
    fn clear_ops(&self) {
        self.hw.borrow_mut().ops.clear();
    }
    fn fail(&self, what: &[&'static str]) {
        self.hw.borrow_mut().failing = what.to_vec();
    }
}
impl Drop for Rig {
    fn drop(&mut self) {
        if let Some(root) = self.path.parent() {
            let _ = std::fs::remove_dir_all(root);
        }
    }
}
fn build(kind: Kind, path: &Path, hw: &Rc<RefCell<Hw>>, percent: &Rc<Cell<u8>>) -> Service {
    let device = DeviceInfo {
        os: "test".into(),
        os_version: "test".into(),
        architecture: "fixture".into(),
        vendor: "fixture".into(),
        model: "fixture".into(),
    };
    let controller = Backend {
        kind,
        hw: hw.clone(),
    };
    let report = ProbeReport {
        schema_version: 1,
        device: device.clone(),
        battery_ids: vec!["BAT0".into()],
        interfaces: BTreeMap::new(),
        candidates: vec![],
        capabilities: controller.capabilities(),
    };
    Service {
        platform: Platform {
            monitor: Box::new(Pct(percent.clone())),
            device,
            report,
        },
        controller: Some(Box::new(controller)),
        policy: Policy::new(config::load(path).unwrap()).unwrap(),
        path: path.to_path_buf(),
        last_error: None,
        sleep_error: None,
        schedule_error: None,
        stopping: false,
        awaiting_wake: false,
    }
}
use std::path::{Path, PathBuf};

fn due_limit(at: i64) -> Schedule {
    Schedule {
        id: 1,
        enabled: true,
        next_at: at,
        anchor_at: None,
        repeat: Repeat::Once,
        action: ScheduleAction::Limit {
            stop: 60,
            start: None,
        },
    }
}

// 1. limit -> shutdown
#[test]
fn shutdown_keeps_a_saved_native_threshold_and_releases_only_daemon_held_state() {
    for kind in [Kind::Threshold, Kind::Both] {
        let mut rig = Rig::new(kind, 74);
        rig.limit(80);
        assert_eq!(rig.hw.borrow().threshold, Some(80));
        rig.clear_ops();
        rig.service.shutdown().unwrap();
        let hw = rig.hw.borrow();
        assert_eq!(hw.threshold, Some(80), "{kind:?}");
        assert!(!hw.wrote_threshold_none(), "{kind:?}: {:?}", hw.ops);
        assert!(hw.gate_open);
        drop(hw);
        assert_eq!(rig.saved().charge_limit, Some(80));
    }
}
#[test]
fn shutdown_always_reopens_a_daemon_dependent_gate() {
    let mut rig = Rig::new(Kind::Gate, 90);
    rig.limit(80);
    assert!(
        !rig.hw.borrow().gate_open,
        "above the limit the gate is closed"
    );
    rig.service.shutdown().unwrap();
    // Nothing maintains a binary gate once the daemon is gone: leaving it closed
    // would stop all charging, so it is always reopened. The limit stays saved.
    assert!(rig.hw.borrow().gate_open);
    assert_eq!(rig.saved().charge_limit, Some(80));
}

// 2. limit -> restart -> recovery
#[test]
fn restart_reapplies_the_saved_limit_without_passing_through_unrestricted_charging() {
    let mut rig = Rig::new(Kind::Threshold, 74);
    rig.limit(80);
    rig.service.shutdown().unwrap();
    rig.clear_ops();
    let mut restored = rig.restart();
    restored.tick();
    assert_eq!(rig.hw.borrow().threshold, Some(80));
    assert!(
        !rig.hw.borrow().wrote_threshold_none(),
        "{:?}",
        rig.hw.borrow().ops
    );
    // The saved value is reported as configuration; the live status carries no error.
    let Response::Status(status) = restored.handle(Request::new(Command::Status)) else {
        panic!("status");
    };
    assert_eq!(status.config.unwrap().charge_limit, Some(80));
    assert_eq!(status.last_error, None);
}
#[test]
fn restart_surfaces_a_hardware_write_failure_instead_of_reporting_the_saved_limit_as_applied() {
    let mut rig = Rig::new(Kind::Threshold, 74);
    rig.limit(80);
    rig.service.shutdown().unwrap();
    rig.fail(&["threshold"]);
    let mut restored = rig.restart();
    restored.tick();
    let Response::Status(status) = restored.handle(Request::new(Command::Status)) else {
        panic!("status");
    };
    assert!(matches!(status.last_error, Some(Error::HardwareError(_))));
    assert_eq!(status.config.unwrap().charge_limit, Some(80));
}

// 3. temporary mode -> shutdown
#[test]
fn shutdown_ends_temporary_modes_and_restores_the_saved_threshold_not_the_temporary_one() {
    for (kind, command) in [
        (Kind::Threshold, Command::TopUp),
        (Kind::Threshold, Command::ChargeTo { target: 95 }),
        (Kind::Both, Command::TopUp),
        (Kind::Both, Command::ChargeTo { target: 95 }),
    ] {
        let mut rig = Rig::new(kind, 74);
        rig.limit(80);
        rig.expect_ok(command.clone());
        let temporary = rig.hw.borrow().threshold;
        assert_ne!(
            temporary,
            Some(80),
            "{command:?} must override the saved limit"
        );
        rig.service.shutdown().unwrap();
        assert_eq!(rig.hw.borrow().threshold, Some(80), "{kind:?} {command:?}");
        assert!(rig.hw.borrow().gate_open);
        let mut restored = rig.restart();
        assert!(!restored.policy.top_up && restored.policy.runtime.charge_target.is_none());
        restored.tick();
        assert_eq!(rig.hw.borrow().threshold, Some(80));
    }
}
#[test]
fn shutdown_stops_forced_discharge_calibration_hold_and_auxiliary_overrides() {
    for command in [
        Command::Discharge { target: 50 },
        Command::Calibrate,
        Command::StopCharging,
    ] {
        let mut rig = Rig::new(Kind::Gate, 74);
        rig.limit(80);
        let mut care = rig.saved().care;
        care.led = LedMode::Orange;
        care.prevent_sleep = true;
        rig.expect_ok(Command::Care {
            settings: care.clone(),
            expected: CareSettings::default(),
        });
        rig.expect_ok(command.clone());
        rig.service.tick();
        rig.service.shutdown().unwrap();
        let hw = rig.hw.borrow();
        assert!(!hw.discharging, "{command:?}");
        assert!(hw.gate_open, "{command:?}");
        assert!(!hw.awake, "{command:?}");
        assert_eq!(hw.led, LedMode::System, "{command:?}");
        drop(hw);
        // Intent survives; temporary state does not.
        assert_eq!(rig.saved().charge_limit, Some(80));
        assert_eq!(rig.saved().care, care);
        let restored = rig.restart();
        assert_eq!(restored.policy.discharge_target, None);
        assert!(restored.policy.runtime.calibration.is_none() && !restored.policy.runtime.holding);
    }
}

// 4. prepare_sleep -> wake
#[test]
fn sleep_closes_a_gate_keeps_a_native_threshold_and_wake_reverifies_before_reporting() {
    let mut rig = Rig::new(Kind::Gate, 78);
    rig.limit(80);
    rig.expect_ok(Command::Discharge { target: 50 });
    rig.service.tick();
    assert!(rig.hw.borrow().discharging);
    rig.service.prepare_sleep().unwrap();
    {
        let hw = rig.hw.borrow();
        assert!(!hw.discharging && !hw.awake);
        assert!(!hw.gate_open, "inhibit while the daemon cannot enforce");
    }
    assert!(matches!(
        rig.status().last_error,
        Some(Error::PlatformError(message)) if message.contains("not yet verified")
    ));
    // Firmware reset the gate during sleep. Wake must rewrite it from the policy.
    rig.hw.borrow_mut().gate_open = true;
    rig.service.wake();
    assert!(
        !rig.hw.borrow().gate_open,
        "78% is above the 75% resume threshold"
    );
    assert_eq!(rig.status().last_error, None);
}
#[test]
fn sleep_does_not_touch_a_native_threshold_and_wake_repairs_a_firmware_reset() {
    let mut rig = Rig::new(Kind::Threshold, 74);
    rig.limit(80);
    rig.clear_ops();
    rig.service.prepare_sleep().unwrap();
    assert!(rig.hw.borrow().ops.is_empty(), "{:?}", rig.hw.borrow().ops);
    assert_eq!(rig.hw.borrow().threshold, Some(80));
    rig.hw.borrow_mut().threshold = None; // firmware dropped it
    rig.service.wake();
    assert_eq!(rig.hw.borrow().threshold, Some(80));
    assert_eq!(rig.status().last_error, None);
}
#[test]
fn sleep_cleanup_steps_are_independent_and_the_failure_stays_visible_after_wake() {
    let mut rig = Rig::new(Kind::Gate, 90);
    rig.limit(80);
    rig.expect_ok(Command::Discharge { target: 85 });
    rig.service.tick();
    rig.fail(&["discharge"]);
    let error = rig.service.prepare_sleep().unwrap_err();
    assert!(error.to_string().contains("forced discharge"), "{error}");
    assert!(
        !rig.hw.borrow().gate_open,
        "gate inhibition still attempted"
    );
    rig.fail(&[]);
    rig.service.wake();
    assert!(matches!(
        rig.status().last_error,
        Some(Error::PlatformError(m)) if m.contains("Sleep preparation failed")
    ));
}

// 5. pause -> resume
#[test]
fn pause_releases_everything_keeps_the_saved_policy_and_resume_restores_it() {
    let mut rig = Rig::new(Kind::Both, 74);
    rig.limit(80);
    rig.expect_ok(Command::Pause);
    {
        let hw = rig.hw.borrow();
        assert_eq!(hw.threshold, None, "pause lifts the native limit too");
        assert!(hw.gate_open);
    }
    assert!(rig.status().runtime.paused);
    assert_eq!(rig.saved().charge_limit, Some(80));
    rig.expect_ok(Command::Resume);
    assert_eq!(rig.hw.borrow().threshold, Some(80));
    assert!(!rig.status().runtime.paused);
}
#[test]
fn stopping_while_paused_honours_the_pause_and_the_next_start_resumes_control() {
    let mut rig = Rig::new(Kind::Threshold, 74);
    rig.limit(80);
    rig.expect_ok(Command::Pause);
    rig.service.shutdown().unwrap();
    assert_eq!(rig.hw.borrow().threshold, None);
    let mut restored = rig.restart();
    assert!(!restored.policy.runtime.paused, "pause is runtime-only");
    restored.tick();
    assert_eq!(rig.hw.borrow().threshold, Some(80));
}

// 6. reset -> stop, 7. uninstall preparation -> stop
#[test]
fn reset_returns_every_mechanism_and_a_later_stop_cannot_reimpose_anything() {
    let mut rig = Rig::new(Kind::Both, 90);
    rig.limit(80);
    let mut care = rig.saved().care;
    care.led = LedMode::Orange;
    care.prevent_sleep = true;
    care.schedules.push(due_limit(100));
    rig.expect_ok(Command::Care {
        settings: care,
        expected: CareSettings::default(),
    });
    rig.service.tick();
    rig.expect_ok(Command::Reset);
    rig.clear_ops();
    // The scheduler runs after reset and before the process stops.
    rig.service.run_schedules(10_000).unwrap();
    rig.service.tick();
    rig.service.shutdown().unwrap();
    {
        let hw = rig.hw.borrow();
        assert_eq!(hw.threshold, None);
        assert!(hw.gate_open && !hw.discharging && !hw.awake);
        assert_eq!(hw.led, LedMode::System);
        assert!(
            !hw.ops
                .iter()
                .any(|op| op == "threshold=Some(80)" || op == "threshold=Some(60)"),
            "{:?}",
            hw.ops
        );
    }
    assert_eq!(rig.saved(), Config::default());
    let mut restored = rig.restart();
    restored.tick();
    assert_eq!(rig.hw.borrow().threshold, None);
}
#[test]
fn reset_is_acknowledged_only_after_hardware_accepted_it_and_does_not_hide_a_failure() {
    let mut rig = Rig::new(Kind::Both, 74);
    rig.limit(80);
    rig.fail(&["threshold"]);
    let response = rig.send(Command::Reset);
    let Response::Error(Error::HardwareError(message)) = response else {
        panic!("reset must fail: {response:?}");
    };
    assert!(message.contains("threshold"));
    // Not durable: a saved limit that is still on the hardware must stay saved.
    assert_eq!(rig.saved().charge_limit, Some(80));
    assert_eq!(rig.service.policy.config.charge_limit, Some(80));
}
#[test]
fn reset_reports_every_failed_step_and_keeps_the_original_error_kind() {
    let mut rig = Rig::new(Kind::Both, 74);
    rig.limit(80);
    rig.fail(&["gate", "threshold"]);
    let Response::Error(error) = rig.send(Command::Reset) else {
        panic!("reset must fail");
    };
    assert!(matches!(error, Error::HardwareError(_)));
    let text = error.to_string();
    assert!(
        text.contains("charging gate") && text.contains("threshold"),
        "{text}"
    );
}

// 8. repeated shutdown and reset
#[test]
fn repeated_shutdown_and_reset_converge_on_the_same_hardware_state() {
    let mut rig = Rig::new(Kind::Both, 74);
    rig.limit(80);
    rig.service.shutdown().unwrap();
    let first = (rig.hw.borrow().threshold, rig.hw.borrow().gate_open);
    rig.service.shutdown().unwrap();
    assert_eq!(
        (rig.hw.borrow().threshold, rig.hw.borrow().gate_open),
        first
    );
    assert_eq!(first, (Some(80), true));
    let mut rig = Rig::new(Kind::Both, 74);
    rig.limit(80);
    rig.expect_ok(Command::Reset);
    rig.expect_ok(Command::Reset);
    rig.service.shutdown().unwrap();
    rig.service.shutdown().unwrap();
    assert_eq!(rig.hw.borrow().threshold, None);
    assert_eq!(rig.saved(), Config::default());
}
#[test]
fn a_failed_shutdown_can_be_retried_and_then_completes() {
    let mut rig = Rig::new(Kind::Both, 74);
    rig.limit(80);
    rig.expect_ok(Command::TopUp);
    rig.fail(&["threshold"]);
    assert!(rig.service.shutdown().is_err());
    rig.fail(&[]);
    rig.service.shutdown().unwrap();
    assert_eq!(rig.hw.borrow().threshold, Some(80));
}

// 9. partial cleanup failure
#[test]
fn one_failed_cleanup_step_does_not_skip_the_independent_ones() {
    let mut rig = Rig::new(Kind::Both, 74);
    rig.limit(80);
    let mut care = rig.saved().care;
    care.led = LedMode::Orange;
    rig.expect_ok(Command::Care {
        settings: care,
        expected: CareSettings::default(),
    });
    rig.expect_ok(Command::TopUp);
    rig.service.tick();
    rig.expect_ok(Command::Discharge { target: 50 });
    rig.service.tick();
    rig.fail(&["gate"]);
    let error = rig.service.shutdown().unwrap_err();
    assert!(matches!(error, Error::HardwareError(_)));
    assert!(error.to_string().contains("charging gate"), "{error}");
    let hw = rig.hw.borrow();
    assert!(
        !hw.discharging,
        "discharge released despite the gate failure"
    );
    assert_eq!(hw.led, LedMode::System);
    assert_eq!(
        hw.threshold,
        Some(80),
        "saved threshold restored despite the gate failure"
    );
}
#[test]
fn several_failures_are_all_named_in_the_shutdown_error() {
    let mut rig = Rig::new(Kind::Both, 74);
    rig.limit(80);
    rig.fail(&["gate", "led", "threshold"]);
    let text = rig.service.shutdown().unwrap_err().to_string();
    for step in ["MagSafe LED", "charging gate", "threshold"] {
        assert!(text.contains(step), "{text}");
    }
}

// 10. stop during a policy change
#[test]
fn nothing_may_start_after_shutdown_begins() {
    let mut rig = Rig::new(Kind::Threshold, 74);
    rig.limit(80);
    let mut care = rig.saved().care;
    care.schedules.push(due_limit(100));
    rig.expect_ok(Command::Care {
        settings: care,
        expected: CareSettings::default(),
    });
    rig.service.shutdown().unwrap();
    rig.clear_ops();
    for command in [
        Command::Limit {
            stop: Some(70),
            start: None,
        },
        Command::Reset,
        Command::TopUp,
        Command::Status,
    ] {
        assert!(matches!(
            rig.send(command),
            Response::Error(Error::ServiceUnavailable(_))
        ));
    }
    rig.service.tick();
    assert!(rig.service.prepare_sleep().is_ok());
    assert!(rig.hw.borrow().ops.is_empty(), "{:?}", rig.hw.borrow().ops);
    assert_eq!(rig.saved().charge_limit, Some(80));
    // The due schedule was neither executed nor consumed.
    assert!(rig.saved().care.schedules[0].enabled);
}
#[test]
fn a_change_whose_persistence_failed_is_rolled_back_before_shutdown_settles() {
    let mut rig = Rig::new(Kind::Threshold, 74);
    std::fs::create_dir_all(&rig.path).unwrap(); // saving must fail
    assert!(matches!(
        rig.send(Command::Limit {
            stop: Some(80),
            start: None
        }),
        Response::Error(_)
    ));
    rig.service.shutdown().unwrap();
    assert_eq!(
        rig.hw.borrow().threshold,
        None,
        "rejected limit must not survive the stop"
    );
    assert_eq!(rig.service.policy.config.charge_limit, None);
}

// capabilities and monitor-only installations
#[test]
fn without_a_controller_shutdown_touches_nothing_and_succeeds() {
    let mut rig = Rig::new(Kind::Threshold, 74);
    rig.service.controller = None;
    rig.service.platform.report.capabilities.charge_limit = Capability::unknown("access denied");
    rig.service.shutdown().unwrap();
    assert!(rig.hw.borrow().ops.is_empty());
}
#[test]
fn a_native_only_backend_has_nothing_to_release_under_overrides() {
    let mut rig = Rig::new(Kind::Threshold, 74);
    rig.limit(80);
    rig.clear_ops();
    let mut backend = Backend {
        kind: Kind::Threshold,
        hw: rig.hw.clone(),
    };
    backend.release(Release::Overrides).unwrap();
    assert_eq!(rig.hw.borrow().threshold, Some(80));
    backend.release(Release::Everything).unwrap();
    assert_eq!(rig.hw.borrow().threshold, None);
}

const UNREADABLE: u8 = 255;

// A failed reconciliation must not clear a saved native threshold.
#[test]
fn an_unreadable_percentage_keeps_the_native_threshold_and_still_reopens_gates() {
    for kind in [Kind::Threshold, Kind::Both] {
        let mut rig = Rig::new(kind, 74);
        rig.limit(80);
        rig.percent.set(UNREADABLE);
        rig.clear_ops();
        rig.service.tick();
        let hw = rig.hw.borrow();
        assert_eq!(hw.threshold, Some(80), "{kind:?}: {:?}", hw.ops);
        assert!(!hw.wrote_threshold_none(), "{kind:?}: {:?}", hw.ops);
        assert!(hw.gate_open, "a latched gate is still released");
        drop(hw);
        assert!(matches!(
            rig.status().last_error,
            Some(Error::BackendUnavailable(_))
        ));
        rig.percent.set(74);
        rig.service.tick();
        assert_eq!(rig.status().last_error, None);
        assert_eq!(rig.hw.borrow().threshold, Some(80));
    }
}
#[test]
fn an_error_ends_a_temporary_top_up_and_falls_back_to_the_saved_threshold() {
    let mut rig = Rig::new(Kind::Threshold, 74);
    rig.limit(80);
    rig.expect_ok(Command::TopUp);
    assert_eq!(rig.hw.borrow().threshold, None);
    rig.percent.set(UNREADABLE);
    rig.service.tick();
    assert_eq!(rig.hw.borrow().threshold, Some(80));
    assert!(!rig.service.policy.top_up);
}
#[test]
fn a_missing_battery_does_not_write_a_native_threshold_it_cannot_reach() {
    let mut rig = Rig::new(Kind::Both, 74);
    rig.limit(80);
    rig.service.platform.monitor = Box::new(NoBattery);
    rig.clear_ops();
    rig.service.tick();
    assert!(!rig.hw.borrow().wrote_threshold_none());
    assert!(matches!(
        rig.service.last_error,
        Some(Error::BackendUnavailable(_))
    ));
}
struct NoBattery;
impl BatteryMonitor for NoBattery {
    fn batteries(&self) -> Result<Vec<BatteryStatus>> {
        Ok(vec![])
    }
}

// The real sysfs adapter, not a fake, through the lifecycle that the audit flagged.
mod sysfs {
    use super::*;
    use batterybridge_platform::linux::SysfsController;
    use std::fs;

    fn attribute(root: &Path, name: &str) -> String {
        fs::read_to_string(root.join(name))
            .unwrap()
            .trim()
            .to_owned()
    }
    fn sysfs_rig() -> (Rig, PathBuf) {
        let mut rig = Rig::new(Kind::Threshold, 74);
        let root = rig.path.parent().unwrap().join("BAT0");
        fs::create_dir_all(&root).unwrap();
        for (name, value) in [
            ("type", "Battery"),
            ("present", "1"),
            ("charge_control_start_threshold", "99"),
            ("charge_control_end_threshold", "100"),
        ] {
            fs::write(root.join(name), value).unwrap();
        }
        let controller = SysfsController::new(root.clone()).unwrap();
        rig.service.platform.report.capabilities = controller.capabilities();
        rig.service.controller = Some(Box::new(controller));
        (rig, root)
    }
    #[test]
    fn limit_then_orderly_stop_leaves_the_kernel_threshold_at_the_saved_limit() {
        let (mut rig, root) = sysfs_rig();
        rig.limit(80);
        assert_eq!(attribute(&root, "charge_control_end_threshold"), "80");
        rig.service.shutdown().unwrap();
        assert_eq!(attribute(&root, "charge_control_end_threshold"), "80");
        assert_eq!(attribute(&root, "charge_control_start_threshold"), "75");
        assert_eq!(rig.saved().charge_limit, Some(80));
    }
    #[test]
    fn top_up_then_stop_returns_to_the_saved_limit_and_reset_then_stop_clears_it() {
        let (mut rig, root) = sysfs_rig();
        rig.limit(80);
        rig.expect_ok(Command::TopUp);
        assert_eq!(attribute(&root, "charge_control_end_threshold"), "100");
        rig.service.shutdown().unwrap();
        assert_eq!(attribute(&root, "charge_control_end_threshold"), "80");
        let (mut rig, root) = sysfs_rig();
        rig.limit(80);
        rig.expect_ok(Command::Reset);
        rig.service.shutdown().unwrap();
        assert_eq!(attribute(&root, "charge_control_end_threshold"), "100");
        assert_eq!(attribute(&root, "charge_control_start_threshold"), "99");
        assert_eq!(rig.saved(), Config::default());
    }
    #[test]
    fn restart_over_a_surviving_threshold_never_passes_through_unrestricted_charging() {
        let (mut rig, root) = sysfs_rig();
        rig.limit(80);
        rig.service.shutdown().unwrap();
        // A new process: hardware kept 80, so restore must not write anything first.
        let before = fs::metadata(root.join("charge_control_end_threshold"))
            .unwrap()
            .modified()
            .unwrap();
        // Filesystem timestamps are coarse; make a skipped write distinguishable.
        std::thread::sleep(std::time::Duration::from_millis(50));
        let mut restored = rig.restart();
        let controller = SysfsController::new(root.clone()).unwrap();
        restored.platform.report.capabilities = controller.capabilities();
        restored.controller = Some(Box::new(controller));
        restored.tick();
        assert_eq!(attribute(&root, "charge_control_end_threshold"), "80");
        let after = fs::metadata(root.join("charge_control_end_threshold"))
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(before, after, "a matching readback must skip the write");
    }
}
