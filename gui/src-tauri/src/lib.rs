use std::fs;
use std::path::PathBuf;
use std::process::Command;
use serde::{Deserialize, Serialize};
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};

#[derive(Serialize, Deserialize, Clone)]
pub struct OpenWindowInfo {
    pub class: String,
    pub title: String,
    pub initial_class: Option<String>,
    pub initial_title: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct DaemonStatus {
    pub is_running: bool,
    pub pid: Option<u32>,
}

fn get_home_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(profile) = std::env::var("USERPROFILE") {
            return PathBuf::from(profile);
        }
    }
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

fn resolve_config_path() -> PathBuf {
    let home = get_home_dir();
    let preferred = home.join(".config").join("8bitdo-micro").join("profiles.toml");
    if preferred.exists() {
        return preferred;
    }
    let fallback = home.join(".config").join("8bitdo-micro-focus-switch").join("profiles.toml");
    if fallback.exists() {
        return fallback;
    }
    preferred
}

#[tauri::command]
fn get_config_path() -> Result<String, String> {
    Ok(resolve_config_path().to_string_lossy().to_string())
}

#[tauri::command]
fn read_config() -> Result<String, String> {
    let path = resolve_config_path();
    if path.exists() {
        fs::read_to_string(&path).map_err(|e| format!("Failed to read {}: {}", path.display(), e))
    } else {
        // Return default starter TOML
        Ok(r#"# 8BitDo Micro Focus-Switch Configuration
[general]
fallback_profile = "default"
backend = "uinput"
combo_timeout_ms = 50

[profiles.global]
description = "Global shortcuts inherited by all profiles"

[profiles.global.combos]
"L2+UP" = ["Super", "F"]

[profiles.default]
description = "Default profile"
turn = 0

[profiles.default.buttons]
A = "ENTER"
B = "ESC"
X = "SPACE"
Y = "TAB"
L = "PAGEDOWN"
R = "PAGEUP"
L2 = "LEFTSHIFT"
R2 = "LEFTCTRL"
SELECT = "ESC"
START = "ENTER"
STAR = "SPACE"
HOME = "SLASH"
UP = "UP"
DOWN = "DOWN"
LEFT = "LEFT"
RIGHT = "RIGHT"
"#.to_string())
    }
}

#[tauri::command]
fn write_config(content: String) -> Result<(), String> {
    let path = resolve_config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory {}: {}", parent.display(), e))?;
    }
    fs::write(&path, content)
        .map_err(|e| format!("Failed to write {}: {}", path.display(), e))?;

    // Signal running daemon to reload
    let _ = Command::new("8bitdo-micro-focus-switch")
        .arg("reload")
        .output();

    Ok(())
}

#[tauri::command]
fn get_daemon_status() -> Result<DaemonStatus, String> {
    // Check if daemon process is currently running
    #[cfg(target_os = "linux")]
    {
        if let Ok(output) = Command::new("pgrep").args(["-f", "8bitdo-micro-focus-switch daemon"]).output() {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if let Some(first_line) = stdout.lines().next() {
                    if let Ok(pid) = first_line.trim().parse::<u32>() {
                        return Ok(DaemonStatus {
                            is_running: true,
                            pid: Some(pid),
                        });
                    }
                }
            }
        }
    }

    Ok(DaemonStatus {
        is_running: false,
        pid: None,
    })
}

#[tauri::command]
fn reload_daemon() -> Result<String, String> {
    let output = Command::new("8bitdo-micro-focus-switch")
        .arg("reload")
        .output()
        .map_err(|e| format!("Failed to invoke reload: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        Ok(if stdout.trim().is_empty() { "Daemon configuration reloaded successfully".to_string() } else { stdout })
    } else {
        Err(format!("Reload failed: {}", stderr))
    }
}

#[tauri::command]
fn toggle_daemon(start: bool) -> Result<bool, String> {
    if start {
        // Spawn daemon in background detached
        #[cfg(target_os = "linux")]
        {
            Command::new("8bitdo-micro-focus-switch")
                .arg("daemon")
                .spawn()
                .map_err(|e| format!("Failed to start daemon: {}", e))?;
            return Ok(true);
        }
        #[cfg(not(target_os = "linux"))]
        {
            Command::new("8bitdo-micro-focus-switch")
                .arg("daemon")
                .spawn()
                .map_err(|e| format!("Failed to start daemon: {}", e))?;
            return Ok(true);
        }
    } else {
        // Stop daemon
        #[cfg(target_os = "linux")]
        {
            let _ = Command::new("pkill").args(["-f", "8bitdo-micro-focus-switch daemon"]).output();
        }
        Ok(false)
    }
}

#[tauri::command]
fn get_open_windows() -> Result<Vec<OpenWindowInfo>, String> {
    let mut windows: Vec<OpenWindowInfo> = Vec::new();

    // 1. Try hyprctl clients -j on Hyprland (Linux)
    if let Ok(output) = Command::new("hyprctl").args(["clients", "-j"]).output() {
        if output.status.success() {
            if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&output.stdout) {
                if let Some(list) = val.as_array() {
                    let mut seen = std::collections::HashSet::new();
                    for item in list {
                        let class = item.get("class").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
                        let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
                        let initial_class = item.get("initialClass").and_then(|v| v.as_str()).map(|s| s.to_string());
                        let initial_title = item.get("initialTitle").and_then(|v| v.as_str()).map(|s| s.to_string());

                        if class.is_empty() || class.starts_with("xdg-desktop-portal") {
                            continue;
                        }

                        if seen.insert(class.clone()) {
                            windows.push(OpenWindowInfo {
                                class,
                                title,
                                initial_class,
                                initial_title,
                            });
                        }
                    }
                }
            }
        }
    }

    Ok(windows)
}

#[tauri::command]
fn flash_preset(preset: String, turn: Option<u32>) -> Result<String, String> {
    let mut cmd = Command::new("8bitdo-micro-focus-switch");
    cmd.args(["ble", "write", &preset]);
    if let Some(t) = turn {
        cmd.args(["--turn", &t.to_string()]);
    }
    let output = cmd.output().map_err(|e| format!("Failed to run BLE flash: {}", e))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        Ok(stdout)
    } else {
        Err(format!("Flash error: {}", stderr))
    }
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // Build Context Menu for System Tray
            let toggle = MenuItemBuilder::with_id("toggle", "Open Configurator").build(app)?;
            let reload = MenuItemBuilder::with_id("reload", "Reload Daemon Config").build(app)?;
            let separator = PredefinedMenuItem::separator(app)?;
            let quit = MenuItemBuilder::with_id("quit", "Quit 8BitDo Focus Switch").build(app)?;

            let menu = MenuBuilder::new(app)
                .items(&[&toggle, &reload, &separator, &quit])
                .build()?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .tooltip("8BitDo Micro Focus Switch")
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| {
                    match event.id.as_ref() {
                        "toggle" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                        "reload" => {
                            let _ = Command::new("8bitdo-micro-focus-switch")
                                .arg("reload")
                                .output();
                        }
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
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
                // Keep daemon alive in tray; hide window to free WebView RAM
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
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
        .expect("error while running 8bitdo-micro-gui application");
}
