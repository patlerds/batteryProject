#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod discovery;
mod logging;
mod telemetry;

use chargekeeper_core::{
    policy::{Config, Engine, Mode},
    simulator::Simulator,
    storage, Capabilities, Snapshot,
};
use serde::Serialize;
use std::{path::PathBuf, sync::Mutex, time::Instant};
use tauri::{
    image::Image,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Manager, State,
};

/// Host-side delivery tracking for the unplug reminder. The policy engine decides
/// when a reminder is due (and marks it as given); this only makes sure a toast
/// that Windows refused is retried on later polls, a bounded number of times.
#[derive(Default)]
struct ReminderDelivery {
    pending: Option<u8>,
    failures: u8,
    last_failed: bool,
}

/// Total delivery attempts per reminder (one per poll cycle, so ~30 s at most).
const MAX_REMINDER_ATTEMPTS: u8 = 3;

impl ReminderDelivery {
    /// `fresh` is a reminder the engine just raised for this limit. `relevant` is whether
    /// a pending reminder still applies: `Some(false)` drops it (unplugged or below the
    /// limit), `None` (telemetry unknown) keeps it without attempting this cycle.
    /// Returns the limit to notify about now, if any.
    fn due(&mut self, fresh: Option<u8>, relevant: Option<bool>) -> Option<u8> {
        if let Some(limit) = fresh {
            self.pending = Some(limit);
            self.failures = 0;
            return self.pending;
        }
        match relevant {
            Some(true) => self.pending,
            Some(false) => {
                self.pending = None;
                self.failures = 0;
                None
            }
            None => None,
        }
    }

    /// Records the outcome of an attempt; returns a static event name to log.
    fn record(&mut self, delivered: bool) -> Option<&'static str> {
        self.pending?;
        if delivered {
            self.pending = None;
            self.failures = 0;
            self.last_failed = false;
            return None;
        }
        self.failures = self.failures.saturating_add(1);
        self.last_failed = true;
        if self.failures >= MAX_REMINDER_ATTEMPTS {
            self.pending = None;
            self.failures = 0;
            Some("reminder_failed_gave_up")
        } else {
            Some("reminder_failed")
        }
    }
}

/// Shows the unplug toast synchronously so a Windows error is observable (the
/// notification plugin dispatches on a background task and drops every error).
fn show_reminder(identifier: &str, limit: u8) -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let dir = exe
        .parent()
        .map(|d| d.display().to_string())
        .unwrap_or_default();
    let mut toast = notify_rust::Notification::new();
    toast.summary("Time to unplug").body(&format!(
        "Your battery has reached {limit}%. Chargekeeper cannot stop charging on this device."
    ));
    toast.auto_icon();
    // Same rule as tauri-plugin-notification: use the app's AUMID only outside
    // target\debug and target\release (there Windows attributes it to PowerShell).
    if !(dir.ends_with(r"\target\debug") || dir.ends_with(r"\target\release")) {
        toast.app_id(identifier);
    }
    toast.show().is_ok()
}

/// Launch-at-login entry, forced to HKCU (never the machine-wide HKLM Run key that
/// auto-launch's default `Dynamic` mode tries first) with a quoted executable path.
fn autostart_entry(app_name: &str, exe: &str) -> Result<auto_launch::AutoLaunch, String> {
    let no_args: [&str; 0] = [];
    auto_launch::AutoLaunchBuilder::new()
        .set_app_name(app_name)
        // auto-launch writes `format!("{path} {args}")` verbatim, so quote it here.
        .set_app_path(&format!("\"{exe}\""))
        .set_windows_enable_mode(auto_launch::WindowsEnableMode::CurrentUser)
        .set_args(&no_args)
        .build()
        .map_err(|_| "Launch at login is unavailable".into())
}

fn autostart(app: &tauri::AppHandle) -> Result<auto_launch::AutoLaunch, String> {
    let exe = std::env::current_exe().map_err(|_| "Cannot locate Chargekeeper")?;
    autostart_entry(&app.package_info().name, &exe.display().to_string())
}

/// Named per-session mutex so autostart plus a manual launch cannot run two workers.
/// Returns false when another instance already holds it.
#[cfg(windows)]
fn single_instance() -> bool {
    use windows_sys::Win32::{
        Foundation::{GetLastError, ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS},
        System::Threading::CreateMutexW,
    };
    let name: Vec<u16> = "Local\\org.chargekeeper.desktop.single-instance\0"
        .encode_utf16()
        .collect();
    // SAFETY: fixed nul-terminated name, default security, no initial ownership.
    // The handle is deliberately never closed: it must live as long as the process.
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    // SAFETY: reads the calling thread's last-error value right after the call.
    let error = unsafe { GetLastError() };
    if handle.is_null() {
        // Denied means an instance with a different token (e.g. elevated) owns it.
        // Any other failure fails open rather than preventing the monitor from starting.
        return error != ERROR_ACCESS_DENIED;
    }
    error != ERROR_ALREADY_EXISTS
}

struct AppState {
    config: Config,
    config_error: Option<String>,
    config_path: PathBuf,
    simulated: bool,
    sim: Simulator,
    engine: Engine,
    started: Instant,
    discovery: discovery::Discovery,
    diagnostics: PathBuf,
    snapshot: Snapshot,
    reason: String,
    phase: String,
    sampled_at: Instant,
    reminders: ReminderDelivery,
}
#[derive(Serialize)]
struct Status {
    config: Config,
    snapshot: Snapshot,
    capabilities: Capabilities,
    simulated: bool,
    mechanism: String,
    reason: String,
    phase: String,
    discovery: discovery::Discovery,
    config_error: Option<String>,
    /// The most recent reminder toast could not be shown by Windows.
    reminder_failed: bool,
}

fn lock<'a>(
    state: &'a State<'_, Mutex<AppState>>,
) -> Result<std::sync::MutexGuard<'a, AppState>, String> {
    state
        .lock()
        .map_err(|_| "App state unavailable; restart Chargekeeper".into())
}

/// Capabilities currently in effect: the simulator's in simulation, otherwise the real defaults.
fn effective_caps(simulated: bool) -> Capabilities {
    if simulated {
        Capabilities {
            set_limit: true,
            thresholds: true,
            inhibit: true,
            force_discharge: true,
            temperature: true,
        }
    } else {
        Capabilities::default()
    }
}

#[tauri::command]
fn get_status(state: State<'_, Mutex<AppState>>) -> Result<Status, String> {
    let s = lock(&state)?;
    let mut snapshot = s.snapshot.clone();
    if s.sampled_at.elapsed().as_secs() > 30 {
        snapshot.fresh = false;
    }
    Ok(Status {
        config: s.config.clone(),
        snapshot,
        capabilities: effective_caps(s.simulated),
        simulated: s.simulated,
        mechanism: if s.simulated {
            "Simulated firmware — no hardware changes"
        } else {
            "Software reminders — no hardware enforcement"
        }
        .into(),
        reason: s.reason.clone(),
        phase: s.phase.clone(),
        discovery: s.discovery.clone(),
        config_error: s.config_error.clone(),
        reminder_failed: s.reminders.last_failed,
    })
}

fn unavailable() -> Snapshot {
    Snapshot {
        batteries: vec![],
        ac: None,
        fresh: false,
    }
}

fn poll(app: &tauri::AppHandle) {
    let state = app.state::<Mutex<AppState>>();
    let simulated = match state.lock() {
        Ok(s) => s.simulated,
        Err(_) => return,
    };
    let measured = if simulated {
        None
    } else {
        Some(telemetry::read())
    };
    let mut s = match state.lock() {
        Ok(s) => s,
        Err(_) => return,
    };
    // A session switch while IO was in flight invalidates that result.
    if simulated != s.simulated {
        return;
    }
    let mut fresh_reminder = None;
    let mut relevant = None;
    if simulated {
        let mut cfg = s.config.clone();
        match s.sim.step(&mut cfg) {
            Ok(sample) => {
                s.config = cfg;
                s.snapshot = sample.snapshot;
                s.reason = sample.reason;
                s.phase = format!("{:?}", sample.phase);
            }
            Err(error) => {
                s.snapshot = unavailable();
                s.reason = error;
            }
        }
    } else {
        match measured.unwrap_or_else(|| Err("Missing telemetry".into())) {
            Ok(snapshot) => {
                let cfg = s.config.clone();
                let now = s.started.elapsed().as_secs();
                let d = s.engine.step(&cfg, &snapshot, Capabilities::default(), now);
                if d.reminder {
                    fresh_reminder = Some(cfg.limit);
                }
                if let Some(limit) = s.reminders.pending {
                    relevant = Some(
                        snapshot.ac == Some(true)
                            && snapshot.highest().is_some_and(|p| p >= f64::from(limit)),
                    );
                }
                s.reason = d.reason.into();
                s.phase = format!("{:?}", d.phase);
                s.snapshot = snapshot;
            }
            Err(error) => {
                s.snapshot = unavailable();
                s.reason = error;
            }
        }
    }
    let percent = s.snapshot.highest();
    s.sampled_at = Instant::now();
    let reminder = s.reminders.due(fresh_reminder, relevant);
    drop(s);
    if let Some(limit) = reminder {
        // Shown outside the lock; the outcome is recorded so a refused toast is retried.
        let delivered = show_reminder(&app.config().identifier, limit);
        if let Ok(mut s) = state.lock() {
            if let Some(event) = s.reminders.record(delivered) {
                logging::event(&s.diagnostics, event);
            }
        }
    }
    if let Some(tray) = app.tray_by_id("battery") {
        let _ = tray.set_icon(Some(battery_icon(percent)));
        let _ = tray.set_tooltip(Some(format!(
            "Chargekeeper · {} · {}",
            percent
                .map(|p| format!("{p:.0}%"))
                .unwrap_or("No battery".into()),
            if simulated {
                "SIMULATION"
            } else {
                "Reminders only"
            }
        )));
    }
}

#[tauri::command]
fn save_config(config: Config, state: State<'_, Mutex<AppState>>) -> Result<(), String> {
    let mut s = lock(&state)?;
    config.validate_for(effective_caps(s.simulated))?;
    if s.config_error.is_some() {
        return Err(
            "Repair the configuration file before saving; existing data was preserved".into(),
        );
    }
    if !s.simulated && (config.mode != Mode::Limit || config.heat_c.is_some() || config.discharge) {
        return Err("This device supports reminders only; hardware modes are unavailable".into());
    }
    if !s.simulated {
        storage::save(&s.config_path, &config)?;
    }
    s.config = config;
    logging::event(&s.diagnostics, "preferences_saved");
    Ok(())
}
#[tauri::command]
fn set_simulation(enabled: bool, state: State<'_, Mutex<AppState>>) -> Result<(), String> {
    let mut s = lock(&state)?;
    // Simulation never saves mode/threshold changes to real preferences.
    let real = storage::load(&s.config_path);
    s.config = real.clone().unwrap_or_default();
    s.config_error = real.err();
    s.config.mode = Mode::Limit;
    s.config.heat_c = None;
    s.config.discharge = false;
    if enabled && s.config.validate_for(effective_caps(true)).is_err() {
        // A stored band the simulated firmware cannot express; never persisted.
        s.config = Config::default();
    }
    s.sim = Simulator::default();
    s.engine = Engine::default();
    s.reminders = ReminderDelivery::default();
    s.simulated = enabled;
    s.snapshot = unavailable();
    s.reason = "Waiting for the next background sample".into();
    s.phase = "Limit".into();
    logging::event(&s.diagnostics, "session_switched");
    Ok(())
}
#[tauri::command]
fn set_simulated_battery(
    percent: f64,
    temperature: f64,
    ac: bool,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    if !percent.is_finite()
        || !(0.0..=100.0).contains(&percent)
        || !temperature.is_finite()
        || !(0.0..=60.0).contains(&temperature)
    {
        return Err("Invalid simulated telemetry".into());
    }
    let mut s = lock(&state)?;
    if !s.simulated {
        return Err("Enable simulation first".into());
    }
    s.sim.percent = percent;
    s.sim.temperature = temperature;
    s.sim.ac = ac;
    Ok(())
}
#[tauri::command]
fn restore_defaults(state: State<'_, Mutex<AppState>>) -> Result<(), String> {
    let mut s = lock(&state)?;
    if !s.simulated {
        return Err(
            "No validated BIOS writer is installed. No firmware settings were changed by this app."
                .into(),
        );
    }
    let defaults = Config {
        limit: 100,
        ..Config::default()
    };
    defaults.validate_for(effective_caps(true))?;
    s.config = defaults;
    s.sim.reset_policy();
    Ok(())
}
#[tauri::command]
fn export_diagnostics(state: State<'_, Mutex<AppState>>) -> Result<String, String> {
    let s = lock(&state)?;
    // Explicit allowlist: no raw WMI output, serial, username, password or paths.
    let data = serde_json::json!({ "schema": 1, "app_version": env!("CARGO_PKG_VERSION"),
        "os": "windows", "simulated": s.simulated, "hardware_writes_enabled": false,
        "dell_wmi_present": s.discovery.wmi, "cctk_present": s.discovery.cctk,
        "limit": s.config.limit, "band": s.config.band,
        "config_error": s.config_error.is_some() });
    std::fs::create_dir_all(&s.diagnostics).map_err(|_| "Cannot create diagnostics folder")?;
    let path = s.diagnostics.join("chargekeeper-diagnostics.json");
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&data).map_err(|_| "Cannot encode diagnostics")?,
    )
    .map_err(|_| "Cannot export diagnostics")?;
    logging::event(&s.diagnostics, "diagnostics_exported");
    Ok(path.display().to_string())
}
#[tauri::command]
fn set_launch_at_login(enabled: bool, app: tauri::AppHandle) -> Result<(), String> {
    let entry = autostart(&app)?;
    if enabled {
        entry.enable()
    } else {
        match entry.disable() {
            // Already absent counts as disabled.
            Err(auto_launch::Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            other => other,
        }
    }
    .map_err(|_| "Windows could not update launch at login".into())
}
#[tauri::command]
fn get_launch_at_login(app: tauri::AppHandle) -> Result<bool, String> {
    autostart(&app)?
        .is_enabled()
        .map_err(|_| "Cannot read launch at login".into())
}

#[tauri::command]
fn open_dell_support() -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};
        let verb: Vec<u16> = "open\0".encode_utf16().collect();
        let url: Vec<u16> = "https://www.dell.com/support/home/en-us/product-support/product/command-configure/overview\0".encode_utf16().collect();
        // SAFETY: fixed trusted URL, nul-terminated strings, no user arguments.
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                url.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        if result as isize <= 32 {
            return Err("Windows could not open Dell support".into());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err("Windows only".into())
    }
}

fn battery_icon(percent: Option<f64>) -> Image<'static> {
    let mut rgba = vec![0_u8; 32 * 32 * 4];
    let fill = (percent.unwrap_or(0.).clamp(0., 100.) / 100. * 20.) as usize;
    for y in 8..24 {
        for x in 2..30 {
            let border = (2..27).contains(&x) && (y == 8 || y == 23 || x == 2 || x == 26);
            let terminal = x >= 27 && (13..19).contains(&y);
            let inside = (11..21).contains(&y) && (5..5 + fill).contains(&x);
            if border || terminal || inside {
                let i = (y * 32 + x) * 4;
                rgba[i..i + 4].copy_from_slice(&[74, 169, 136, 255]);
            }
        }
    }
    Image::new_owned(rgba, 32, 32)
}

fn main() {
    #[cfg(windows)]
    if !single_instance() {
        // Another Chargekeeper is already running in this session; exit quietly.
        return;
    }
    tauri::Builder::default()
        .setup(|app| {
            let config_path = app.path().app_config_dir()?.join("config.json");
            let loaded = storage::load(&config_path);
            let mut config = loaded.clone().unwrap_or_default();
            // No software-driven mode can survive into an unverified hardware session.
            config.mode = Mode::Limit;
            config.heat_c = None;
            config.discharge = false;
            let diagnostics = app.path().app_local_data_dir()?.join("diagnostics");
            app.manage(Mutex::new(AppState {
                config,
                config_error: loaded.err(),
                config_path,
                simulated: false,
                sim: Simulator::default(),
                engine: Engine::default(),
                started: Instant::now(),
                discovery: discovery::Discovery {
                    note: "Discovering hardware…".into(),
                    ..Default::default()
                },
                snapshot: unavailable(),
                reason: "Starting battery monitor".into(),
                phase: "Limit".into(),
                sampled_at: Instant::now(),
                diagnostics,
                reminders: ReminderDelivery::default(),
            }));
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let discovery = discovery::probe();
                if let Ok(mut state) = handle.state::<Mutex<AppState>>().lock() {
                    state.discovery = discovery;
                }
            });
            let show = MenuItem::with_id(app, "show", "Open Chargekeeper", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            TrayIconBuilder::with_id("battery")
                .icon(battery_icon(None))
                .menu(&menu)
                .tooltip("Chargekeeper · Reminders only")
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;
            let monitor = app.handle().clone();
            std::thread::spawn(move || loop {
                poll(&monitor);
                std::thread::sleep(std::time::Duration::from_secs(10));
            });
            let state = app.state::<Mutex<AppState>>();
            if let Ok(s) = state.lock() {
                logging::event(&s.diagnostics, "app_started");
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            save_config,
            set_simulation,
            set_simulated_battery,
            restore_defaults,
            export_diagnostics,
            set_launch_at_login,
            get_launch_at_login,
            open_dell_support
        ])
        .run(tauri::generate_context!())
        .expect("Chargekeeper could not start");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delivered_reminder_is_not_retried() {
        let mut r = ReminderDelivery::default();
        assert_eq!(r.due(Some(80), None), Some(80));
        assert_eq!(r.record(true), None);
        assert!(!r.last_failed);
        assert_eq!(r.due(None, Some(true)), None);
    }

    #[test]
    fn failed_reminder_retries_once_per_poll_then_gives_up() {
        let mut r = ReminderDelivery::default();
        assert_eq!(r.due(Some(80), None), Some(80));
        assert_eq!(r.record(false), Some("reminder_failed"));
        assert!(r.last_failed);
        // Unknown telemetry keeps the reminder but does not attempt it.
        assert_eq!(r.due(None, None), None);
        assert_eq!(r.due(None, Some(true)), Some(80));
        assert_eq!(r.record(false), Some("reminder_failed"));
        assert_eq!(r.due(None, Some(true)), Some(80));
        assert_eq!(r.record(false), Some("reminder_failed_gave_up"));
        // Bounded: nothing further until the engine raises a new reminder.
        assert_eq!(r.due(None, Some(true)), None);
        assert_eq!(r.record(false), None);
        assert!(r.last_failed);
    }

    #[test]
    fn failed_reminder_is_dropped_once_irrelevant_and_success_clears_flag() {
        let mut r = ReminderDelivery::default();
        r.due(Some(80), None);
        r.record(false);
        assert_eq!(r.due(None, Some(false)), None);
        assert_eq!(r.due(None, Some(true)), None);
        assert_eq!(r.due(Some(80), None), Some(80));
        assert_eq!(r.record(true), None);
        assert!(!r.last_failed);
    }

    #[test]
    fn autostart_path_is_quoted_and_args_empty() {
        let entry = autostart_entry(
            "Chargekeeper",
            r"C:\Program Files\Chargekeeper\chargekeeper.exe",
        )
        .expect("entry builds");
        assert_eq!(
            entry.get_app_path(),
            r#""C:\Program Files\Chargekeeper\chargekeeper.exe""#
        );
        assert_eq!(entry.get_app_name(), "Chargekeeper");
        assert!(entry.get_args().is_empty());
    }
}
