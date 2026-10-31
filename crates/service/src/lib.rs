//! Service-side command handling, charging-policy reconciliation and lifecycle management.

#![forbid(unsafe_code)]
use batterybridge_core::{policy::Policy, *};
use batterybridge_platform::Platform;
use std::path::PathBuf;

pub struct Service {
    platform: Platform,
    controller: Option<Box<dyn ChargeController>>,
    policy: Policy,
    path: PathBuf,
    last_error: Option<Error>,
    sleep_error: Option<Error>,
    schedule_error: Option<Error>,
    /// Set once shutdown starts: no command, schedule or reconciliation may touch hardware afterwards.
    stopping: bool,
    /// Between sleep preparation and the first reconciliation after wake the hardware state is unverified.
    awaiting_wake: bool,
}
impl Service {
    pub fn new(platform: Platform, path: PathBuf) -> Result<Self> {
        let policy = Policy::new(config::load(&path)?)?;
        let (controller, last_error) = match platform.controller() {
            Ok(c) => (Some(c), None),
            Err(e) => (None, Some(e)),
        };
        Ok(Self {
            platform,
            controller,
            policy,
            path,
            last_error,
            sleep_error: None,
            schedule_error: None,
            stopping: false,
            awaiting_wake: false,
        })
    }
    fn apply_policy(&mut self, next: &mut Policy) -> Result<()> {
        let controller = self
            .controller
            .as_mut()
            .ok_or_else(|| Error::Unsupported("no active charge controller; run probe".into()))?;
        let batteries = self.platform.monitor.batteries()?;
        let Some(battery) = batteries.first() else {
            // Nothing can be written to a missing pack; end daemon-held state only.
            let _ = next.settle(controller.as_mut());
            return Err(Error::BackendUnavailable("battery missing".into()));
        };
        next.tick(controller.as_mut(), battery)
    }
    pub fn tick(&mut self) {
        if self.controller.is_none() || self.stopping {
            return;
        }
        if let Err(error) = self.run_schedules(chrono::Utc::now().timestamp()) {
            if self.schedule_error.as_ref() != Some(&error) {
                tracing::warn!(error=%error, "schedule execution failed");
            }
            self.schedule_error = Some(error);
        }
        let mut next = self.policy.clone();
        match self.apply_policy(&mut next) {
            Ok(()) => {
                if next.auxiliary_error != self.policy.auxiliary_error
                    && let Some(error) = &next.auxiliary_error
                {
                    tracing::warn!(error=%error, "optional battery control failed");
                }
                self.policy = next;
                self.last_error = None;
            }
            Err(e) => {
                // Ephemeral modes are cancelled on errors, never restarted blindly.
                self.policy.cancel_temporary();
                // An enforcement error must not leave a latched gate or a temporary
                // override. A saved native threshold is re-asserted, not cleared:
                // it does not depend on the failed read. The error stays visible.
                if let Some(c) = &mut self.controller {
                    let _ = self.policy.settle(c.as_mut());
                }
                if self.policy.config.care.heat_limit_c.is_some() {
                    self.policy.runtime.heat_paused = true;
                    if let Some(controller) = &mut self.controller {
                        let _ = controller.charging(false);
                    }
                }
                if self.last_error.as_ref() != Some(&e) {
                    tracing::warn!(error=%e,"policy enforcement failed");
                }
                self.last_error = Some(e);
            }
        }
    }
    fn run_schedules(&mut self, now: i64) -> Result<()> {
        // Claim every due task durably before touching hardware. Missed runs coalesce.
        let mut claimed = self.policy.config.clone();
        let mut due = Vec::new();
        for task in &mut claimed.care.schedules {
            if task.enabled && task.next_at <= now {
                due.push((task.id, batterybridge_core::schedule::command(&task.action)));
                batterybridge_core::schedule::advance(task, now)?;
            }
        }
        if due.is_empty() {
            return Ok(());
        }
        config::save(&self.path, &claimed)?;
        self.policy.config = claimed;
        // Deterministic configured order. A failed action is reported, never replayed after restart.
        let mut error = None;
        for (id, command) in due {
            if let Err(e) = self.execute(command) {
                tracing::warn!(schedule_id=id, error=%e, "scheduled action failed");
                error = Some(e);
            }
        }
        self.schedule_error = error.clone();
        error.map_or(Ok(()), Err)
    }
    pub fn prepare_sleep(&mut self) -> Result<()> {
        if self.stopping {
            return Ok(());
        }
        // Until `wake` reconciles, firmware may have changed anything we set.
        self.awaiting_wake = true;
        let result = match &mut self.controller {
            Some(controller) => self.policy.prepare_sleep(controller.as_mut()),
            None => Ok(()),
        };
        // A later successful awake tick does not prove that sleep preparation worked.
        self.sleep_error = result
            .as_ref()
            .err()
            .map(|error| Error::PlatformError(format!("Sleep preparation failed: {error}")));
        result
    }
    /// Re-read and rewrite the hardware before the saved policy is reported as restored.
    pub fn wake(&mut self) {
        self.tick();
        self.awaiting_wake = false;
    }
    /// Orderly stop; see [`Policy::shutdown`] for what is released and what is kept.
    /// Safe to repeat: every step converges on the same hardware state.
    pub fn shutdown(&mut self) -> Result<()> {
        // No new action may start; queued ones are answered by the caller.
        self.stopping = true;
        match &mut self.controller {
            Some(controller) => self.policy.shutdown(controller.as_mut()),
            None => Ok(()),
        }
    }
    pub fn handle(&mut self, request: Request) -> Response {
        if self.stopping {
            return Response::Error(Error::ServiceUnavailable("service is shutting down".into()));
        }
        if let Err(e) = request.validate() {
            return Response::Error(e);
        }
        match self.execute(request.command) {
            Ok(v) => v,
            Err(e) => Response::Error(e),
        }
    }
    fn rollback(&mut self) {
        if let Some(controller) = &mut self.controller {
            let _ = controller.release(Release::Overrides);
        }
        let mut previous = self.policy.clone();
        let paused = previous.runtime.paused;
        previous.cancel_temporary();
        previous.runtime.paused = paused;
        if self.apply_policy(&mut previous).is_err()
            && let Some(controller) = &mut self.controller
        {
            let _ = previous.settle(controller.as_mut());
            if previous.config.care.heat_limit_c.is_some() {
                let _ = controller.charging(false);
            }
        }
        self.policy = previous;
    }
    /// Reset is its own contract, not a tick with an empty policy: every
    /// threshold and override is returned to the hardware, every step is
    /// attempted, and the empty policy (no limit, care or schedules) is made
    /// durable only after the hardware accepted it.
    fn reset(&mut self) -> Result<Response> {
        let mut next = self.policy.clone();
        next.operation(
            &Command::Reset,
            &self.platform.report.capabilities,
            chrono::Utc::now().timestamp(),
        )?;
        let controller = self
            .controller
            .as_mut()
            .ok_or_else(|| Error::Unsupported("no active charge controller; run probe".into()))?;
        if let Err(error) = controller.release(Release::Everything) {
            self.rollback();
            return Err(error);
        }
        if let Err(error) = config::save(&self.path, &next.config) {
            self.rollback();
            return Err(error);
        }
        self.policy = next;
        self.last_error = None;
        self.schedule_error = None;
        Ok(Response::Ok)
    }
    fn execute(&mut self, command: Command) -> Result<Response> {
        match command {
            // Monitoring-only installations have no hardware policy to release.
            // A saved active limit still requires a working controller, so removal
            // cannot silently leave previously configured hardware inhibition.
            Command::Reset
            | Command::Limit {
                stop: None,
                start: None,
            } if self.controller.is_none()
                && self.policy.config.charge_limit.is_none()
                && !self.policy.top_up
                && self.policy.discharge_target.is_none()
                && self.policy.config.care == CareSettings::default()
                && self.platform.report.capabilities.charge_limit.state
                    == CapabilityState::Unsupported =>
            {
                Ok(Response::Ok)
            }
            Command::Status => Ok(Response::Status(Box::new(Snapshot {
                batteries: self.platform.monitor.batteries()?,
                device: self.platform.device.clone(),
                backend: self.platform.backend(),
                capabilities: self.platform.report.capabilities.clone(),
                config: Some(self.policy.config.clone()),
                top_up_active: self.policy.top_up,
                discharge_target: self.policy.discharge_target,
                runtime: self.policy.runtime.clone(),
                last_error: self
                    .awaiting_wake
                    .then(|| {
                        Error::PlatformError("hardware state is not yet verified after wake".into())
                    })
                    .or_else(|| self.last_error.clone())
                    .or_else(|| self.policy.auxiliary_error.clone())
                    .or_else(|| self.sleep_error.clone())
                    .or_else(|| self.schedule_error.clone()),
            }))),
            Command::Capabilities => Ok(Response::Capabilities(Box::new(
                self.platform.report.capabilities.clone(),
            ))),
            Command::Probe => Ok(Response::Probe(Box::new(Platform::discover()?.report))),
            Command::Config => Ok(Response::Config(self.policy.config.clone())),
            Command::Reset => self.reset(),
            command => {
                if let Command::Care { expected, .. } = &command
                    && expected != &self.policy.config.care
                {
                    return Err(Error::ConfigurationError(
                        "care settings changed; refresh before saving".into(),
                    ));
                }
                let mut next = self.policy.clone();
                let caps = &self.platform.report.capabilities;
                let persist = matches!(
                    command,
                    Command::Limit { .. } | Command::Care { .. } | Command::Reset
                );
                next.operation(&command, caps, chrono::Utc::now().timestamp())?;
                if let Err(error) = self.apply_policy(&mut next) {
                    self.rollback();
                    return Err(error);
                }
                if persist && let Err(error) = config::save(&self.path, &next.config) {
                    self.rollback();
                    return Err(error);
                }
                self.policy = next;
                self.last_error = None;
                self.schedule_error = None;
                Ok(Response::Ok)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::RefCell,
        collections::BTreeMap,
        rc::Rc,
        sync::atomic::{AtomicU64, Ordering},
    };
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    struct Monitor;
    impl BatteryMonitor for Monitor {
        fn batteries(&self) -> Result<Vec<BatteryStatus>> {
            Ok(vec![BatteryStatus {
                id: "BAT0".into(),
                percent: Some(74),
                state: "charging".into(),
                on_ac: Some(true),
                health_percent: None,
                hardware_start: None,
                hardware_stop: None,
                telemetry: BatteryTelemetry::default(),
            }])
        }
    }
    struct Controller(Rc<RefCell<Option<u8>>>);
    impl ChargeController for Controller {
        fn capabilities(&self) -> Capabilities {
            let mut c = Capabilities::unsupported("fixture");
            c.charge_limit = Capability::supported("fixture");
            c.stop_threshold = c.charge_limit.clone();
            c.temporary_full_charge = c.charge_limit.clone();
            c
        }
        fn apply(&mut self, _: Option<u8>, stop: Option<u8>) -> Result<()> {
            *self.0.borrow_mut() = stop;
            Ok(())
        }
        fn release(&mut self, scope: Release) -> Result<()> {
            match scope {
                Release::Overrides => Ok(()),
                Release::Everything => self.apply(None, None),
            }
        }
    }
    fn service(path: PathBuf, state: Rc<RefCell<Option<u8>>>) -> Service {
        let device = DeviceInfo {
            os: "test".into(),
            os_version: "test".into(),
            architecture: "fixture".into(),
            vendor: "fixture".into(),
            model: "fixture".into(),
        };
        let controller = Controller(state);
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
                monitor: Box::new(Monitor),
                device,
                report,
            },
            controller: Some(Box::new(controller)),
            policy: Policy::new(config::load(&path).unwrap()).unwrap(),
            path,
            last_error: None,
            sleep_error: None,
            schedule_error: None,
            stopping: false,
            awaiting_wake: false,
        }
    }
    fn dir() -> PathBuf {
        std::env::temp_dir().join(format!(
            "batterybridge-service-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }
    #[test]
    fn sleep_failure_remains_visible_after_successful_awake_reconciliation() {
        struct SleepFailure;
        impl ChargeController for SleepFailure {
            fn capabilities(&self) -> Capabilities {
                Controller(Rc::new(RefCell::new(None))).capabilities()
            }
            fn apply(&mut self, _: Option<u8>, _: Option<u8>) -> Result<()> {
                Ok(())
            }
            fn charging(&mut self, enabled: bool) -> Result<()> {
                if enabled {
                    Ok(())
                } else {
                    Err(Error::PlatformError("sleep gate refused".into()))
                }
            }
            fn release(&mut self, _: Release) -> Result<()> {
                Ok(())
            }
        }
        let mut s = service(dir().join("config.toml"), Rc::new(RefCell::new(None)));
        s.controller = Some(Box::new(SleepFailure));
        s.policy.config.charge_limit = Some(80);
        assert!(s.prepare_sleep().is_err());
        s.wake(); // 74% allows charging, but cannot validate a previous sleep transition.
        let Response::Status(snapshot) = s.handle(Request::new(Command::Status)) else {
            panic!("status");
        };
        assert_eq!(
            snapshot.last_error,
            Some(Error::PlatformError(
                "Sleep preparation failed: platform error: inhibit charging for sleep: sleep gate refused"
                    .into()
            ))
        );
        s.controller = Some(Box::new(Controller(Rc::new(RefCell::new(None)))));
        s.prepare_sleep().unwrap();
        s.wake();
        let Response::Status(snapshot) = s.handle(Request::new(Command::Status)) else {
            panic!("status");
        };
        assert_eq!(snapshot.last_error, None);
    }
    #[test]
    fn monitoring_only_uninstall_accepts_off_but_does_not_hide_an_active_policy() {
        let mut s = service(dir().join("config.toml"), Rc::new(RefCell::new(None)));
        s.controller = None;
        s.platform.report.capabilities = Capabilities::unsupported("monitor only");
        assert!(matches!(
            s.handle(Request::new(Command::Limit {
                stop: None,
                start: None
            })),
            Response::Ok
        ));
        s.platform.report.capabilities.charge_limit = Capability::unknown("access denied");
        assert!(matches!(
            s.handle(Request::new(Command::Reset)),
            Response::Error(_)
        ));
        s.platform.report.capabilities = Capabilities::unsupported("monitor only");
        s.policy.config.charge_limit = Some(80);
        assert!(matches!(
            s.handle(Request::new(Command::Limit {
                stop: None,
                start: None
            })),
            Response::Error(_)
        ));
        assert_eq!(s.policy.config.charge_limit, Some(80));
    }
    #[test]
    fn persists_policy_and_clears_ephemeral_mode_on_restart() {
        let root = dir();
        let path = root.join("config.toml");
        let state = Rc::new(RefCell::new(None));
        let mut s = service(path.clone(), state.clone());
        assert!(matches!(
            s.handle(Request::new(Command::Limit {
                stop: Some(80),
                start: None
            })),
            Response::Ok
        ));
        assert_eq!(*state.borrow(), Some(80));
        assert!(matches!(
            s.handle(Request::new(Command::TopUp)),
            Response::Ok
        ));
        assert_eq!(*state.borrow(), None);
        let mut restored = service(path, state.clone());
        restored.tick();
        assert_eq!(*state.borrow(), Some(80));
        assert!(!restored.policy.top_up);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn rejects_start_threshold_and_rolls_back_persistence_error() {
        let root = dir();
        let path = root.join("config.toml");
        let state = Rc::new(RefCell::new(None));
        let mut s = service(path.clone(), state.clone());
        assert!(matches!(
            s.handle(Request::new(Command::Limit {
                stop: Some(80),
                start: Some(75)
            })),
            Response::Error(Error::Unsupported(_))
        ));
        assert_eq!(*state.borrow(), None);
        std::fs::create_dir_all(&path).unwrap();
        assert!(matches!(
            s.handle(Request::new(Command::Limit {
                stop: Some(80),
                start: None
            })),
            Response::Error(_)
        ));
        assert_eq!(*state.borrow(), None);
        assert_eq!(s.policy.config.charge_limit, None);
        std::fs::remove_dir_all(root).unwrap();
    }
    fn scheduled(at: i64) -> Schedule {
        Schedule {
            id: 1,
            enabled: true,
            next_at: at,
            anchor_at: None,
            repeat: Repeat::Once,
            action: ScheduleAction::Limit {
                stop: 80,
                start: None,
            },
        }
    }
    #[test]
    fn due_schedule_is_claimed_before_hardware_and_is_not_replayed_on_restart() {
        let root = dir();
        let path = root.join("config.toml");
        let state = Rc::new(RefCell::new(None));
        let mut s = service(path.clone(), state.clone());
        s.policy.config.care.schedules.push(scheduled(100));
        config::save(&path, &s.policy.config).unwrap();
        s.run_schedules(200).unwrap();
        assert_eq!(*state.borrow(), Some(80));
        let disk = config::load(&path).unwrap();
        assert!(!disk.care.schedules[0].enabled);
        assert_eq!(disk.charge_limit, Some(80));
        let mut restored = service(path, state.clone());
        restored.run_schedules(300).unwrap();
        assert!(!restored.policy.config.care.schedules[0].enabled);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn failed_schedule_claim_never_runs_action_but_still_enforces_existing_limit() {
        let root = dir();
        let path = root.join("config.toml");
        let state = Rc::new(RefCell::new(None));
        let mut s = service(path.clone(), state.clone());
        s.policy.config.charge_limit = Some(85);
        s.policy.config.care.schedules.push(scheduled(100));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(path.with_extension("toml.new"), "interrupted").unwrap();
        s.tick();
        assert_eq!(*state.borrow(), Some(85));
        assert!(s.schedule_error.is_some());
        assert!(s.policy.config.care.schedules[0].enabled);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn stale_care_update_cannot_reactivate_a_claimed_schedule() {
        let root = dir();
        let path = root.join("config.toml");
        let mut s = service(path.clone(), Rc::new(RefCell::new(None)));
        s.policy.config.care.schedules.push(scheduled(100));
        let stale = s.policy.config.care.clone();
        s.run_schedules(200).unwrap();
        assert!(matches!(
            s.handle(Request::new(Command::Care {
                settings: stale.clone(),
                expected: stale
            })),
            Response::Error(Error::ConfigurationError(_))
        ));
        assert!(!s.policy.config.care.schedules[0].enabled);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn unsupported_scheduled_operations_are_rejected_before_persistence() {
        let root = dir();
        let path = root.join("config.toml");
        let mut s = service(path.clone(), Rc::new(RefCell::new(None)));
        let expected = s.policy.config.care.clone();
        let mut settings = expected.clone();
        let mut task = scheduled(100);
        task.action = ScheduleAction::Calibrate;
        settings.schedules.push(task);
        assert!(matches!(
            s.handle(Request::new(Command::Care { settings, expected })),
            Response::Error(Error::Unsupported(_))
        ));
        assert!(!path.exists());
    }
    #[test]
    fn failed_scheduled_hardware_action_is_reported_and_not_replayed() {
        struct RejectLimit(Rc<RefCell<u32>>);
        impl ChargeController for RejectLimit {
            fn capabilities(&self) -> Capabilities {
                Controller(Rc::new(RefCell::new(None))).capabilities()
            }
            fn apply(&mut self, _: Option<u8>, stop: Option<u8>) -> Result<()> {
                if stop == Some(80) {
                    *self.0.borrow_mut() += 1;
                    Err(Error::HardwareError("fixture refused".into()))
                } else {
                    Ok(())
                }
            }
            fn release(&mut self, _: Release) -> Result<()> {
                Ok(())
            }
        }
        let root = dir();
        let path = root.join("config.toml");
        let mut s = service(path.clone(), Rc::new(RefCell::new(None)));
        let count = Rc::new(RefCell::new(0));
        s.controller = Some(Box::new(RejectLimit(count.clone())));
        s.policy.config.care.schedules.push(scheduled(100));
        assert!(s.run_schedules(200).is_err());
        assert_eq!(*count.borrow(), 1);
        s.run_schedules(300).unwrap();
        assert_eq!(*count.borrow(), 1);
        s.tick();
        let Response::Status(status) = s.handle(Request::new(Command::Status)) else {
            panic!("status");
        };
        assert!(
            matches!(status.last_error, Some(Error::HardwareError(_))),
            "successful periodic enforcement must not erase a consumed task failure"
        );
        assert!(!config::load(&path).unwrap().care.schedules[0].enabled);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn reset_disables_schedules_and_saved_care_before_acknowledgement() {
        let root = dir();
        let path = root.join("config.toml");
        let state = Rc::new(RefCell::new(Some(80)));
        let mut s = service(path.clone(), state.clone());
        s.policy.config.charge_limit = Some(80);
        s.policy.config.care.schedules.push(scheduled(100));
        assert!(matches!(
            s.handle(Request::new(Command::Reset)),
            Response::Ok
        ));
        assert_eq!(*state.borrow(), None);
        assert_eq!(config::load(&path).unwrap(), Config::default());
        s.run_schedules(200).unwrap();
        assert_eq!(*state.borrow(), None);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn optional_led_and_idle_assertion_failures_preserve_charging_and_report_recovery() {
        #[derive(Default)]
        struct State {
            enabled: bool,
            failure: Option<&'static str>,
            releases: usize,
        }
        struct OptionalController(Rc<RefCell<State>>);
        impl ChargeController for OptionalController {
            fn capabilities(&self) -> Capabilities {
                let mut caps = Controller(Rc::new(RefCell::new(None))).capabilities();
                caps.magsafe_led = caps.charge_limit.clone();
                caps.sleep_prevention = caps.charge_limit.clone();
                caps
            }
            fn apply(&mut self, _: Option<u8>, _: Option<u8>) -> Result<()> {
                Ok(())
            }
            fn charging(&mut self, enabled: bool) -> Result<()> {
                self.0.borrow_mut().enabled = enabled;
                Ok(())
            }
            fn magsafe_led(&mut self, _: LedMode) -> Result<()> {
                if self.0.borrow().failure == Some("LED rejected") {
                    Err(Error::HardwareError("LED rejected".into()))
                } else {
                    Ok(())
                }
            }
            fn prevent_sleep(&mut self, _: bool) -> Result<()> {
                if self.0.borrow().failure == Some("idle assertion rejected") {
                    Err(Error::PlatformError("idle assertion rejected".into()))
                } else {
                    Ok(())
                }
            }
            fn release(&mut self, _: Release) -> Result<()> {
                let mut state = self.0.borrow_mut();
                state.enabled = true;
                state.releases += 1;
                Ok(())
            }
        }
        let root = dir();
        let mut s = service(root.join("config.toml"), Rc::new(RefCell::new(None)));
        let state = Rc::new(RefCell::new(State::default()));
        let controller = OptionalController(state.clone());
        s.platform.report.capabilities = controller.capabilities();
        s.controller = Some(Box::new(controller));
        s.policy.config.charge_limit = Some(70); // Monitor reports 74%.
        s.policy.config.care.led = LedMode::Orange;
        s.tick();
        assert!(!state.borrow().enabled);
        for failure in ["LED rejected", "idle assertion rejected"] {
            state.borrow_mut().failure = Some(failure);
            for _ in 0..2 {
                s.tick();
                assert!(!state.borrow().enabled);
                assert_eq!(state.borrow().releases, 0);
                assert_eq!(s.policy.config.charge_limit, Some(70));
                let Response::Status(status) = s.handle(Request::new(Command::Status)) else {
                    panic!("status");
                };
                assert!(status.last_error.unwrap().to_string().contains(failure));
            }
            state.borrow_mut().failure = None;
            s.tick();
            let Response::Status(status) = s.handle(Request::new(Command::Status)) else {
                panic!("status");
            };
            assert_eq!(status.last_error, None);
            assert!(!state.borrow().enabled);
        }
    }
    #[test]
    fn disabling_percentage_limit_does_not_get_blocked_by_a_future_top_up() {
        let root = dir();
        let path = root.join("config.toml");
        let mut s = service(path.clone(), Rc::new(RefCell::new(None)));
        s.policy.config.charge_limit = Some(80);
        let mut task = scheduled(100);
        task.action = ScheduleAction::TopUp;
        s.policy.config.care.schedules.push(task);
        assert!(matches!(
            s.handle(Request::new(Command::Limit {
                stop: None,
                start: None
            })),
            Response::Ok
        ));
        assert_eq!(config::load(&path).unwrap().charge_limit, None);
        assert!(s.policy.config.care.schedules[0].enabled);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod lifecycle_tests;
