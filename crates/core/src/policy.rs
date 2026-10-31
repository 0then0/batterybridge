use crate::*;

#[derive(Debug, Default, Clone)]
pub struct Policy {
    pub config: Config,
    pub top_up: bool,
    pub discharge_target: Option<u8>,
    pub runtime: RuntimeStatus,
    /// Optional LED/idle-assertion errors must not release charging controls.
    pub auxiliary_error: Option<Error>,
    inhibited: bool,
    automatic_discharge_active: bool,
    calibration_clock: Option<(std::time::Instant, i64)>,
}
impl Policy {
    pub fn new(config: Config) -> Result<Self> {
        config::validate(&config)?;
        Ok(Self {
            config,
            ..Self::default()
        })
    }
    pub fn validate_for(&self, caps: &Capabilities) -> Result<()> {
        config::validate(&self.config)?;
        if let Some(stop) = self.config.charge_limit {
            require(&caps.charge_limit)?;
            if caps
                .allowed_limits
                .as_ref()
                .is_some_and(|v| !v.contains(&stop))
            {
                return Err(Error::Unsupported(format!(
                    "allowed charge limits: {:?}",
                    caps.allowed_limits
                )));
            }
        }
        if self.config.start_threshold.enabled {
            require(&caps.start_threshold)?;
        }
        let care = &self.config.care;
        if care.heat_limit_c.is_some() {
            require(&caps.heat_protection)?;
        }
        if care.automatic_discharge {
            require(&caps.force_discharge)?;
            if self.config.charge_limit.is_some_and(|limit| limit < 20) {
                return Err(Error::InvalidLimit(
                    "automatic discharge requires a saved limit of at least 20%".into(),
                ));
            }
        }
        if care.prevent_sleep {
            require(&caps.sleep_prevention)?;
        }
        if care.led != LedMode::System {
            require(&caps.magsafe_led)?;
        }
        for task in &care.schedules {
            if task.enabled {
                let mut probe = self.clone();
                // Validate operation availability without running it or recursing into schedules.
                probe.config.care.schedules.clear();
                if matches!(task.action, ScheduleAction::TopUp) {
                    require(&caps.temporary_full_charge)?;
                } else {
                    probe.operation(&schedule::command(&task.action), caps, task.next_at)?;
                }
            }
        }
        Ok(())
    }
    pub fn operation(&mut self, command: &Command, caps: &Capabilities, now: i64) -> Result<()> {
        match command {
            Command::Limit { stop, start } => self.set_limit(*stop, *start, caps),
            Command::Care { settings, .. } => self.set_care(settings.clone(), caps),
            Command::TopUp => self.top_up(caps),
            Command::Discharge { target } => self.discharge(*target, caps),
            Command::ChargeTo { target } => self.charge_to(*target, caps),
            Command::StopCharging => self.hold(caps),
            Command::Calibrate => self.calibrate(caps, now),
            Command::Reset => {
                *self = Self::default();
                Ok(())
            }
            Command::Pause => {
                self.pause();
                Ok(())
            }
            Command::Resume => {
                self.cancel_temporary();
                Ok(())
            }
            Command::Cancel => {
                self.cancel_temporary();
                self.runtime.automatic_discharge_paused = true;
                Ok(())
            }
            Command::Adapter { enabled: true } => {
                require(&caps.adapter_disable)?;
                self.cancel_temporary();
                Ok(())
            }
            Command::Adapter { enabled: false } => {
                require(&caps.adapter_disable)?;
                self.discharge(20, caps)
            }
            _ => Err(Error::ProtocolError("not a policy operation".into())),
        }
    }
    pub fn set_limit(
        &mut self,
        stop: Option<u8>,
        start: Option<u8>,
        caps: &Capabilities,
    ) -> Result<()> {
        require(&caps.charge_limit)?;
        let mut candidate = Self::new(Config {
            version: 2,
            charge_limit: stop,
            start_threshold: StartThreshold {
                enabled: start.is_some(),
                value: start.unwrap_or(75),
            },
            care: self.config.care.clone(),
        })?;
        candidate.runtime.heat_paused = self.runtime.heat_paused;
        candidate.validate_for(caps)?;
        *self = candidate;
        Ok(())
    }
    pub fn set_care(&mut self, settings: CareSettings, caps: &Capabilities) -> Result<()> {
        let mut next = self.clone();
        if self.automatic_discharge_active && !settings.automatic_discharge {
            next.discharge_target = None;
            next.automatic_discharge_active = false;
        }
        next.config.care = settings;
        next.validate_for(caps)?;
        *self = next;
        Ok(())
    }
    pub fn cancel_temporary(&mut self) {
        self.top_up = false;
        self.discharge_target = None;
        self.automatic_discharge_active = false;
        self.calibration_clock = None;
        self.runtime = RuntimeStatus {
            heat_paused: self.runtime.heat_paused,
            ..RuntimeStatus::default()
        };
    }
    pub fn top_up(&mut self, caps: &Capabilities) -> Result<()> {
        require(&caps.temporary_full_charge)?;
        if self.config.charge_limit.is_none() {
            return Err(Error::ConfigurationError(
                "set a charge limit before top-up".into(),
            ));
        }
        self.cancel_temporary();
        self.top_up = true;
        Ok(())
    }
    pub fn discharge(&mut self, target: u8, caps: &Capabilities) -> Result<()> {
        Command::Discharge { target }.validate()?;
        require(&caps.force_discharge)?;
        self.cancel_temporary();
        self.discharge_target = Some(target);
        Ok(())
    }
    pub fn charge_to(&mut self, target: u8, caps: &Capabilities) -> Result<()> {
        Command::ChargeTo { target }.validate()?;
        require(&caps.charge_limit)?;
        if caps
            .allowed_limits
            .as_ref()
            .is_some_and(|limits| !limits.contains(&target))
        {
            return Err(Error::Unsupported("unsupported charge target".into()));
        }
        self.cancel_temporary();
        self.runtime.charge_target = Some(target);
        self.inhibited = false;
        Ok(())
    }
    pub fn calibrate(&mut self, caps: &Capabilities, now: i64) -> Result<()> {
        require(&caps.force_discharge)?;
        require(&caps.temporary_full_charge)?;
        self.cancel_temporary();
        self.calibration_clock = Some((std::time::Instant::now(), now));
        self.runtime.calibration = Some(Calibration {
            phase: CalibrationPhase::Charging,
            started_at: now,
            phase_at: now,
        });
        Ok(())
    }
    pub fn hold(&mut self, caps: &Capabilities) -> Result<()> {
        require(&caps.manual_charging)?;
        self.cancel_temporary();
        self.runtime.holding = true;
        Ok(())
    }
    pub fn pause(&mut self) {
        self.cancel_temporary();
        self.runtime = RuntimeStatus {
            paused: true,
            ..RuntimeStatus::default()
        };
    }
    /// Sleep keeps the saved policy and every native threshold. The daemon
    /// cannot enforce while asleep, so daemon-held overrides end and a binary
    /// gate is closed for limits below 100%. Each step is independent: a failed
    /// discharge release must not skip the charging inhibition.
    pub fn prepare_sleep(&mut self, controller: &mut dyn ChargeController) -> Result<()> {
        let discharge = if self.discharge_target.is_some() || self.runtime.calibration.is_some() {
            let result = controller.force_discharge(false);
            if result.is_ok() {
                self.discharge_target = None;
                self.runtime.calibration = None;
                self.calibration_clock = None;
            }
            result
        } else {
            Ok(())
        };
        let idle = if controller.capabilities().sleep_prevention.available() {
            let result = controller.prevent_sleep(false);
            if result.is_ok() {
                self.runtime.sleep_prevented = false;
            }
            result
        } else {
            self.runtime.sleep_prevented = false;
            Ok(())
        };
        let inhibit = if !self.runtime.paused
            && (self.runtime.charge_target.map_or(
                self.config.charge_limit.is_some() && !self.top_up,
                |target| target < 100,
            ) || self.runtime.holding
                || self.runtime.heat_paused)
        {
            let result = controller.charging(false);
            if result.is_ok() {
                self.inhibited = true;
            }
            result
        } else {
            Ok(())
        };
        merge_steps([
            ("release forced discharge", discharge),
            ("release idle-sleep assertion", idle),
            ("inhibit charging for sleep", inhibit),
        ])
    }
    /// Orderly stop of the service process. Temporary modes end, daemon-held
    /// overrides are released, and the hardware is left holding the *saved*
    /// native threshold rather than a temporary top-up or target. Pause is
    /// honoured: its release stays in place until the next start re-applies the
    /// saved policy. Nothing here distinguishes a service restart from an OS
    /// shutdown, because the process cannot tell them apart reliably.
    pub fn shutdown(&mut self, controller: &mut dyn ChargeController) -> Result<()> {
        let paused = self.runtime.paused;
        self.cancel_temporary();
        self.runtime.paused = paused;
        self.settle(controller)
    }
    /// Bring the hardware to the saved, non-temporary policy without clearing a
    /// native threshold: daemon-held overrides end and the saved threshold is
    /// written again (unless paused, which keeps its release). This is the
    /// fallback for a failed reconciliation too. The service may not know why
    /// reconciliation failed, but a native limit does not depend on it, so it
    /// is never cleared here. Both steps always run.
    pub fn settle(&self, controller: &mut dyn ChargeController) -> Result<()> {
        let overrides = controller.release(Release::Overrides);
        let threshold = if self.runtime.paused {
            Ok(())
        } else {
            let (start, stop) = self.saved_band();
            controller.apply(start, stop)
        };
        merge_steps([
            ("release daemon-held overrides", overrides),
            ("restore saved threshold", threshold),
        ])
    }
    /// The native threshold pair that represents the saved, non-temporary policy.
    fn saved_band(&self) -> (Option<u8>, Option<u8>) {
        match self.config.charge_limit {
            None => (None, None),
            Some(stop) => (
                self.config
                    .start_threshold
                    .enabled
                    .then_some(self.config.start_threshold.value),
                Some(stop),
            ),
        }
    }
    pub fn tick(
        &mut self,
        controller: &mut dyn ChargeController,
        battery: &BatteryStatus,
    ) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        if self.runtime.calibration.is_some()
            && self.calibration_clock.is_some_and(|(clock, origin)| {
                let elapsed = clock.elapsed().as_secs();
                elapsed > 86_400 || now.saturating_sub(origin).abs_diff(elapsed as i64) > 120
            })
        {
            self.cancel_temporary();
            let _ = self.settle(controller);
            return Err(Error::PlatformError(
                "calibration cancelled: clock changed or 24-hour timeout".into(),
            ));
        }
        self.tick_at(controller, battery, now)
    }
    pub fn tick_at(
        &mut self,
        controller: &mut dyn ChargeController,
        battery: &BatteryStatus,
        now: i64,
    ) -> Result<()> {
        self.auxiliary_error = None;
        require(&controller.capabilities().charge_limit)?;
        if self.runtime.paused {
            controller.release(Release::Everything)?;
            self.runtime.sleep_prevented = false;
            return Ok(());
        }
        self.validate_for(&controller.capabilities())?;
        let Some(percent) = battery.percent else {
            if self.discharge_target.is_some() || self.runtime.calibration.is_some() {
                controller.force_discharge(false)?;
            }
            self.cancel_temporary();
            if self.config.care.heat_limit_c.is_some() {
                self.runtime.heat_paused = true;
                self.runtime.temperature_unavailable = true;
                controller.charging(false)?;
                return self.finish(controller, battery, false, false);
            }
            return Err(Error::BackendUnavailable(
                "battery percentage unavailable".into(),
            ));
        };
        if battery.on_ac != Some(true)
            && (self.discharge_target.is_some() || self.runtime.calibration.is_some())
        {
            controller.force_discharge(false)?;
            self.cancel_temporary();
            return Err(Error::BackendUnavailable(
                "external power unavailable; temporary discharge/calibration cancelled".into(),
            ));
        }
        if self.runtime.calibration.as_ref().is_some_and(|cycle| {
            now < cycle.started_at
                || now < cycle.phase_at
                || now.saturating_sub(cycle.started_at) > 86_400
        }) {
            self.cancel_temporary();
            let _ = self.settle(controller);
            return Err(Error::PlatformError(
                "calibration cancelled: clock changed or 24-hour timeout".into(),
            ));
        }
        let heat = if let Some(limit) = self.config.care.heat_limit_c {
            self.runtime.temperature_unavailable = battery.telemetry.temperature_deci_c.is_none();
            let temperature = battery.telemetry.temperature_deci_c.unwrap_or(i16::MAX);
            if temperature >= i16::from(limit) * 10 {
                self.runtime.heat_paused = true;
            } else if temperature <= (i16::from(limit) - 2) * 10 {
                self.runtime.heat_paused = false;
            }
            self.runtime.heat_paused
        } else {
            self.runtime.heat_paused = false;
            self.runtime.temperature_unavailable = false;
            false
        };
        if heat {
            if controller.capabilities().force_discharge.available() {
                controller.force_discharge(false)?;
            }
            controller.charging(false)?;
            self.finish(controller, battery, false, false)?;
            return Ok(());
        }
        if self.runtime.holding {
            if controller.capabilities().force_discharge.available() {
                controller.force_discharge(false)?;
            }
            controller.charging(false)?;
            return self.finish(controller, battery, false, false);
        }
        if let Some(calibration) = &mut self.runtime.calibration {
            let next = match calibration.phase {
                CalibrationPhase::Charging
                    if percent == 100 || battery.state.eq_ignore_ascii_case("full") =>
                {
                    Some(CalibrationPhase::Discharging)
                }
                CalibrationPhase::Discharging if percent <= 10 => {
                    Some(CalibrationPhase::Recharging)
                }
                CalibrationPhase::Recharging
                    if percent == 100 || battery.state.eq_ignore_ascii_case("full") =>
                {
                    Some(CalibrationPhase::Resting)
                }
                _ => None,
            };
            if let Some(phase) = next {
                calibration.phase = phase;
                calibration.phase_at = now;
            }
            if calibration.phase == CalibrationPhase::Resting
                && now.saturating_sub(calibration.phase_at) >= 3600
            {
                self.runtime.calibration = None;
                self.calibration_clock = None;
            } else {
                let discharging = calibration.phase == CalibrationPhase::Discharging;
                controller.force_discharge(discharging)?;
                if !discharging {
                    controller.apply(None, None)?;
                    controller.charging(true)?;
                }
                return self.finish(
                    controller,
                    battery,
                    !discharging && percent < 100,
                    discharging,
                );
            }
        }
        if self.top_up && (percent == 100 || battery.state.eq_ignore_ascii_case("full")) {
            self.top_up = false;
        }
        let temporary_charge = self.top_up || self.runtime.charge_target.is_some();
        let stop = if temporary_charge {
            self.runtime.charge_target
        } else {
            self.config.charge_limit
        };
        if !temporary_charge
            && !self.runtime.automatic_discharge_paused
            && self.config.care.automatic_discharge
            && self.discharge_target.is_none()
            && stop.is_some_and(|stop| stop >= 20 && percent > stop)
            && battery.on_ac == Some(true)
        {
            self.discharge_target = stop;
            self.automatic_discharge_active = true;
        }
        if let Some(target) = self.discharge_target {
            if percent <= target {
                self.discharge_target = None;
                self.automatic_discharge_active = false;
                controller.force_discharge(false)?;
            } else {
                controller.force_discharge(true)?;
                return self.finish(controller, battery, false, true);
            }
        } else if controller.capabilities().force_discharge.available() {
            controller.force_discharge(false)?;
        }
        if self.top_up || stop.is_none() {
            controller.apply(None, None)?;
            controller.charging(true)?;
            self.inhibited = false;
            return self.finish(controller, battery, percent < 100, false);
        }
        let stop = stop.expect("checked limit");
        let explicit_start = !temporary_charge && self.config.start_threshold.enabled;
        let start = if explicit_start {
            self.config.start_threshold.value
        } else {
            stop.saturating_sub(5)
        };
        controller.apply(explicit_start.then_some(start), Some(stop))?;
        if percent >= stop {
            self.inhibited = true;
        } else if percent <= start {
            self.inhibited = false;
        }
        controller.charging(!self.inhibited)?;
        self.finish(controller, battery, !self.inhibited, false)
    }
    fn finish(
        &mut self,
        controller: &mut dyn ChargeController,
        battery: &BatteryStatus,
        charging: bool,
        discharging: bool,
    ) -> Result<()> {
        let caps = controller.capabilities();
        let awake = self.config.care.prevent_sleep
            && battery.on_ac == Some(true)
            && (charging || discharging);
        if caps.sleep_prevention.available() {
            match controller.prevent_sleep(awake) {
                Ok(()) => self.runtime.sleep_prevented = awake,
                Err(error) => self.auxiliary_error = Some(error),
            }
        } else {
            self.runtime.sleep_prevented = false;
        }
        if caps.magsafe_led.available() {
            let led = match self.config.care.led {
                LedMode::Automatic => {
                    if charging || discharging {
                        LedMode::Orange
                    } else {
                        LedMode::Green
                    }
                }
                mode => mode,
            };
            if let Err(error) = controller.magsafe_led(led)
                && self.auxiliary_error.is_none()
            {
                self.auxiliary_error = Some(error);
            }
        }
        Ok(())
    }
}
pub fn require(cap: &Capability) -> Result<()> {
    if !cap.available() {
        return Err(Error::Unsupported(cap.reason.clone()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Fake {
        fail_idle: bool,
        awake: bool,
        led: LedMode,
        enabled: bool,
        stop: Option<u8>,
        discharge: bool,
    }
    impl ChargeController for Fake {
        fn capabilities(&self) -> Capabilities {
            let mut c = Capabilities::unsupported("test");
            c.charge_limit = Capability::supported("test");
            c.start_threshold = c.charge_limit.clone();
            c.temporary_full_charge = c.charge_limit.clone();
            c.force_discharge = c.charge_limit.clone();
            c.adapter_disable = c.charge_limit.clone();
            c.manual_charging = c.charge_limit.clone();
            c.heat_protection = c.charge_limit.clone();
            c.sleep_prevention = c.charge_limit.clone();
            c.magsafe_led = c.charge_limit.clone();
            c
        }
        fn apply(&mut self, _: Option<u8>, stop: Option<u8>) -> Result<()> {
            self.stop = stop;
            Ok(())
        }
        fn charging(&mut self, v: bool) -> Result<()> {
            self.enabled = v;
            Ok(())
        }
        fn prevent_sleep(&mut self, enabled: bool) -> Result<()> {
            if self.fail_idle {
                return Err(Error::PlatformError("idle assertion rejected".into()));
            }
            self.awake = enabled;
            Ok(())
        }
        fn magsafe_led(&mut self, mode: LedMode) -> Result<()> {
            self.led = mode;
            Ok(())
        }
        fn release(&mut self, scope: Release) -> Result<()> {
            self.enabled = true;
            self.discharge = false;
            self.awake = false;
            self.led = LedMode::System;
            if scope == Release::Everything {
                self.stop = None;
            }
            Ok(())
        }
        fn force_discharge(&mut self, v: bool) -> Result<()> {
            self.discharge = v;
            Ok(())
        }
    }
    fn battery(p: u8) -> BatteryStatus {
        BatteryStatus {
            id: "BAT0".into(),
            percent: Some(p),
            state: "charging".into(),
            health_percent: None,
            on_ac: Some(true),
            hardware_start: None,
            hardware_stop: None,
            telemetry: BatteryTelemetry::default(),
        }
    }
    #[test]
    fn sleep_preserves_inhibition_until_resume_threshold_without_releasing_limit() {
        let mut f = Fake {
            enabled: true,
            stop: None,
            discharge: false,
            ..Fake::default()
        };
        let mut p = Policy::default();
        p.set_limit(Some(80), None, &f.capabilities()).unwrap();
        p.tick(&mut f, &battery(78)).unwrap();
        assert!(f.enabled);
        p.prepare_sleep(&mut f).unwrap();
        assert!(!f.enabled);
        assert_eq!(f.stop, Some(80));
        p.tick(&mut f, &battery(79)).unwrap();
        assert!(!f.enabled, "wake must not reopen the gate inside the band");
        p.tick(&mut f, &battery(75)).unwrap();
        assert!(f.enabled);
        p.top_up(&f.capabilities()).unwrap();
        p.tick(&mut f, &battery(85)).unwrap();
        p.prepare_sleep(&mut f).unwrap();
        assert!(f.enabled, "explicit top-up remains allowed during sleep");
    }
    #[test]
    fn sleep_still_inhibits_charging_when_releasing_idle_assertion_fails() {
        let mut f = Fake::default();
        let mut p = Policy::default();
        p.set_limit(Some(80), None, &f.capabilities()).unwrap();
        p.tick(&mut f, &battery(50)).unwrap();
        assert!(f.enabled);
        f.fail_idle = true;
        assert!(p.prepare_sleep(&mut f).is_err());
        assert!(!f.enabled);
    }
    #[test]
    fn sleep_inhibits_temporary_targets_below_full_and_retains_wake_hysteresis() {
        for saved_limit in [None, Some(80)] {
            let mut f = Fake::default();
            let mut p = Policy::default();
            p.set_limit(saved_limit, None, &f.capabilities()).unwrap();
            p.charge_to(90, &f.capabilities()).unwrap();
            p.tick(&mut f, &battery(50)).unwrap();
            assert!(f.enabled);
            p.prepare_sleep(&mut f).unwrap();
            assert!(!f.enabled);
            p.tick(&mut f, &battery(88)).unwrap();
            assert!(
                !f.enabled,
                "wake must retain inhibition inside the target band"
            );
            p.tick(&mut f, &battery(85)).unwrap();
            assert!(f.enabled);
            p.charge_to(100, &f.capabilities()).unwrap();
            p.tick(&mut f, &battery(88)).unwrap();
            p.prepare_sleep(&mut f).unwrap();
            assert!(f.enabled, "an explicit full charge remains allowed");
        }
    }
    #[test]
    fn sleep_cancels_forced_discharge_without_releasing_the_saved_limit() {
        let mut f = Fake {
            enabled: true,
            stop: None,
            discharge: false,
            ..Fake::default()
        };
        let mut p = Policy::default();
        p.set_limit(Some(80), None, &f.capabilities()).unwrap();
        p.tick(&mut f, &battery(90)).unwrap();
        p.discharge(70, &f.capabilities()).unwrap();
        p.tick(&mut f, &battery(90)).unwrap();
        assert!(f.discharge);
        p.prepare_sleep(&mut f).unwrap();
        assert!(!f.discharge);
        assert!(!f.enabled);
        assert_eq!(f.stop, Some(80));
        assert_eq!(p.discharge_target, None);
        assert_eq!(p.config.charge_limit, Some(80));
    }
    #[test]
    fn hysteresis_top_up_and_restore() {
        let mut f = Fake {
            enabled: true,
            stop: None,
            discharge: false,
            ..Fake::default()
        };
        let mut p = Policy::default();
        p.set_limit(Some(80), Some(75), &f.capabilities()).unwrap();
        p.tick(&mut f, &battery(80)).unwrap();
        assert!(!f.enabled);
        p.tick(&mut f, &battery(79)).unwrap();
        assert!(!f.enabled);
        p.tick(&mut f, &battery(75)).unwrap();
        assert!(f.enabled);
        p.top_up(&f.capabilities()).unwrap();
        p.tick(&mut f, &battery(90)).unwrap();
        assert_eq!(f.stop, None);
        p.tick(&mut f, &battery(100)).unwrap();
        assert_eq!(f.stop, Some(80));
        assert!(!p.top_up);
        assert_eq!(p.config.charge_limit, Some(80));
    }
    #[test]
    fn discharge_stops_at_target_and_sensor_loss() {
        let mut f = Fake {
            enabled: true,
            stop: None,
            discharge: false,
            ..Fake::default()
        };
        let mut p = Policy::default();
        p.discharge(80, &f.capabilities()).unwrap();
        p.tick(&mut f, &battery(90)).unwrap();
        assert!(f.discharge);
        p.tick(&mut f, &battery(80)).unwrap();
        assert!(!f.discharge);
        p.discharge(80, &f.capabilities()).unwrap();
        p.tick(&mut f, &battery(90)).unwrap();
        let mut b = battery(90);
        b.percent = None;
        assert!(p.tick(&mut f, &b).is_err());
        assert!(!f.discharge);
    }
    #[test]
    fn changing_or_disabling_limit_cancels_ephemeral_overrides() {
        let mut p = Policy::default();
        let f = Fake {
            enabled: true,
            stop: None,
            discharge: false,
            ..Fake::default()
        };
        let caps = f.capabilities();
        p.set_limit(Some(80), None, &caps).unwrap();
        p.top_up(&caps).unwrap();
        p.set_limit(None, None, &caps).unwrap();
        assert!(!p.top_up);
        assert_eq!(p.config.charge_limit, None);
        p.discharge(80, &caps).unwrap();
        p.set_limit(Some(85), None, &caps).unwrap();
        assert_eq!(p.discharge_target, None);
    }
    #[test]
    fn unsupported_controller_is_never_written_even_for_default_policy() {
        struct Unsupported(bool);
        impl ChargeController for Unsupported {
            fn capabilities(&self) -> Capabilities {
                Capabilities::unsupported("fixture")
            }
            fn apply(&mut self, _: Option<u8>, _: Option<u8>) -> Result<()> {
                self.0 = true;
                Ok(())
            }
            fn release(&mut self, _: Release) -> Result<()> {
                self.0 = true;
                Ok(())
            }
        }
        let mut controller = Unsupported(false);
        assert!(
            Policy::default()
                .tick(&mut controller, &battery(74))
                .is_err()
        );
        assert!(!controller.0);
    }
    #[test]
    fn calibration_is_the_only_route_below_twenty_and_restores_saved_policy() {
        let mut f = Fake::default();
        let caps = f.capabilities();
        let mut p = Policy::default();
        p.set_limit(Some(80), None, &caps).unwrap();
        assert!(p.discharge(10, &caps).is_err());
        p.calibrate(&caps, 1000).unwrap();
        p.tick_at(&mut f, &battery(90), 1001).unwrap();
        assert!(f.enabled);
        assert!(!f.discharge);
        p.tick_at(&mut f, &battery(100), 1100).unwrap();
        assert!(f.discharge);
        p.tick_at(&mut f, &battery(11), 1200).unwrap();
        assert!(f.discharge);
        p.tick_at(&mut f, &battery(10), 1300).unwrap();
        assert!(!f.discharge);
        assert!(f.enabled);
        p.tick_at(&mut f, &battery(100), 1400).unwrap();
        assert_eq!(
            p.runtime.calibration.as_ref().unwrap().phase,
            CalibrationPhase::Resting
        );
        p.tick_at(&mut f, &battery(100), 4999).unwrap();
        assert!(p.runtime.calibration.is_some());
        p.tick_at(&mut f, &battery(100), 5000).unwrap();
        assert!(p.runtime.calibration.is_none());
        assert_eq!(f.stop, Some(80));
        assert!(!f.enabled);
        assert_eq!(p.config.charge_limit, Some(80));
    }
    #[test]
    fn heat_hysteresis_sensor_loss_and_timeout_take_priority_over_calibration() {
        let mut f = Fake::default();
        let caps = f.capabilities();
        let mut p = Policy::default();
        p.set_care(
            CareSettings {
                heat_limit_c: Some(35),
                prevent_sleep: true,
                led: LedMode::Automatic,
                ..CareSettings::default()
            },
            &caps,
        )
        .unwrap();
        p.calibrate(&caps, 1000).unwrap();
        let mut b = battery(50);
        b.telemetry.temperature_deci_c = Some(350);
        p.tick_at(&mut f, &b, 1001).unwrap();
        assert!(!f.enabled);
        assert!(!f.discharge);
        assert!(!f.awake);
        b.telemetry.temperature_deci_c = Some(340);
        p.tick_at(&mut f, &b, 1002).unwrap();
        assert!(!f.enabled);
        b.telemetry.temperature_deci_c = Some(330);
        p.tick_at(&mut f, &b, 1003).unwrap();
        assert!(f.enabled);
        assert!(f.awake);
        assert_eq!(f.led, LedMode::Orange);
        b.telemetry.temperature_deci_c = None;
        p.tick_at(&mut f, &b, 1004).unwrap();
        assert!(!f.enabled);
        assert!(p.runtime.temperature_unavailable);
        assert!(p.tick_at(&mut f, &b, 87401).is_err());
        assert!(p.runtime.calibration.is_none());
        assert!(!f.awake);
    }
    #[test]
    fn unplug_and_sleep_cancel_calibration_and_release_isolation() {
        let mut f = Fake::default();
        let caps = f.capabilities();
        let mut p = Policy::default();
        p.calibrate(&caps, 1000).unwrap();
        p.tick_at(&mut f, &battery(100), 1001).unwrap();
        assert!(f.discharge);
        let mut b = battery(99);
        b.on_ac = Some(false);
        assert!(p.tick_at(&mut f, &b, 1002).is_err());
        assert!(!f.discharge);
        assert!(p.runtime.calibration.is_none());
        p.calibrate(&caps, 1100).unwrap();
        p.tick_at(&mut f, &battery(100), 1101).unwrap();
        p.prepare_sleep(&mut f).unwrap();
        assert!(!f.discharge);
        assert!(p.runtime.calibration.is_none());
    }
    #[test]
    fn temporary_target_holds_until_cancel_and_pause_releases_all_controls() {
        let mut f = Fake::default();
        let caps = f.capabilities();
        let mut p = Policy::default();
        p.set_limit(Some(80), None, &caps).unwrap();
        p.tick(&mut f, &battery(85)).unwrap();
        assert!(!f.enabled);
        p.charge_to(90, &caps).unwrap();
        p.tick(&mut f, &battery(87)).unwrap();
        assert!(f.enabled);
        p.tick(&mut f, &battery(90)).unwrap();
        assert!(!f.enabled);
        p.tick(&mut f, &battery(89)).unwrap();
        assert!(!f.enabled);
        assert_eq!(p.runtime.charge_target, Some(90));
        p.operation(&Command::Cancel, &caps, 0).unwrap();
        p.tick(&mut f, &battery(89)).unwrap();
        assert_eq!(f.stop, Some(80));
        p.pause();
        p.tick(&mut f, &battery(89)).unwrap();
        assert!(f.enabled);
        assert_eq!(f.stop, None);
        assert!(!f.discharge);
        assert!(!f.awake);
        assert_eq!(f.led, LedMode::System);
        assert_eq!(p.config.charge_limit, Some(80));
    }
    #[test]
    fn automatic_discharge_is_opt_in_and_cancel_does_not_immediately_restart_it() {
        let mut f = Fake::default();
        let caps = f.capabilities();
        let mut p = Policy::default();
        p.set_limit(Some(80), None, &caps).unwrap();
        p.tick(&mut f, &battery(90)).unwrap();
        assert!(!f.discharge);
        p.set_care(
            CareSettings {
                automatic_discharge: true,
                ..CareSettings::default()
            },
            &caps,
        )
        .unwrap();
        p.tick(&mut f, &battery(90)).unwrap();
        assert!(f.discharge);
        p.operation(&Command::Cancel, &caps, 0).unwrap();
        p.tick(&mut f, &battery(90)).unwrap();
        assert!(!f.discharge);
        p.operation(&Command::Resume, &caps, 0).unwrap();
        p.tick(&mut f, &battery(90)).unwrap();
        assert!(f.discharge);
        assert!(p.set_limit(Some(10), None, &caps).is_err());
    }
    #[test]
    fn disabling_automatic_discharge_releases_only_the_automatic_operation() {
        let mut f = Fake::default();
        let caps = f.capabilities();
        let mut p = Policy::default();
        p.set_limit(Some(80), None, &caps).unwrap();
        p.set_care(
            CareSettings {
                automatic_discharge: true,
                ..CareSettings::default()
            },
            &caps,
        )
        .unwrap();
        p.tick(&mut f, &battery(90)).unwrap();
        assert!(f.discharge);
        p.set_care(CareSettings::default(), &caps).unwrap();
        p.tick(&mut f, &battery(90)).unwrap();
        assert!(!f.discharge);
        p.discharge(70, &caps).unwrap();
        p.set_care(CareSettings::default(), &caps).unwrap();
        p.tick(&mut f, &battery(90)).unwrap();
        assert!(f.discharge);
    }
    #[test]
    fn pause_releases_controls_even_if_saved_care_capability_is_no_longer_available() {
        let mut f = Fake::default();
        let mut p = Policy::default();
        p.config.care.heat_limit_c = Some(35);
        p.runtime.heat_paused = true;
        p.pause();
        p.config.care.schedules.push(Schedule {
            id: 1,
            enabled: true,
            next_at: 100,
            anchor_at: None,
            repeat: Repeat::Once,
            action: ScheduleAction::Discharge { target: 10 },
        });
        // An invalid saved schedule must not prevent releasing controls.
        assert!(p.validate_for(&f.capabilities()).is_err());
        p.tick(&mut f, &battery(80)).unwrap();
        assert!(f.enabled);
        assert!(!p.runtime.heat_paused);
    }
    #[test]
    fn wall_clock_jump_cancels_calibration() {
        let mut f = Fake::default();
        let caps = f.capabilities();
        let mut p = Policy::default();
        let now = chrono::Utc::now().timestamp();
        p.calibrate(&caps, now).unwrap();
        p.calibration_clock = Some((std::time::Instant::now(), now + 3600));
        assert!(p.tick(&mut f, &battery(90)).is_err());
        assert!(p.runtime.calibration.is_none());
        assert!(!f.discharge);
    }
}
