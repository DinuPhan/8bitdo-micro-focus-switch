use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};
use tokio::sync::{mpsc, watch};

use crate::cli;
use crate::core::ble;
use crate::core::config;
use crate::core::daemon;
use crate::core::protocol;
use crate::core::window_watcher;
pub use crate::core::window_watcher::OpenWindowInfo;

#[derive(Serialize, Deserialize)]
pub struct DaemonStatus {
    pub is_running: bool,
    pub pid: Option<u32>,
}

// ---------------------------------------------------------------------------
// In-process daemon management
// ---------------------------------------------------------------------------

struct DaemonHandle {
    stop_tx: watch::Sender<bool>,
    reload_tx: mpsc::UnboundedSender<()>,
    thread: std::thread::JoinHandle<()>,
}

#[derive(Default)]
pub struct DaemonState {
    handle: Mutex<Option<DaemonHandle>>,
}

impl DaemonState {
    /// True if the in-process daemon thread is alive.
    pub fn in_process_running(&self) -> bool {
        let mut guard = self.handle.lock().unwrap();
        if let Some(h) = guard.as_ref() {
            if h.thread.is_finished() {
                *guard = None;
                return false;
            }
            return true;
        }
        false
    }

    /// Start the focus-switch daemon on a dedicated thread with its own tokio runtime.
    pub fn start(&self) -> Result<(), String> {
        if self.in_process_running() {
            return Ok(());
        }
        let cfg_path = config::ensure_default_config(None)?;
        let cfg = config::load_config(Some(&cfg_path))?;

        let (stop_tx, stop_rx) = watch::channel(false);
        let (reload_tx, reload_rx) = mpsc::unbounded_channel();

        let thread = std::thread::Builder::new()
            .name("focus-switch-daemon".to_string())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        eprintln!("[8bitdo-micro] Failed to create daemon runtime: {}", e);
                        return;
                    }
                };
                rt.block_on(async move {
                    let d = daemon::FocusSwitchDaemon::new(cfg, Some(cfg_path), None, None);
                    if let Err(e) = d.run(stop_rx, reload_rx).await {
                        eprintln!("[8bitdo-micro] Daemon stopped with error: {}", e);
                    }
                });
            })
            .map_err(|e| format!("Failed to spawn daemon thread: {}", e))?;

        *self.handle.lock().unwrap() = Some(DaemonHandle {
            stop_tx,
            reload_tx,
            thread,
        });
        Ok(())
    }

    pub fn stop(&self) {
        let handle = self.handle.lock().unwrap().take();
        if let Some(h) = handle {
            let _ = h.stop_tx.send(true);
            // Give the daemon a moment to release grabbed devices & held keys.
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            while !h.thread.is_finished() && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            if h.thread.is_finished() {
                let _ = h.thread.join();
            }
        }
    }

    pub fn reload(&self) -> bool {
        if !self.in_process_running() {
            return false;
        }
        let guard = self.handle.lock().unwrap();
        guard
            .as_ref()
            .map(|h| h.reload_tx.send(()).is_ok())
            .unwrap_or(false)
    }
}

/// True if a daemon is running outside of this GUI process (systemd service or standalone CLI).
pub fn external_daemon_running() -> (bool, Option<u32>) {
    if cli::is_systemd_service_active() {
        let pid = cli::find_daemon_pids().into_iter().next();
        return (true, pid);
    }
    let pids = cli::find_daemon_pids();
    (!pids.is_empty(), pids.first().copied())
}

/// Reload both the in-process daemon (if any) and any external daemon. Returns a status message.
pub fn reload_all(state: &DaemonState) -> (bool, String) {
    let mut msgs = Vec::new();
    let mut any = false;
    if state.reload() {
        any = true;
        msgs.push("in-process daemon".to_string());
    }
    let (ok, msg) = cli::reload_running_daemon();
    if ok {
        any = true;
        msgs.push(msg);
    }
    if any {
        (true, format!("Reloaded: {}", msgs.join("; ")))
    } else {
        (false, "Daemon is not currently running.".to_string())
    }
}

// ---------------------------------------------------------------------------
// Tauri IPC commands
// ---------------------------------------------------------------------------

fn resolve_config_path() -> PathBuf {
    config::default_config_path()
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PlatformInfo {
    pub os: String,
    pub supports_focus_daemon: bool,
}

#[tauri::command]
pub fn get_platform_info() -> PlatformInfo {
    PlatformInfo {
        os: std::env::consts::OS.to_string(),
        supports_focus_daemon: cfg!(target_os = "linux"),
    }
}

#[tauri::command]
pub fn get_config_path() -> Result<String, String> {
    Ok(resolve_config_path().to_string_lossy().to_string())
}

#[tauri::command]
pub fn read_config() -> Result<String, String> {
    let path = resolve_config_path();
    if path.exists() {
        fs::read_to_string(&path).map_err(|e| format!("Failed to read {}: {}", path.display(), e))
    } else {
        Ok(config::DEFAULT_TOML_CONTENT.to_string())
    }
}

#[tauri::command]
pub fn write_config(content: String, state: tauri::State<'_, DaemonState>) -> Result<(), String> {
    config::load_config_from_str(&content).map_err(|e| format!("Invalid configuration: {}", e))?;

    let path = resolve_config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory {}: {}", parent.display(), e))?;
    }
    fs::write(&path, content).map_err(|e| format!("Failed to write {}: {}", path.display(), e))?;

    let _ = reload_all(&state);
    Ok(())
}

#[tauri::command]
pub fn get_daemon_status(state: tauri::State<'_, DaemonState>) -> Result<DaemonStatus, String> {
    if state.in_process_running() {
        return Ok(DaemonStatus {
            is_running: true,
            pid: Some(std::process::id()),
        });
    }
    let (running, pid) = external_daemon_running();
    Ok(DaemonStatus {
        is_running: running,
        pid,
    })
}

#[tauri::command]
pub fn reload_daemon(state: tauri::State<'_, DaemonState>) -> Result<String, String> {
    let path = resolve_config_path();
    config::load_config(Some(&path))
        .map_err(|e| format!("Invalid configuration in {}: {}", path.display(), e))?;
    let (ok, msg) = reload_all(&state);
    if ok {
        Ok(msg)
    } else {
        Err(msg)
    }
}

#[tauri::command]
pub fn toggle_daemon(start: bool, state: tauri::State<'_, DaemonState>) -> Result<bool, String> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (start, state);
        return Err("The focus-switching daemon is exclusive to Linux (Hyprland/Wayland). On Windows/macOS, use the BLE Hardware Flasher to write bindings directly to your controller!".to_string());
    }
    #[cfg(target_os = "linux")]
    {
        if start {
            let (external, _) = external_daemon_running();
            if external {
                return Ok(true);
            }
            state.start()?;
            Ok(true)
        } else {
            state.stop();
            if cli::is_systemd_service_active() {
                let _ = std::process::Command::new("systemctl")
                    .args(["--user", "stop", "8bitdo-micro-focus-switch.service"])
                    .output();
            }
            for pid in cli::find_daemon_pids() {
                unsafe {
                    libc::kill(pid as i32, libc::SIGTERM);
                }
            }
            Ok(false)
        }
    }
}

#[tauri::command]
pub fn get_open_windows() -> Result<Vec<OpenWindowInfo>, String> {
    Ok(window_watcher::query_platform_open_windows())
}

async fn run_ble_task<F, Fut>(f: F) -> Result<String, String>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<String, String>>,
{
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let res = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("Failed to create BLE runtime: {}", e))
            .and_then(|rt| rt.block_on(f()));
        let _ = tx.send(res);
    });
    rx.await
        .map_err(|_| "BLE worker thread terminated unexpectedly".to_string())?
}

#[tauri::command]
pub async fn flash_preset(preset: String, turn: Option<u32>) -> Result<String, String> {
    run_ble_task(move || async move {
        let cfg_path = config::ensure_default_config(None)?;
        let cfg = config::load_config(Some(&cfg_path))?;
        let prof = cfg.get_profile(&preset)?;
        let is_factory = matches!(
            preset.trim().to_lowercase().as_str(),
            "factory" | "default" | "reset"
        );
        let mut hw = prof.to_hardware_profile(!is_factory)?;
        if let Some(t) = turn {
            hw.turn = protocol::turn_degrees_to_code(t as i32)?;
        }
        let snapshot =
            ble::write_hardware_profile(&hw, None, Duration::from_secs(6), true).await?;
        Ok(format!(
            "Flashed profile {:?} (turn={}°)\n\n{}",
            prof.name,
            hw.turn_degrees(),
            ble::format_snapshot_report(&snapshot)
        ))
    })
    .await
    .map_err(|e| format!("Flash error: {}", e))
}

// ---------------------------------------------------------------------------
// GUI entry point
// ---------------------------------------------------------------------------

pub fn run() {
    run_gui();
}

pub fn run_gui() {
    tauri::Builder::default()
        .manage(DaemonState::default())
        .setup(|app| {
            #[cfg(target_os = "linux")]
            {
                let (external, _) = external_daemon_running();
                if !external {
                    if let Err(e) = app.state::<DaemonState>().start() {
                        eprintln!("[8bitdo-micro] Could not start in-process daemon: {}", e);
                    }
                }
            }

            // Build Context Menu for System Tray
            let toggle = MenuItemBuilder::with_id("toggle", "Open Configurator").build(app)?;
            let reload = MenuItemBuilder::with_id("reload", "Reload Configuration").build(app)?;
            let separator = PredefinedMenuItem::separator(app)?;
            let quit = MenuItemBuilder::with_id("quit", "Quit 8BitDo Micro Manager").build(app)?;

            let menu = MenuBuilder::new(app)
                .items(&[&toggle, &reload, &separator, &quit])
                .build()?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .tooltip(if cfg!(target_os = "linux") {
                    "8BitDo Micro Focus Switch"
                } else {
                    "8BitDo Micro Hardware Manager"
                })
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "toggle" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "reload" => {
                        let _ = reload_all(&app.state::<DaemonState>());
                    }
                    "quit" => {
                        app.state::<DaemonState>().stop();
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            if window.is_visible().unwrap_or(false) {
                                let _ = window.hide();
                            } else {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    }
                })
                .build(app)?;

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_platform_info,
            get_config_path,
            read_config,
            write_config,
            get_daemon_status,
            reload_daemon,
            toggle_daemon,
            get_open_windows,
            flash_preset,
        ])
        .run(tauri::generate_context!())
        .expect("error while running 8bitdo-micro-focus-switch application");
}
