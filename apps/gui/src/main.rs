#![forbid(unsafe_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod care;
mod i18n;
mod model;
mod platform;
mod startup;
mod view;

use batterybridge_protocol::{Command, Response};
use fastframe_shell::{Closed, Headless, Held, Resident, Shell, Waker};
use fastframe_tray::{Event, MenuItem, Tray};
use std::{
    path::PathBuf,
    sync::{
        OnceLock,
        atomic::{AtomicU8, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

const REFRESH: Duration = Duration::from_secs(30);
const APP_ID: &str = "io.github.batterybridge";

#[derive(Default)]
struct Options {
    preview: Option<PathBuf>,
    language: Option<String>,
    start_hidden: bool,
}
impl Options {
    fn parse() -> Result<Option<Self>, String> {
        Self::parse_args(std::env::args().skip(1))
    }
    fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Option<Self>, String> {
        let mut options = Self::default();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--help" | "-h" => {
                    println!(
                        "BatteryBridge\nUsage: batterybridge-gui [--start-hidden] [--preview SNAPSHOT.json [--language LOCALE]]"
                    );
                    return Ok(None);
                }
                "--version" | "-V" => {
                    println!("BatteryBridge {}", env!("CARGO_PKG_VERSION"));
                    return Ok(None);
                }
                "--preview" => {
                    options.preview =
                        Some(args.next().ok_or("--preview needs a snapshot path")?.into())
                }
                "--language" => {
                    options.language = Some(args.next().ok_or("--language needs a locale")?)
                }
                fastframe_autostart::START_HIDDEN => options.start_hidden = true,
                _ => return Err(format!("Unknown argument: {arg}")),
            }
        }
        if options
            .language
            .as_ref()
            .is_some_and(|s| !i18n::valid_choice(s))
        {
            return Err("Unsupported language".into());
        }
        if options.language.is_some() && options.preview.is_none() {
            return Err(
                "--language is a preview option; use the application's language selector".into(),
            );
        }
        Ok(Some(options))
    }
}

enum Reply {
    Protocol(Result<Response, String>),
    Autostart(Result<fastframe_autostart::State, String>),
    Uninstalled(Result<(), String>),
    Saved(Result<(), String>),
}

pub(crate) struct App {
    model: model::Model,
    language: String,
    locale: String,
    preferences_loaded: bool,
    startup_state: Option<fastframe_autostart::State>,
    startup_busy: bool,
    startup_error: Option<String>,
    startup_query: fn(Option<bool>) -> Result<fastframe_autostart::State, String>,
    tray: Option<Tray>,
    waker: Waker,
    sender: mpsc::Sender<Reply>,
    replies: mpsc::Receiver<Reply>,
    busy: bool,
    refreshed: Instant,
    report: Option<String>,
    report_destination: String,
    report_saved: bool,
    confirm_uninstall: bool,
    wants_show: bool,
    hide: bool,
    quit: bool,
    preview: bool,
    // UI interaction tests substitute this transport instead of accessing the live service.
    care_editor: care::Editor,
    pending_command: Option<Command>,
    query: fn(Command) -> Result<Response, String>,
}
impl App {
    fn new(waker: Waker, options: Options) -> Result<Self, String> {
        let (sender, replies) = mpsc::channel();
        let mut model = model::Model::default();
        let preview = options.preview.is_some();
        if let Some(path) = options.preview {
            model.accept(
                serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?,
            );
        }
        let language = options
            .language
            .or_else(|| (!preview).then(startup::saved_language).flatten())
            .unwrap_or_else(|| "system".into());
        let locale = i18n::resolve(&language, fastframe_i18n::system_languages()).to_owned();
        let tray = if preview {
            None
        } else {
            let wake = waker.clone();
            Tray::spawn(
                fastframe_tray::Config {
                    id: APP_ID,
                    title: "BatteryBridge".into(),
                    icon: tray_icon,
                    template_icon: None,
                    themed_icon: true,
                    menu: vec![
                        MenuItem::action("open", i18n::text(&locale, "trayOpen")),
                        MenuItem::action("topup", i18n::text(&locale, "topUp")),
                        MenuItem::action("pause", i18n::text(&locale, "pausePolicy")),
                        MenuItem::action("resume", i18n::text(&locale, "restorePolicy")),
                        MenuItem::action("quit", i18n::text(&locale, "trayQuit")),
                    ],
                },
                move || wake.wake(),
            )
        };
        Ok(Self {
            model,
            language,
            locale,
            preferences_loaded: preview,
            startup_state: preview.then_some(fastframe_autostart::State::Off),
            startup_busy: false,
            startup_error: None,
            startup_query: startup::query,
            tray,
            waker,
            sender,
            replies,
            busy: false,
            refreshed: Instant::now() - REFRESH,
            report: None,
            report_destination: platform::report_path().to_string_lossy().into_owned(),
            report_saved: false,
            confirm_uninstall: false,
            wants_show: false,
            hide: false,
            quit: false,
            preview,
            care_editor: care::Editor::default(),
            pending_command: None,
            query: |command| {
                batterybridge_platform::client::query(command, batterybridge_ipc::call)
                    .map_err(|e| e.to_string())
            },
        })
    }
    fn text(&self, key: &str) -> String {
        i18n::text(&self.locale, key)
    }
    fn language_changed(&mut self) {
        self.locale = i18n::resolve(&self.language, fastframe_i18n::system_languages()).to_owned();
        if let Some(tray) = &mut self.tray {
            tray.set_label("open", i18n::text(&self.locale, "trayOpen"));
            tray.set_label("topup", i18n::text(&self.locale, "topUp"));
            tray.set_label("pause", i18n::text(&self.locale, "pausePolicy"));
            tray.set_label("resume", i18n::text(&self.locale, "restorePolicy"));
            tray.set_label("quit", i18n::text(&self.locale, "trayQuit"));
        }
    }
    fn attach(&mut self, cc: &eframe::CreationContext<'_>) {
        if !self.preferences_loaded {
            self.preferences_loaded = true;
            if let Some(saved) = cc.storage.and_then(|s| s.get_string("language"))
                && i18n::valid_choice(&saved)
            {
                self.language = saved;
            }
            self.language_changed();
        }
        fastframe_fonts::FontSetup::default().install(&cc.egui_ctx);
        view::style(&cc.egui_ctx);
        if let Some(tray) = &mut self.tray {
            tray.attach();
            #[cfg(target_os = "macos")]
            if tray.is_shown() {
                fastframe_dock::set_dock_icon_visible(false);
            }
        }
        self.refreshed = Instant::now() - REFRESH;
        if self.startup_state.is_none() {
            self.request_autostart(None);
        }
    }
    fn request_autostart(&mut self, enabled: Option<bool>) {
        if self.preview || self.startup_busy || self.confirm_uninstall {
            return;
        }
        self.startup_busy = true;
        self.startup_error = None;
        let sender = self.sender.clone();
        let waker = self.waker.clone();
        let query = self.startup_query;
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(move || query(enabled))
                .unwrap_or_else(|_| Err("Startup operation failed".into()));
            let _ = sender.send(Reply::Autostart(result));
            waker.wake();
        });
    }
    fn spawn(&mut self, job: impl FnOnce() -> Reply + Send + std::panic::UnwindSafe + 'static) {
        if self.busy {
            return;
        }
        self.busy = true;
        let sender = self.sender.clone();
        let waker = self.waker.clone();
        std::thread::spawn(move || {
            let _ = sender
                .send(std::panic::catch_unwind(job).unwrap_or_else(|_| {
                    Reply::Protocol(Err("Background operation failed".into()))
                }));
            waker.wake();
        });
    }
    fn request(&mut self, command: Command) {
        if self.preview || self.busy {
            return;
        }
        if !matches!(command, Command::Status | Command::Probe) && self.model.read_only() {
            return;
        }
        if let Err(error) = command.validate() {
            self.model.error = Some(error.to_string());
            return;
        }
        if !matches!(command, Command::Status) {
            self.model.updated = false;
        }
        self.model.error = None;
        self.pending_command = Some(command.clone());
        let query = self.query;
        self.spawn(move || Reply::Protocol(query(command)));
    }
    fn tray_status(&mut self) {
        if let (Some(tray), Some(snapshot)) = (&mut self.tray, &self.model.snapshot) {
            let percent = snapshot
                .batteries
                .first()
                .and_then(|b| b.percent)
                .map_or_else(|| "?".into(), |p| p.to_string());
            TRAY_PERCENT.store(
                snapshot
                    .batteries
                    .first()
                    .and_then(|b| b.percent)
                    .unwrap_or(255),
                Ordering::Relaxed,
            );
            tray.set_icon(tray_icon, None);
            let state = snapshot
                .batteries
                .first()
                .map(|b| i18n::battery_state(&self.locale, &b.state))
                .unwrap_or_default();
            tray.set_tooltip(format!("BatteryBridge · {percent}% · {state}"));
            let writable =
                snapshot.config.is_some() && snapshot.capabilities.charge_limit.available();
            tray.set_enabled(
                "topup",
                writable
                    && snapshot.capabilities.temporary_full_charge.available()
                    && snapshot
                        .config
                        .as_ref()
                        .is_some_and(|c| c.charge_limit.is_some()),
            );
            tray.set_enabled("pause", writable && !snapshot.runtime.paused);
            tray.set_enabled("resume", writable);
        }
    }
    fn show_window(&mut self, ctx: &egui::Context, window_exists: bool) {
        if window_exists {
            #[cfg(target_os = "macos")]
            {
                self.hide = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        } else {
            self.wants_show = true;
        }
    }
    fn handle_close_requested(&mut self, ctx: &egui::Context, tray_shown: bool) {
        self.hide = tray_shown && !self.quit;
        #[cfg(target_os = "macos")]
        if self.hide {
            // Ending run_native leaves the status item in fastframe's headless
            // AppKit pump, where clicks stop arriving on macOS 15.7.9. Keep winit's
            // native event loop alive while the window is hidden instead.
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        #[cfg(not(target_os = "macos"))]
        let _ = ctx;
    }
    fn events(&mut self, ctx: &egui::Context, visible: bool) {
        while let Ok(reply) = self.replies.try_recv() {
            if !matches!(&reply, Reply::Autostart(_)) {
                self.busy = false;
            }
            match reply {
                Reply::Autostart(result) => {
                    self.startup_busy = false;
                    match result {
                        Ok(state) => {
                            self.startup_state = Some(state);
                            self.startup_error = None;
                        }
                        Err(error) => self.startup_error = Some(error),
                    }
                }
                Reply::Protocol(Ok(Response::Status(snapshot))) => {
                    self.model.accept(*snapshot);
                    self.tray_status();
                    self.refreshed = Instant::now();
                }
                Reply::Protocol(Ok(Response::Ok)) => {
                    match self.pending_command.take() {
                        Some(Command::Limit { .. }) => self.model.edited = false,
                        Some(Command::Care { .. }) => self.model.care_edited = false,
                        Some(Command::Reset) => {
                            self.model.edited = false;
                            self.model.care_edited = false;
                        }
                        _ => {}
                    }
                    self.model.updated = true;
                    self.request(Command::Status);
                }
                Reply::Protocol(Ok(Response::Probe(report))) => {
                    match serde_json::to_string_pretty(&report) {
                        Ok(report) => {
                            self.report = Some(report);
                            self.report_saved = false;
                        }
                        Err(error) => self.model.error = Some(error.to_string()),
                    }
                }
                Reply::Protocol(Ok(Response::Error(error))) => {
                    self.model.error = Some(error.to_string())
                }
                Reply::Protocol(Ok(_)) => {
                    self.model.error = Some("Unexpected service response".into())
                }
                Reply::Protocol(Err(error)) => {
                    self.model.error = Some(error);
                    self.refreshed = Instant::now();
                }
                Reply::Uninstalled(Ok(())) => self.quit = true,
                Reply::Uninstalled(Err(error)) | Reply::Saved(Err(error)) => {
                    self.model.error = Some(error)
                }
                Reply::Saved(Ok(())) => self.report_saved = true,
            }
        }
        let events = self
            .tray
            .as_ref()
            .map(|tray| tray.events())
            .unwrap_or_default();
        for event in events {
            match event {
                Event::Menu("quit") => self.quit = true,
                Event::Toggle | Event::Show | Event::Menu("open") => {
                    self.show_window(ctx, visible);
                }
                Event::Menu("topup") => self.request(Command::TopUp),
                Event::Menu("pause") => self.request(Command::Pause),
                Event::Menu("resume") => self.request(Command::Resume),
                Event::Menu(_) => {}
            }
        }
        if self.quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}
impl Resident for App {
    fn start_hidden(&mut self) -> bool {
        let Some(tray) = self.tray.as_mut() else {
            return false;
        };
        tray.create_item();
        if !tray.is_shown() {
            return false;
        }
        #[cfg(target_os = "macos")]
        fastframe_dock::set_dock_icon_visible(false);
        true
    }
    fn closed(&self) -> Closed {
        if self.hide && !self.quit {
            Closed::Hide
        } else {
            Closed::Quit
        }
    }
    fn window_gone(&mut self) {
        self.hide = false;
        self.wants_show = false;
    }
    fn headless_frame(&mut self, ctx: &egui::Context) -> Headless {
        self.events(ctx, false);
        if !self.preview && !self.busy && self.refreshed.elapsed() >= REFRESH {
            self.request(Command::Status);
        }
        if self.quit {
            Headless::Quit
        } else if self.wants_show {
            Headless::Show
        } else {
            Headless::WaitFor(REFRESH)
        }
    }
    fn shutdown(&mut self) {}
}
struct Window {
    app: Held<App>,
    focused: bool,
}
impl eframe::App for Window {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.app.events(ctx, true);
        if ctx.input(|i| i.viewport().close_requested()) {
            let tray_shown = self.app.tray.as_ref().is_some_and(Tray::is_shown);
            self.app.handle_close_requested(ctx, tray_shown);
        }
        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(false));
        if !self.app.preview && focused && !self.focused {
            self.app.request_autostart(None);
        }
        if !self.app.preview
            && !self.app.busy
            && (self.app.refreshed.elapsed() >= REFRESH || focused && !self.focused)
        {
            self.app.request(Command::Status);
        }
        self.focused = focused;
        if !self.app.preview {
            ctx.request_repaint_after(
                REFRESH
                    .saturating_sub(self.app.refreshed.elapsed())
                    .max(Duration::from_secs(1)),
            );
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| self.app.render(ui));
    }
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if !self.app.preview {
            storage.set_string("language", self.app.language.clone());
        }
    }
    fn persist_egui_memory(&self) -> bool {
        false
    }
}

fn icon_data() -> &'static egui::IconData {
    static ICON: OnceLock<egui::IconData> = OnceLock::new();
    ICON.get_or_init(|| {
        eframe::icon_data::from_png_bytes(include_bytes!("../icons/128x128.png"))
            .expect("bundled icon")
    })
}
static TRAY_PERCENT: AtomicU8 = AtomicU8::new(255);
fn tray_icon(size: usize) -> Vec<u8> {
    let mut rgba = vec![0; size * size * 4];
    let percent = TRAY_PERCENT.load(Ordering::Relaxed);
    // Draw at native pixel density while retaining a crisp battery silhouette.
    for row in 0..size {
        for col in 0..size {
            let x = col * 32 / size;
            let y = row * 32 / size;
            let body = (3..27).contains(&x) && (8..24).contains(&y);
            let edge = body && (!(5..25).contains(&x) || !(10..22).contains(&y));
            let tip = (27..30).contains(&x) && (12..20).contains(&y);
            let fill = percent <= 100
                && (7..23).contains(&x)
                && (12..20).contains(&y)
                && x - 7 < usize::from(percent) * 16 / 100;
            if edge || tip || fill {
                rgba[(row * size + col) * 4..(row * size + col + 1) * 4]
                    .copy_from_slice(&[216, 234, 255, 255]);
            }
        }
    }
    rgba
}

fn run() -> Result<(), String> {
    let Some(options) = Options::parse()? else {
        return Ok(());
    };
    let preview = options.preview.is_some();
    let start_hidden = options.start_hidden;
    let waker = Waker::default();
    let app = App::new(waker.clone(), options)?;
    let shell = Shell::new(app, &waker).start_hidden(start_hidden);
    #[cfg(target_os = "macos")]
    let shell = shell.idle(fastframe_tray::idle);
    shell
        .run(|lease| {
            let mut options = eframe::NativeOptions {
                #[cfg(target_os = "windows")]
                renderer: eframe::Renderer::Wgpu,
                #[cfg(not(target_os = "windows"))]
                renderer: eframe::Renderer::Glow,
                viewport: egui::ViewportBuilder::default()
                    .with_app_id(if preview {
                        "io.github.batterybridge.preview"
                    } else {
                        APP_ID
                    })
                    .with_inner_size([420.0, 730.0])
                    .with_min_inner_size([320.0, 480.0])
                    .with_icon(icon_data().clone()),
                persist_window: !preview,
                ..Default::default()
            };
            if preview {
                options.persistence_path =
                    Some(std::env::temp_dir().join("batterybridge-preview.ron"));
            }
            eframe::run_native(
                if preview {
                    "BatteryBridge (Preview)"
                } else {
                    "BatteryBridge"
                },
                options,
                Box::new(move |cc| {
                    let mut app = lease.take(&cc.egui_ctx);
                    app.attach(cc);
                    Ok(Box::new(Window {
                        app,
                        focused: false,
                    }))
                }),
            )
        })
        .map_err(|e| e.to_string())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("BatteryBridge: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preview() -> App {
        App::new(
            Waker::default(),
            Options {
                preview: Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/status.json")),
                language: Some("ru".into()),
                ..Default::default()
            },
        )
        .unwrap()
    }

    #[test]
    fn preview_never_starts_ipc_or_tray() {
        let mut app = preview();
        assert!(app.tray.is_none());
        for command in [
            Command::Status,
            Command::Probe,
            Command::Limit {
                stop: Some(80),
                start: None,
            },
        ] {
            app.request(command);
            assert!(!app.busy);
        }
        assert!(
            !app.start_hidden(),
            "without a tray, hidden startup must open a window"
        );
        app.request_autostart(Some(true));
        assert!(!app.startup_busy);
        assert_eq!(app.startup_state, Some(fastframe_autostart::State::Off));
    }

    #[test]
    fn startup_argument_is_optional_and_preview_keeps_its_safety_boundary() {
        fn parse(args: &[&str]) -> Result<Option<Options>, String> {
            Options::parse_args(args.iter().map(|s| (*s).into()))
        }
        assert!(!parse(&[]).unwrap().unwrap().start_hidden);
        assert!(parse(&["--start-hidden"]).unwrap().unwrap().start_hidden);
        let options = parse(&[
            "--start-hidden",
            "--preview",
            "snapshot.json",
            "--language",
            "ru",
        ])
        .unwrap()
        .unwrap();
        assert!(options.start_hidden);
        assert_eq!(options.language.as_deref(), Some("ru"));
        assert_eq!(options.preview, Some(PathBuf::from("snapshot.json")));
        assert!(parse(&["--unknown"]).is_err());
    }

    #[test]
    fn startup_worker_is_independent_of_charging_permissions_and_inflight_requests() {
        use fastframe_autostart::{Start, State};
        let mut app = preview();
        app.preview = false;
        app.model.snapshot.as_mut().unwrap().config = None;
        app.busy = true;
        app.model.stop = 85;
        app.model.edited = true;
        app.model.error = Some("battery request failed".into());
        app.startup_query = |enabled| {
            assert_eq!(enabled, Some(true));
            Ok(State::NeedsApproval(Start::Hidden))
        };
        app.request_autostart(Some(true));
        assert!(app.startup_busy);
        // A second click cannot race the first registration.
        app.request_autostart(Some(false));
        let reply = app.replies.recv_timeout(Duration::from_secs(5)).unwrap();
        app.sender.send(reply).unwrap();
        app.events(&egui::Context::default(), true);
        assert!(!app.startup_busy);
        assert!(
            app.busy,
            "an autostart reply must not complete a charging request"
        );
        assert_eq!(app.startup_state, Some(State::NeedsApproval(Start::Hidden)));
        assert!(app.startup_error.is_none());
        assert_eq!(app.model.stop, 85);
        assert!(app.model.edited);
        assert_eq!(app.model.error.as_deref(), Some("battery request failed"));

        app.startup_query = |_| Err("OS denied startup registration".into());
        app.request_autostart(Some(false));
        let reply = app.replies.recv_timeout(Duration::from_secs(5)).unwrap();
        app.sender.send(reply).unwrap();
        app.events(&egui::Context::default(), true);
        assert!(!app.startup_busy);
        assert!(app.busy);
        assert_eq!(app.startup_state, Some(State::NeedsApproval(Start::Hidden)));
        assert_eq!(
            app.startup_error.as_deref(),
            Some("OS denied startup registration")
        );
    }

    #[test]
    fn offline_mode_rejects_writes_before_starting_a_worker() {
        let mut app = preview();
        app.preview = false;
        app.model.snapshot.as_mut().unwrap().config = None;
        app.request(Command::Limit {
            stop: Some(80),
            start: None,
        });
        assert!(!app.busy);
    }

    #[test]
    fn command_acknowledgements_only_discard_the_corresponding_draft() {
        for command in [
            Command::Care {
                settings: Default::default(),
                expected: Default::default(),
            },
            Command::Limit {
                stop: Some(85),
                start: None,
            },
            Command::Reset,
            Command::TopUp,
        ] {
            let mut app = preview();
            app.model.stop = 85;
            app.model.edited = true;
            app.model.care.heat_limit_c = Some(35);
            app.model.care_edited = true;
            app.pending_command = Some(command.clone());
            app.sender.send(Reply::Protocol(Ok(Response::Ok))).unwrap();
            app.events(&egui::Context::default(), false);
            let snapshot = app.model.snapshot.clone().unwrap();
            app.sender
                .send(Reply::Protocol(Ok(Response::Status(Box::new(snapshot)))))
                .unwrap();
            app.events(&egui::Context::default(), false);
            let resets_limit = matches!(command, Command::Limit { .. } | Command::Reset);
            let resets_care = matches!(command, Command::Care { .. } | Command::Reset);
            assert_eq!(app.model.edited, !resets_limit);
            assert_eq!(app.model.care_edited, !resets_care);
            if !resets_limit {
                assert_eq!(app.model.stop, 85);
            }
            if !resets_care {
                assert_eq!(app.model.care.heat_limit_c, Some(35));
            }
        }
    }
    #[test]
    fn closing_and_reopening_from_tray_preserves_drafts_and_native_lifecycle() {
        use egui::ViewportCommand::{CancelClose, Close, Focus, Minimized, Visible};
        let mut app = preview();
        app.model.stop = 85;
        app.model.edited = true;
        let ctx = egui::Context::default();
        for _ in 0..3 {
            let mut hidden = ctx.run_ui(Default::default(), |ui| {
                app.handle_close_requested(ui.ctx(), true);
            });
            hidden.textures_delta.clear();
            let commands = &hidden.viewport_output[&egui::ViewportId::ROOT].commands;
            assert!(app.hide);
            assert!(!commands.contains(&Close));
            if cfg!(target_os = "macos") {
                assert!(commands.contains(&CancelClose));
                assert!(commands.contains(&Visible(false)));
            } else {
                assert!(!commands.contains(&CancelClose));
                assert!(!commands.contains(&Visible(false)));
                assert!(matches!(app.closed(), Closed::Hide));
                app.window_gone();
            }
            let mut shown = ctx.run_ui(Default::default(), |ui| app.show_window(ui.ctx(), true));
            shown.textures_delta.clear();
            let commands = &shown.viewport_output[&egui::ViewportId::ROOT].commands;
            let expected = if cfg!(target_os = "macos") {
                vec![Visible(true), Minimized(false), Focus]
            } else {
                vec![Minimized(false), Focus]
            };
            assert_eq!(commands, &expected);
            assert!(!app.hide);
            assert!(!app.wants_show);
            assert_eq!(app.model.stop, 85);
            assert!(app.model.edited);
        }
    }
    #[test]
    fn close_without_tray_and_explicit_quit_are_not_cancelled() {
        for (tray_shown, quit) in [(false, false), (true, true)] {
            let mut app = preview();
            app.quit = quit;
            let mut output = egui::Context::default().run_ui(Default::default(), |ui| {
                app.handle_close_requested(ui.ctx(), tray_shown);
            });
            output.textures_delta.clear();
            let commands = &output.viewport_output[&egui::ViewportId::ROOT].commands;
            assert!(!commands.contains(&egui::ViewportCommand::CancelClose));
            assert!(!commands.contains(&egui::ViewportCommand::Visible(false)));
            assert!(!app.hide);
            assert!(matches!(app.closed(), Closed::Quit));
        }
    }
    #[test]
    fn show_without_an_existing_window_uses_shell_recreation_once() {
        let mut app = preview();
        app.show_window(&egui::Context::default(), false);
        assert!(matches!(
            app.headless_frame(&egui::Context::default()),
            Headless::Show
        ));
        app.window_gone();
        assert!(matches!(
            app.headless_frame(&egui::Context::default()),
            Headless::WaitFor(_)
        ));
    }
    #[test]
    fn closing_without_a_tray_quits_and_quit_overrides_hide() {
        let mut app = preview();
        assert!(matches!(app.closed(), Closed::Quit));
        app.hide = true;
        assert!(matches!(app.closed(), Closed::Hide));
        app.quit = true;
        assert!(matches!(app.closed(), Closed::Quit));
    }
}
