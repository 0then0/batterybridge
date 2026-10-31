//! Advanced care controls use the same validated service commands as the CLI.
use crate::{App, i18n};
use batterybridge_protocol::*;
use chrono::{Duration, Local, NaiveDateTime, TimeZone};
use egui::vec2;

pub(crate) struct Editor {
    target: u8,
    discharge: u8,
    calibration_confirmed: bool,
    date: String,
    repeat: Repeat,
    action: usize,
    schedule_target: u8,
}
impl Default for Editor {
    fn default() -> Self {
        Self {
            target: 100,
            discharge: 80,
            calibration_confirmed: false,
            date: (Local::now() + Duration::hours(1))
                .format("%Y-%m-%d %H:%M")
                .to_string(),
            repeat: Repeat::Once,
            action: 0,
            schedule_target: 80,
        }
    }
}
fn full_button(ui: &mut egui::Ui, label: String) -> bool {
    ui.add(
        egui::Button::new(label)
            .wrap()
            .min_size(vec2(ui.available_width(), 40.0)),
    )
    .clicked()
}
impl App {
    pub(crate) fn care_view(&mut self, ui: &mut egui::Ui) {
        let Some(snapshot) = self.model.snapshot.clone() else {
            return;
        };
        let caps = &snapshot.capabilities;
        let locale = self.locale.clone();
        let t = |key| i18n::text(&locale, key);
        let enabled = !self.busy
            && !self.preview
            && snapshot.config.is_some()
            && caps.charge_limit.available();
        if caps.charge_limit.available() {
            if let Some(cycle) = &snapshot.runtime.calibration {
                ui.heading(format!(
                    "{}: {}",
                    t("calibration"),
                    t(phase_key(cycle.phase))
                ));
                ui.add_enabled_ui(enabled, |ui| {
                    if full_button(ui, t("cancelOperation")) {
                        self.request(Command::Cancel);
                    }
                });
            }
            egui::CollapsingHeader::new(t("temporaryModes"))
                .default_open(cfg!(test))
                .show(ui, |ui| {
                    ui.add_enabled_ui(enabled, |ui| {
                        ui.label(t("temporaryHelp"));
                        let label = ui.label(t("chargeTo"));
                        if let Some(limits) = &caps.allowed_limits {
                            if !limits.contains(&self.care_editor.target) {
                                self.care_editor.target = limits.first().copied().unwrap_or(80);
                            }
                            egui::ComboBox::from_id_salt("temporary_target")
                                .selected_text(format!("{}%", self.care_editor.target))
                                .show_ui(ui, |ui| {
                                    for target in limits {
                                        ui.selectable_value(
                                            &mut self.care_editor.target,
                                            *target,
                                            format!("{target}%"),
                                        );
                                    }
                                })
                                .response
                                .labelled_by(label.id);
                        } else {
                            ui.add(
                                egui::DragValue::new(&mut self.care_editor.target)
                                    .range(1..=100)
                                    .suffix("%"),
                            )
                            .labelled_by(label.id);
                        }
                        if full_button(ui, t("startChargeTo")) {
                            self.request(Command::ChargeTo {
                                target: self.care_editor.target,
                            });
                        }
                        if caps.force_discharge.available() {
                            let label = ui.label(t("dischargeTo"));
                            ui.add(
                                egui::DragValue::new(&mut self.care_editor.discharge)
                                    .range(20..=100)
                                    .suffix("%"),
                            )
                            .labelled_by(label.id);
                            if full_button(ui, t("startDischarge")) {
                                self.request(Command::Discharge {
                                    target: self.care_editor.discharge,
                                });
                            }
                        }
                        if caps.manual_charging.available() && full_button(ui, t("holdCharging")) {
                            self.request(Command::StopCharging);
                        }
                        if full_button(ui, t("pausePolicy")) {
                            self.request(Command::Pause);
                        }
                        if full_button(ui, t("restorePolicy")) {
                            self.request(Command::Resume);
                        }
                        if caps.adapter_disable.available() {
                            ui.label(t("adapterHelp"));
                            if full_button(ui, t("adapterOff")) {
                                self.request(Command::Adapter { enabled: false });
                            }
                        }
                    });
                    if snapshot.runtime.paused {
                        ui.label(t("policyPaused"));
                    }
                    if let Some(target) = snapshot.discharge_target {
                        ui.label(format!("{}: {target}%", t("dischargeTo")));
                    }
                    if let Some(cycle) = &snapshot.runtime.calibration {
                        ui.label(format!(
                            "{}: {}",
                            t("calibration"),
                            t(phase_key(cycle.phase))
                        ));
                    }
                    ui.add_enabled_ui(enabled, |ui| {
                        if full_button(ui, t("cancelOperation")) {
                            self.request(Command::Cancel);
                        }
                    });
                });
            if caps.force_discharge.available() && caps.temporary_full_charge.available() {
                egui::CollapsingHeader::new(t("calibration"))
                    .default_open(cfg!(test))
                    .show(ui, |ui| {
                        ui.label(t("calibrationHelp"));
                        ui.add_enabled_ui(
                            enabled && snapshot.runtime.calibration.is_none(),
                            |ui| {
                                ui.checkbox(
                                    &mut self.care_editor.calibration_confirmed,
                                    t("calibrationConfirm"),
                                );
                                if ui
                                    .add_enabled(
                                        self.care_editor.calibration_confirmed,
                                        egui::Button::new(t("startCalibration")).wrap(),
                                    )
                                    .clicked()
                                {
                                    self.request(Command::Calibrate);
                                    self.care_editor.calibration_confirmed = false;
                                }
                            },
                        );
                    });
            }
            egui::CollapsingHeader::new(t("careSettings"))
                .default_open(cfg!(test))
                .show(ui, |ui| {
                    let before = self.model.care.clone();
                    ui.add_enabled_ui(enabled, |ui| {
                        if caps.heat_protection.available()
                            || self.model.care.heat_limit_c.is_some()
                        {
                            let mut heat = self.model.care.heat_limit_c.is_some();
                            if ui.checkbox(&mut heat, t("heatProtection")).changed() {
                                self.model.care.heat_limit_c = heat.then_some(35);
                            }
                            if let Some(limit) = &mut self.model.care.heat_limit_c {
                                let label = ui.label(t("temperatureLimit"));
                                ui.add(egui::DragValue::new(limit).range(25..=60).suffix(" °C"))
                                    .labelled_by(label.id);
                            }
                            ui.label(t("heatHelp"));
                        }
                        if caps.force_discharge.available() || self.model.care.automatic_discharge {
                            ui.checkbox(
                                &mut self.model.care.automatic_discharge,
                                t("automaticDischarge"),
                            );
                        }
                        if caps.sleep_prevention.available() || self.model.care.prevent_sleep {
                            ui.checkbox(&mut self.model.care.prevent_sleep, t("preventSleep"));
                            ui.label(t("preventSleepHelp"));
                        }
                        if caps.magsafe_led.available() || self.model.care.led != LedMode::System {
                            let label = ui.label(t("ledMode"));
                            egui::ComboBox::from_id_salt("led_mode")
                                .width(ui.available_width())
                                .selected_text(t(led_key(self.model.care.led)))
                                .show_ui(ui, |ui| {
                                    for mode in [
                                        LedMode::System,
                                        LedMode::Automatic,
                                        LedMode::Off,
                                        LedMode::Green,
                                        LedMode::Orange,
                                        LedMode::SlowBlinkOrange,
                                        LedMode::FastBlinkOrange,
                                    ] {
                                        ui.selectable_value(
                                            &mut self.model.care.led,
                                            mode,
                                            t(led_key(mode)),
                                        );
                                    }
                                })
                                .response
                                .labelled_by(label.id);
                        }
                        self.schedule_view(ui);
                        if self.model.care != before {
                            self.model.care_edited = true;
                            self.model.error = None;
                        }
                        if full_button(ui, t("saveCare")) {
                            self.request(Command::Care {
                                settings: self.model.care.clone(),
                                expected: self.model.care_base.clone(),
                            });
                        }
                        if full_button(ui, t("discardCare")) {
                            self.model.care_edited = false;
                            if let Some(config) = &snapshot.config {
                                self.model.care = config.care.clone();
                                self.model.care_base = config.care.clone();
                            }
                        }
                    });
                });
        }
        egui::CollapsingHeader::new(t("monitoring"))
            .default_open(cfg!(test))
            .show(ui, |ui| {
                let Some(battery) = snapshot.batteries.first() else {
                    ui.label(t("noBattery"));
                    return;
                };
                let telemetry = &battery.telemetry;
                ui.checkbox(&mut self.model.raw_percent, t("rawPercent"));
                ui.label(t("rawPercentHelp"));
                for (key, value) in [
                    (
                        "temperature",
                        telemetry
                            .temperature_deci_c
                            .map(|v| format!("{:.1} °C", f32::from(v) / 10.0)),
                    ),
                    (
                        "voltage",
                        telemetry
                            .voltage_mv
                            .map(|v| format!("{:.2} V", v as f32 / 1000.0)),
                    ),
                    (
                        "current",
                        telemetry
                            .current_ma
                            .map(|v| format!("{:.2} A", v as f32 / 1000.0)),
                    ),
                    (
                        "batteryPower",
                        telemetry
                            .power_mw
                            .map(|v| format!("{:+.2} W", v as f32 / 1000.0)),
                    ),
                    ("cycles", telemetry.cycle_count.map(|v| v.to_string())),
                    (
                        "remaining",
                        telemetry.remaining_minutes.map(|v| format!("{v} min")),
                    ),
                    ("health", battery.health_percent.map(|v| format!("{v}%"))),
                ] {
                    ui.label(format!(
                        "{}: {}",
                        t(key),
                        value.unwrap_or_else(|| t("unknown"))
                    ));
                }
                ui.label(t("powerHelp"));
                ui.label(t("historyHelp"));
                if self.model.history.len() > 1 {
                    let (rect, _) = ui.allocate_exact_size(
                        vec2(ui.available_width(), 100.0),
                        egui::Sense::hover(),
                    );
                    let start = self.model.history.front().map_or(0, |(at, _)| *at);
                    let end = self.model.history.back().map_or(start + 1, |(at, _)| *at);
                    let points = self
                        .model
                        .history
                        .iter()
                        .map(|(at, percent)| {
                            egui::pos2(
                                rect.left()
                                    + rect.width() * (*at - start) as f32
                                        / (end - start).max(1) as f32,
                                rect.bottom() - rect.height() * f32::from(*percent) / 100.0,
                            )
                        })
                        .collect();
                    ui.painter().rect_stroke(
                        rect,
                        0.0,
                        (1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
                        egui::StrokeKind::Inside,
                    );
                    ui.painter()
                        .add(egui::Shape::line(points, (2.0, crate::view::accent(ui))));
                }
            });
        egui::CollapsingHeader::new(t("automation"))
            .default_open(cfg!(test))
            .show(ui, |ui| {
                ui.label(t("automationHelp"));
                #[cfg(target_os = "macos")]
                let executable = "/Library/PrivilegedHelperTools/io.github.batterybridge.cli";
                #[cfg(not(target_os = "macos"))]
                let executable = "batterybridge";
                for (key, command) in [
                    ("topUp", "top-up"),
                    ("pausePolicy", "pause"),
                    ("restorePolicy", "resume"),
                    ("holdCharging", "hold"),
                    ("apply", "limit 80"),
                ] {
                    if full_button(ui, t("copyCommand").replace("{action}", &t(key))) {
                        ui.ctx().copy_text(format!("\"{executable}\" {command}"));
                    }
                }
            });
        ui.add_space(12.0);
    }
    fn schedule_view(&mut self, ui: &mut egui::Ui) {
        let locale = self.locale.clone();
        let t = |key| i18n::text(&locale, key);
        egui::CollapsingHeader::new(t("schedules"))
            .default_open(cfg!(test))
            .show(ui, |ui| {
                ui.label(t("scheduleHelp"));
                let mut remove = None;
                for (index, task) in self.model.care.schedules.iter_mut().enumerate() {
                    ui.push_id(task.id, |ui| {
                        ui.separator();
                        ui.checkbox(
                            &mut task.enabled,
                            format!(
                                "#{} · {}",
                                task.id,
                                Local
                                    .timestamp_opt(task.next_at, 0)
                                    .single()
                                    .map(|v| v.format("%Y-%m-%d %H:%M %Z").to_string())
                                    .unwrap_or_default()
                            ),
                        );
                        ui.label(format!(
                            "{} · {}",
                            t(repeat_key(task.repeat)),
                            t(action_key(&task.action))
                        ));
                        if ui.button(t("removeSchedule")).clicked() {
                            remove = Some(index);
                        }
                    });
                }
                if let Some(index) = remove {
                    self.model.care.schedules.remove(index);
                }
                let label = ui.label(t("scheduleDate"));
                ui.add(
                    egui::TextEdit::singleline(&mut self.care_editor.date)
                        .desired_width(f32::INFINITY),
                )
                .labelled_by(label.id);
                let label = ui.label(t("scheduleRepeat"));
                egui::ComboBox::from_id_salt("repeat")
                    .width(ui.available_width())
                    .selected_text(t(repeat_key(self.care_editor.repeat)))
                    .show_ui(ui, |ui| {
                        for repeat in [
                            Repeat::Once,
                            Repeat::Daily,
                            Repeat::Weekdays,
                            Repeat::Weekly,
                            Repeat::Biweekly,
                            Repeat::Monthly,
                            Repeat::Yearly,
                        ] {
                            ui.selectable_value(
                                &mut self.care_editor.repeat,
                                repeat,
                                t(repeat_key(repeat)),
                            );
                        }
                    })
                    .response
                    .labelled_by(label.id);
                let keys = [
                    "chargeLimit",
                    "topUp",
                    "chargeTo",
                    "dischargeTo",
                    "holdCharging",
                    "pausePolicy",
                    "restorePolicy",
                    "calibration",
                ];
                let caps = &self.model.snapshot.as_ref().expect("snapshot").capabilities;
                let label = ui.label(t("scheduleAction"));
                egui::ComboBox::from_id_salt("schedule_action")
                    .width(ui.available_width())
                    .selected_text(t(keys[self.care_editor.action]))
                    .show_ui(ui, |ui| {
                        for (index, key) in keys.iter().enumerate() {
                            let available = match index {
                                1 => caps.temporary_full_charge.available(),
                                3 => caps.force_discharge.available(),
                                4 => caps.manual_charging.available(),
                                7 => {
                                    caps.force_discharge.available()
                                        && caps.temporary_full_charge.available()
                                }
                                _ => true,
                            };
                            ui.add_enabled_ui(available, |ui| {
                                ui.selectable_value(&mut self.care_editor.action, index, t(key));
                            });
                        }
                    })
                    .response
                    .labelled_by(label.id);
                if [0, 2, 3].contains(&self.care_editor.action) {
                    let label = ui.label(t("targetPercent"));
                    ui.add(
                        egui::DragValue::new(&mut self.care_editor.schedule_target)
                            .range(if self.care_editor.action == 3 {
                                20..=100
                            } else {
                                1..=100
                            })
                            .suffix("%"),
                    )
                    .labelled_by(label.id);
                }
                if self.care_editor.action == 7 {
                    ui.label(t("calibrationHelp"));
                    ui.checkbox(
                        &mut self.care_editor.calibration_confirmed,
                        t("calibrationConfirm"),
                    );
                }
                if ui
                    .add_enabled(
                        self.model.care.schedules.len() < 16
                            && (self.care_editor.action != 7
                                || self.care_editor.calibration_confirmed),
                        egui::Button::new(t("addSchedule")).wrap(),
                    )
                    .clicked()
                {
                    match schedule_from_editor(&self.care_editor, &self.model.care.schedules) {
                        Ok(task) => self.model.care.schedules.push(task),
                        Err(_) => self.model.error = Some(t("invalidScheduleDate")),
                    }
                }
            });
    }
}
fn schedule_from_editor(editor: &Editor, existing: &[Schedule]) -> Result<Schedule> {
    let date = NaiveDateTime::parse_from_str(&editor.date, "%Y-%m-%d %H:%M")
        .map_err(|e| Error::ConfigurationError(e.to_string()))?;
    let at = Local
        .from_local_datetime(&date)
        .earliest()
        .filter(|v| *v > Local::now())
        .ok_or_else(|| Error::ConfigurationError("future local time required".into()))?;
    let target = editor.schedule_target;
    let action = match editor.action {
        0 => ScheduleAction::Limit {
            stop: target,
            start: None,
        },
        1 => ScheduleAction::TopUp,
        2 => ScheduleAction::ChargeTo { target },
        3 => ScheduleAction::Discharge { target },
        4 => ScheduleAction::Hold,
        5 => ScheduleAction::Pause,
        6 => ScheduleAction::Resume,
        _ => ScheduleAction::Calibrate,
    };
    let id = (1..=17)
        .find(|id| existing.iter().all(|s| s.id != *id))
        .ok_or_else(|| Error::ConfigurationError("schedule limit".into()))?;
    Ok(Schedule {
        id,
        enabled: true,
        next_at: at.timestamp(),
        anchor_at: None,
        repeat: editor.repeat,
        action,
    })
}
fn phase_key(phase: CalibrationPhase) -> &'static str {
    match phase {
        CalibrationPhase::Charging => "phaseCharging",
        CalibrationPhase::Discharging => "phaseDischarging",
        CalibrationPhase::Recharging => "phaseRecharging",
        CalibrationPhase::Resting => "phaseResting",
    }
}
fn led_key(mode: LedMode) -> &'static str {
    match mode {
        LedMode::System => "ledSystem",
        LedMode::Automatic => "ledAutomatic",
        LedMode::Off => "off",
        LedMode::Green => "ledGreen",
        LedMode::Orange => "ledOrange",
        LedMode::SlowBlinkOrange => "ledSlow",
        LedMode::FastBlinkOrange => "ledFast",
    }
}
fn repeat_key(repeat: Repeat) -> &'static str {
    match repeat {
        Repeat::Once => "repeatOnce",
        Repeat::Daily => "repeatDaily",
        Repeat::Weekdays => "repeatWeekdays",
        Repeat::Weekly => "repeatWeekly",
        Repeat::Biweekly => "repeatBiweekly",
        Repeat::Monthly => "repeatMonthly",
        Repeat::Yearly => "repeatYearly",
    }
}
fn action_key(action: &ScheduleAction) -> &'static str {
    match action {
        ScheduleAction::Limit { .. } => "chargeLimit",
        ScheduleAction::TopUp => "topUp",
        ScheduleAction::ChargeTo { .. } => "chargeTo",
        ScheduleAction::Discharge { .. } => "dischargeTo",
        ScheduleAction::Hold => "holdCharging",
        ScheduleAction::Pause => "pausePolicy",
        ScheduleAction::Resume => "restorePolicy",
        ScheduleAction::Calibrate => "calibration",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Options;
    use egui::accesskit::{Action, ActionRequest, Role, TreeId, TreeUpdate};
    use fastframe_shell::Waker;
    use std::sync::Mutex;
    static COMMANDS: Mutex<Vec<Command>> = Mutex::new(Vec::new());
    fn app() -> App {
        App::new(
            Waker::default(),
            Options {
                preview: Some(
                    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/status.json"),
                ),
                language: Some("en".into()),
                ..Default::default()
            },
        )
        .unwrap()
    }
    fn frame(
        ctx: &egui::Context,
        app: &mut App,
        target: Option<egui::accesskit::NodeId>,
    ) -> TreeUpdate {
        let events = target
            .map(|target_node| {
                egui::Event::AccessKitActionRequest(ActionRequest {
                    action: Action::Click,
                    target_tree: TreeId::ROOT,
                    target_node,
                    data: None,
                })
            })
            .into_iter()
            .collect();
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    vec2(420.0, 12000.0),
                )),
                events,
                ..Default::default()
            },
            |root| {
                egui::CentralPanel::default().show(root, |ui| app.care_view(ui));
            },
        );
        output.textures_delta.clear();
        output.platform_output.accesskit_update.take().unwrap()
    }
    fn node<'a>(
        tree: &'a TreeUpdate,
        label: &str,
    ) -> &'a (egui::accesskit::NodeId, egui::accesskit::Node) {
        tree.nodes
            .iter()
            .find(|(_, node)| node.label() == Some(label))
            .unwrap_or_else(|| panic!("missing {label}"))
    }
    #[test]
    fn calibration_requires_explicit_confirmation_and_sends_only_typed_command() {
        fn query(command: Command) -> std::result::Result<Response, String> {
            COMMANDS.lock().unwrap().push(command);
            Ok(Response::Ok)
        }
        COMMANDS.lock().unwrap().clear();
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut app = app();
        app.preview = false;
        app.query = query;
        let tree = frame(&ctx, &mut app, None);
        assert!(node(&tree, "Start calibration").1.is_disabled());
        let agree = node(&tree, "I agree to discharge to 10%").0;
        frame(&ctx, &mut app, Some(agree));
        assert!(app.care_editor.calibration_confirmed);
        let tree = frame(&ctx, &mut app, None);
        assert!(!node(&tree, "Start calibration").1.is_disabled());
        frame(&ctx, &mut app, Some(node(&tree, "Start calibration").0));
        let reply = app
            .replies
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        drop(reply);
        assert!(matches!(
            COMMANDS.lock().unwrap().as_slice(),
            [Command::Calibrate]
        ));
        assert!(!app.care_editor.calibration_confirmed);
    }
    #[test]
    fn new_mutations_remain_disabled_in_preview_and_read_only_mode() {
        for offline in [false, true] {
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            let mut app = app();
            if offline {
                app.preview = false;
                app.model.snapshot.as_mut().unwrap().config = None;
            }
            let tree = frame(&ctx, &mut app, None);
            for label in [
                "Charge and hold at target",
                "Start discharge",
                "Pause battery control",
                "Save care settings and schedules",
                "Start calibration",
            ] {
                assert!(node(&tree, label).1.is_disabled(), "{label}");
            }
            let checkbox = node(&tree, "I agree to discharge to 10%");
            assert_eq!(checkbox.1.role(), Role::CheckBox);
            assert!(checkbox.1.is_disabled());
        }
    }
    #[test]
    fn invalid_schedule_can_be_corrected_without_losing_the_draft() {
        let mut editor = Editor {
            date: "invalid".into(),
            ..Editor::default()
        };
        assert!(schedule_from_editor(&editor, &[]).is_err());
        editor.date = (Local::now() + Duration::days(2))
            .format("%Y-%m-%d %H:%M")
            .to_string();
        editor.action = 3;
        editor.schedule_target = 20;
        let task = schedule_from_editor(&editor, &[]).unwrap();
        assert_eq!(task.action, ScheduleAction::Discharge { target: 20 });
        let second = schedule_from_editor(&editor, std::slice::from_ref(&task)).unwrap();
        assert_ne!(task.id, second.id);
    }
}
