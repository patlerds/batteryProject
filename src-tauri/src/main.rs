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
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_notification::NotificationExt;

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
}

fn lock<'a>(
    state: &'a State<'_, Mutex<AppState>>,
) -> Result<std::sync::MutexGuard<'a, AppState>, String> {
    state
        .lock()
        .map_err(|_| "App state unavailable; restart Chargekeeper".into())
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
        capabilities: if s.simulated {
            Capabilities {
                set_limit: true,
                thresholds: true,
                inhibit: true,
                force_discharge: true,
                temperature: true,
            }
        } else {
            Capabilities::default()
        },
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
    let mut reminder = None;
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
                    reminder = Some(cfg.limit);
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
    drop(s);
    if let Some(limit) = reminder {
        let _ = app.notification().builder().title("Time to unplug")
            .body(format!("Your battery has reached {limit}%. Chargekeeper cannot stop charging on this device.")).show();
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
    config.validate()?;
    let mut s = lock(&state)?;
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
    s.sim = Simulator::default();
    s.engine = Engine::default();
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
    s.config = Config {
        limit: 100,
        ..Config::default()
    };
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
    let manager = app.autolaunch();
    if enabled {
        manager.enable()
    } else {
        manager.disable()
    }
    .map_err(|_| "Windows could not update launch at login".into())
}
#[tauri::command]
fn get_launch_at_login(app: tauri::AppHandle) -> Result<bool, String> {
    app.autolaunch()
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
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::Builder::new().build())
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
