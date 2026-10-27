use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::{Parser, Subcommand};
use regex::Regex;
use serde_json::json;

use crate::ble::{
    format_snapshot_report, read_hardware_snapshot, scan_for_micro_ble_devices,
    write_hardware_profile,
};
use crate::config::{
    default_config_path, ensure_default_config, load_config, resolve_private_runtime_dir,
    write_private_atomic,
};
use crate::daemon::{find_micro_input_devices_info as find_micro_input_devices, FocusSwitchDaemon};
use crate::input_backend::check_uinput_access;
use std::time::Duration;
use crate::protocol::{get_builtin_hardware_preset, PHYSICAL_BUTTONS};
use crate::recorder::{
    capture_keyboard_chord, format_keys_catalog, set_profile_binding,
};
use crate::visualizer::{generate_html_viewer, generate_svg_diagram};
use crate::window_watcher::{find_hyprland_socket2, query_active_window};

pub const UDEV_RULE_PATH: &str = "/etc/udev/rules.d/99-8bitdo-micro-uinput.rules";
pub const UDEV_RULE_CONTENT: &str = concat!(
    "KERNEL==\"uinput\", SUBSYSTEM==\"misc\", GROUP=\"input\", MODE=\"0660\", TAG+=\"uaccess\", OPTIONS+=\"static_node=uinput\"\n",
    "KERNEL==\"event*\", SUBSYSTEM==\"input\", ATTRS{id/vendor}==\"2dc8\", GROUP=\"input\", MODE=\"0660\", TAG+=\"uaccess\"\n"
);
pub const MODULES_LOAD_PATH: &str = "/etc/modules-load.d/99-8bitdo-micro-uinput.conf";

pub fn is_udev_rule_secure_and_current(content: &str) -> bool {
    !content.contains("0666")
        && !content.contains("ATTRS{name}==\"8BitDo*\"")
        && content.contains("KERNEL==\"uinput\"")
        && content.contains("ATTRS{id/vendor}==\"2dc8\"")
        && content.contains("GROUP=\"input\"")
        && content.contains("MODE=\"0660\"")
        && content.contains("TAG+=\"uaccess\"")
}

#[derive(Parser, Debug)]
#[command(
    name = "8bitdo-micro-focus-switch",
    version,
    about = "Focus-aware input remapper daemon + GUI and direct BLE GATT ('80EL') hardware profile flasher for the 8BitDo Micro."
)]
pub struct Cli {
    /// Path to profiles.toml
    #[arg(short = 'c', long = "config", global = true)]
    pub config: Option<PathBuf>,

    /// Enable debug logging
    #[arg(short = 'v', long = "verbose", global = true)]
    pub verbose: bool,

    /// Launch the graphical configurator & system tray explicitly
    #[arg(long = "gui")]
    pub gui: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Launch the graphical configurator and system tray
    Gui,

    /// [Daily] Show Hyprland/OS focus, connected controller, and profile status
    Status {
        /// Output structured JSON status for UI/plugin integration
        #[arg(long)]
        json: bool,
        /// Also query live hardware state over BLE GATT
        #[arg(long)]
        ble: bool,
        /// Optional BLE MAC address override (e.g. E4:17:D8:D8:52:02)
        #[arg(long)]
        address: Option<String>,
        /// BLE scan timeout in seconds
        #[arg(long, default_value_t = 6.0)]
        timeout: f64,
    },

    /// [Daily] Scan and list all connected and nearby 8BitDo Micro controllers (HID + BLE)
    #[command(visible_alias = "scan")]
    Discover {
        /// BLE scan timeout in seconds
        #[arg(long, default_value_t = 4.0)]
        timeout: f64,
    },

    /// [Daemon] Run the focus-aware profile switching daemon in foreground
    Daemon {
        /// Remapping backend: 'uinput', 'host', 'ble', or 'hybrid'
        #[arg(long, value_parser = ["uinput", "host", "ble", "hybrid"])]
        backend: Option<String>,
        /// On startup, backup hardware config and reset controller to factory keys over BLE if needed
        #[arg(long)]
        ensure_factory_ble: bool,
        /// Optional BLE MAC address override
        #[arg(long)]
        address: Option<String>,
    },

    /// [Hardware] Direct BLE GATT ('80EL') hardware profile reader and flasher (standalone mode)
    Ble {
        #[command(subcommand)]
        subcommand: BleCommands,
    },

    /// [Daily] Open profiles.toml in your editor, jump to a profile, or create new profiles
    #[command(visible_aliases = ["edit", "init-config"])]
    Config {
        /// Profile name to jump to or create (e.g. 'youtube', 'spotify')
        profile: Option<String>,
        /// Print path to profiles.toml and exit
        #[arg(long)]
        path: bool,
        /// List all configured profiles with line numbers and exit
        #[arg(long)]
        list: bool,
        /// Automatically confirm new profile creation without prompting
        #[arg(short = 'y', long)]
        yes: bool,
        /// Override editor command (default: $VISUAL, $EDITOR, or nvim)
        #[arg(long)]
        editor: Option<String>,
        /// Do not launch editor after creating/validating profile
        #[arg(long)]
        no_edit: bool,
    },

    /// [Daily] Interactively bind a button or combo by pressing keys on your keyboard (ESC cancels)
    Bind {
        /// Profile name (e.g. 'agy', 'youtube'). Defaults to focused window.
        profile: Option<String>,
        /// Button (e.g. 'A', 'L1', 'X') or combo (e.g. 'L+R', 'Select+Start').
        button: Option<String>,
        /// Specify key name or chord to bind directly without waiting for keypress
        #[arg(long)]
        key: Option<String>,
        /// Optional inline comment/description for the binding
        #[arg(short = 'd', long)]
        description: Option<String>,
        /// Display catalog of all bindable keys and exit
        #[arg(long)]
        list_keys: bool,
    },

    /// [Daily] Listen and inspect keyboard chords and their TOML representation (ESC exits)
    #[command(visible_aliases = ["listen", "capture"])]
    Record {
        /// Optional timeout in seconds
        #[arg(long)]
        timeout: Option<f64>,
    },

    /// [Daily] Display or search catalog of all bindable keys (Letters, Modifiers, Media, etc.)
    #[command(visible_aliases = ["list-keys", "catalog"])]
    Keys {
        /// Optional keyword filter (e.g. 'volume', 'media', 'nav')
        query: Option<String>,
    },

    /// [Daily] Reload profiles.toml into the running daemon (zero downtime)
    Reload,

    /// [One-Step Setup] Create default profiles.toml, install udev rule, and enable/start systemd service
    #[command(visible_aliases = ["install", "init"])]
    Setup {
        /// Backend flag to bake into the systemd service
        #[arg(long, value_parser = ["uinput", "host", "ble", "hybrid"])]
        backend: Option<String>,
        /// Skip /dev/uinput udev rule check/installation
        #[arg(long)]
        skip_udev: bool,
        /// Skip systemd --user service installation and startup
        #[arg(long)]
        skip_service: bool,
    },

    /// Generate and enable systemd --user service to auto-start daemon on login
    InstallService {
        /// Backend flag to bake into the systemd service
        #[arg(long, value_parser = ["uinput", "host", "ble", "hybrid"])]
        backend: Option<String>,
    },

    /// Install /dev/uinput udev rule
    InstallUdev,

    /// [Daily] Display visual button mapping diagram of focused window or specified profile
    #[command(visible_aliases = ["hud", "diagram"])]
    Show {
        /// Profile name (e.g. 'agy', 'youtube'). If omitted, auto-detects currently focused window.
        profile: Option<String>,
        /// Diagram format (default: 'svg')
        #[arg(long, default_value = "svg", value_parser = ["svg", "html", "png"])]
        format: String,
        /// Open interactive multi-profile HTML viewer with tabs
        #[arg(short = 'i', long = "interactive", visible_alias = "all")]
        interactive: bool,
        /// Save diagram to specified file path instead of opening viewer
        #[arg(short = 'o', long = "save")]
        save: Option<PathBuf>,
        /// Output raw diagram content directly to stdout
        #[arg(long)]
        stdout: bool,
        /// Custom image/browser viewer command (e.g. 'imv', 'chromium', 'xdg-open')
        #[arg(long)]
        viewer: Option<String>,
        /// Do not block waiting for the viewer process to close
        #[arg(long)]
        no_wait: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum BleCommands {
    /// Read current firmware version and mapping from controller
    Read {
        /// Optional BLE MAC address override
        #[arg(long)]
        address: Option<String>,
        /// BLE scan timeout in seconds
        #[arg(long, default_value_t = 6.0)]
        timeout: f64,
        /// Save the raw 180-byte record_micro buffer to a file
        #[arg(long)]
        save_bin: Option<PathBuf>,
    },

    /// Flash a profile from profiles.toml (or preset: 'supermemo', 'anki', 'factory') to controller
    Write {
        /// Profile name in profiles.toml or 'supermemo'/'anki'/'factory'
        profile: String,
        /// Override D-pad rotation angle in degrees (0, 90, 180, 270)
        #[arg(long)]
        turn: Option<u16>,
        /// Optional BLE MAC address override
        #[arg(long)]
        address: Option<String>,
        /// BLE scan timeout in seconds
        #[arg(long, default_value_t = 6.0)]
        timeout: f64,
    },

    /// Backup current hardware profile and reset controller to factory K-mode default (c..t keys)
    Reset {
        /// Optional BLE MAC address override
        #[arg(long)]
        address: Option<String>,
        /// BLE scan timeout in seconds
        #[arg(long, default_value_t = 6.0)]
        timeout: f64,
    },

    /// Scan for nearby 8BitDo Micro BLE GATT endpoints
    #[command(visible_alias = "discover")]
    Scan {
        /// Scan timeout in seconds
        #[arg(long, default_value_t = 4.0)]
        timeout: f64,
    },
}

fn secs(v: f64) -> Duration {
    Duration::from_secs_f64(v.max(0.0))
}

pub fn which_binary(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(name))
            .find(|p| p.is_file())
    })
}

pub fn is_systemd_service_active() -> bool {
    #[cfg(target_os = "linux")]
    {
        if let Ok(status) = Command::new("systemctl")
            .args([
                "--user",
                "is-active",
                "--quiet",
                "8bitdo-micro-focus-switch.service",
            ])
            .status()
        {
            return status.success();
        }
    }
    false
}

pub fn find_daemon_pids() -> Vec<u32> {
    let mut pids = Vec::new();
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        let my_pid = std::process::id();
        let my_uid = unsafe { libc::getuid() };
        if let Ok(entries) = fs::read_dir("/proc") {
            for entry in entries.flatten() {
                let fname = entry.file_name();
                let name_str = fname.to_string_lossy();
                if let Ok(pid) = name_str.parse::<u32>() {
                    if pid == my_pid {
                        continue;
                    }
                    let proc_path = entry.path();
                    if let Ok(meta) = fs::metadata(&proc_path) {
                        if meta.uid() != my_uid {
                            continue;
                        }
                    } else {
                        continue;
                    }
                    let cmdline_path = proc_path.join("cmdline");
                    if let Ok(raw) = fs::read(&cmdline_path) {
                        let args: Vec<String> = raw
                            .split(|&b| b == 0)
                            .filter(|s| !s.is_empty())
                            .map(|s| String::from_utf8_lossy(s).to_string())
                            .collect();
                        let is_our_bin = args
                            .first()
                            .and_then(|arg0| Path::new(arg0).file_name())
                            .and_then(|n| n.to_str())
                            .map(|n| n == "8bitdo-micro-focus-switch")
                            .unwrap_or(false);
                        if is_our_bin && args.iter().skip(1).any(|a| a == "daemon") {
                            pids.push(pid);
                        }
                    }
                }
            }
        }
    }
    pids
}

pub fn reload_running_daemon() -> (bool, String) {
    let mut reloaded_any = false;
    let mut details: Vec<String> = Vec::new();

    if is_systemd_service_active() {
        if let Ok(status) = Command::new("systemctl")
            .args([
                "--user",
                "kill",
                "-s",
                "HUP",
                "8bitdo-micro-focus-switch.service",
            ])
            .status()
        {
            if status.success() {
                reloaded_any = true;
                details.push("systemd service (8bitdo-micro-focus-switch.service)".to_string());
            }
        }
    }

    #[cfg(unix)]
    if !reloaded_any {
        for pid in find_daemon_pids() {
            let rc = unsafe { libc::kill(pid as i32, libc::SIGHUP) };
            if rc == 0 {
                reloaded_any = true;
                details.push(format!("daemon PID {}", pid));
            }
        }
    }

    if reloaded_any {
        (
            true,
            format!("Sent reload signal (SIGHUP) to {}.", details.join(", ")),
        )
    } else {
        (false, "Daemon is not currently running.".to_string())
    }
}

pub fn format_profile_table(config_path: &Path) -> Result<String, String> {
    let cfg = load_config(Some(config_path))?;
    let mut lines = vec![
        format!("Config File      : {}", config_path.display()),
        format!("Default Backend  : {}", cfg.backend),
        format!("Fallback Profile : {}", cfg.fallback_profile),
        String::new(),
        "Available Profiles:".to_string(),
    ];

    if !cfg.global_combos.is_empty() {
        let mut combo_parts: Vec<String> = cfg
            .global_combos
            .iter()
            .map(|(c, act)| {
                let btns: Vec<String> = c.iter().cloned().collect();
                let target = if !act.keys.is_empty() {
                    act.keys.join("+")
                } else {
                    act.exec_cmd.clone().unwrap_or_default()
                };
                format!("{} -> {}", btns.join("+"), target)
            })
            .collect();
        combo_parts.sort();
        lines.push("Global Combos (inherited by all profiles by default):".to_string());
        lines.push(format!("  {}", combo_parts.join(", ")));
        lines.push(String::new());
    }

    for name in &cfg.profile_order {
        let Some(prof) = cfg.profiles.get(name) else {
            continue;
        };
        if name == "global" || name == "all" {
            lines.push(format!(
                "  - {:<12} (global base template: inherited by all profiles)",
                name
            ));
            if !prof.combos.is_empty() {
                let mut combo_parts: Vec<String> = prof
                    .combos
                    .iter()
                    .map(|(c, act)| {
                        let btns: Vec<String> = c.iter().cloned().collect();
                        let target = if !act.keys.is_empty() {
                            act.keys.join("+")
                        } else {
                            act.exec_cmd.clone().unwrap_or_default()
                        };
                        format!("{} -> {}", btns.join("+"), target)
                    })
                    .collect();
                combo_parts.sort();
                lines.push(format!("      Combos: {}", combo_parts.join(", ")));
            }
            continue;
        }

        let match_str = if !prof.match_rules.is_empty() {
            let mut m_parts = Vec::new();
            for r in &prof.match_rules {
                let mut r_parts = Vec::new();
                if let Some(mc) = &r.match_class {
                    r_parts.push(format!("class~={:?}", mc));
                }
                if let Some(mt) = &r.match_title {
                    r_parts.push(format!("title~={:?}", mt));
                }
                if !r_parts.is_empty() {
                    m_parts.push(format!("({})", r_parts.join(" && ")));
                }
            }
            if m_parts.is_empty() {
                "(manual / fallback)".to_string()
            } else {
                m_parts.join(" || ")
            }
        } else {
            let mut m_parts = Vec::new();
            if let Some(mc) = &prof.match_class {
                m_parts.push(format!("class~={:?}", mc));
            }
            if let Some(mt) = &prof.match_title {
                m_parts.push(format!("title~={:?}", mt));
            }
            if m_parts.is_empty() {
                "(manual / fallback)".to_string()
            } else {
                m_parts.join(", ")
            }
        };

        lines.push(format!(
            "  - {:<12} [turn={:>3}°] {}",
            name, prof.turn, match_str
        ));
        if !prof.description.is_empty() {
            lines.push(format!("      {}", prof.description));
        }
        if !prof.combos.is_empty() {
            let mut combo_parts: Vec<String> = prof
                .combos
                .iter()
                .map(|(c, act)| {
                    let btns: Vec<String> = c.iter().cloned().collect();
                    let target = if !act.keys.is_empty() {
                        act.keys.join("+")
                    } else {
                        act.exec_cmd.clone().unwrap_or_default()
                    };
                    format!("{} -> {}", btns.join("+"), target)
                })
                .collect();
            combo_parts.sort();
            lines.push(format!("      Combos: {}", combo_parts.join(", ")));
        }
    }

    Ok(lines.join("\n"))
}

pub fn find_profile_line_number(config_path: &Path, profile_name: &str) -> Option<usize> {
    let norm = profile_name.trim().to_lowercase();
    let pattern = format!(r"^\s*\[(?:profiles\.)?{}\]\s*$", regex::escape(&norm));
    let re = Regex::new(&format!("(?i){}", pattern)).ok()?;
    let content = fs::read_to_string(config_path).ok()?;
    for (idx, line) in content.lines().enumerate() {
        if re.is_match(line.trim()) {
            return Some(idx + 1);
        }
    }
    None
}

pub fn build_editor_command(
    editor_cmd: &str,
    file_path: &Path,
    line_number: Option<usize>,
) -> Vec<String> {
    let mut parts: Vec<String> = editor_cmd
        .split_whitespace()
        .map(|s| s.to_string())
        .collect();
    if parts.is_empty() {
        if which_binary("nvim").is_some() {
            parts.push("nvim".to_string());
        } else {
            parts.push("nano".to_string());
        }
    }

    let Some(lineno) = line_number.filter(|&n| n > 0) else {
        parts.push(file_path.to_string_lossy().to_string());
        return parts;
    };

    let base = Path::new(&parts[0])
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    if matches!(base.as_str(), "code" | "codium" | "cursor") {
        parts.push("-g".to_string());
        parts.push(format!("{}:{}", file_path.display(), lineno));
    } else if matches!(base.as_str(), "hx" | "helix") {
        parts.push(format!("{}:{}", file_path.display(), lineno));
    } else {
        parts.push(format!("+{}", lineno));
        parts.push(file_path.to_string_lossy().to_string());
    }
    parts
}

fn write_systemd_service_unit(
    cfg_path: &Path,
    backend: Option<&str>,
) -> Result<PathBuf, String> {
    let exe = which_binary("8bitdo-micro-focus-switch")
        .or_else(|| std::env::current_exe().ok())
        .unwrap_or_else(|| PathBuf::from("8bitdo-micro-focus-switch"));
    let home = std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    let service_dir = home.join(".config").join("systemd").join("user");
    fs::create_dir_all(&service_dir)
        .map_err(|e| format!("Failed to create {}: {}", service_dir.display(), e))?;
    let service_path = service_dir.join("8bitdo-micro-focus-switch.service");

    let backend_arg = backend
        .map(|b| format!(" --backend {}", b))
        .unwrap_or_default();
    let unit_text = format!(
        "[Unit]\n\
Description=8BitDo Micro Focus-Aware Input Remapper Daemon (Hyprland)\n\
After=graphical-session.target\n\
StartLimitIntervalSec=0\n\
\n\
[Service]\n\
Type=simple\n\
Environment=OMARCHY_PATH=/usr/share/omarchy\n\
Environment=PATH=/usr/share/omarchy/bin:{}/.local/bin:/usr/local/bin:/usr/bin\n\
ExecStart={} daemon --config {} --ensure-factory-ble{}\n\
Restart=always\n\
RestartSec=2\n\
\n\
[Install]\n\
WantedBy=default.target\n",
        home.display(),
        exe.display(),
        cfg_path.display(),
        backend_arg
    );
    write_private_atomic(&service_path, unit_text.as_bytes())?;
    Ok(service_path)
}

fn enable_and_start_systemd_service() -> (bool, String) {
    if which_binary("systemctl").is_none() {
        return (false, "systemctl not found".to_string());
    }
    let _ = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .output();
    let _ = Command::new("systemctl")
        .args([
            "--user",
            "reset-failed",
            "8bitdo-micro-focus-switch.service",
        ])
        .output();
    let was_active = is_systemd_service_active();
    let res_en = Command::new("systemctl")
        .args([
            "--user",
            "enable",
            "--now",
            "8bitdo-micro-focus-switch.service",
        ])
        .output();
    if was_active {
        let _ = Command::new("systemctl")
            .args(["--user", "restart", "8bitdo-micro-focus-switch.service"])
            .output();
    }
    if let Ok(out) = res_en {
        if out.status.success() || is_systemd_service_active() {
            return (
                true,
                "Enabled and started 8bitdo-micro-focus-switch.service".to_string(),
            );
        }
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let err = if !stderr.is_empty() { stderr } else { stdout };
        return (
            false,
            if err.is_empty() {
                format!("systemctl exited with status {}", out.status)
            } else {
                err
            },
        );
    }
    (false, "Failed to invoke systemctl".to_string())
}

async fn cmd_status(
    config: Option<&Path>,
    json_out: bool,
    ble: bool,
    address: Option<&str>,
    timeout: f64,
) -> Result<i32, String> {
    let cfg_path = ensure_default_config(config)?;
    let cfg = load_config(Some(&cfg_path))?;

    let sock = find_hyprland_socket2();
    let win = query_active_window().await;
    let matched = cfg.resolve_profile_for_window(&win.window_class, &win.window_title);

    let service_systemd = is_systemd_service_active();
    let daemon_pids = if service_systemd {
        Vec::new()
    } else {
        find_daemon_pids()
    };
    let service_active = service_systemd || !daemon_pids.is_empty();

    let devs = find_micro_input_devices();
    let devices_info: Vec<serde_json::Value> = devs
        .iter()
        .map(|d| {
            json!({
                "path": d.path,
                "name": d.name,
                "model": d.model,
                "vendor": d.vendor,
                "product": d.product,
                "uniq": d.uniq,
            })
        })
        .collect();

    let (mut uinput_ok, mut uinput_msg) = check_uinput_access();
    if !uinput_ok && which_binary("wtype").is_some() {
        uinput_ok = true;
        uinput_msg =
            "Wayland user-space virtual keyboard (wtype fallback active; no /dev/uinput required)"
                .to_string();
    }

    if json_out {
        let action_json = |act: &crate::config::ButtonAction| -> serde_json::Value {
            let label = if !act.keys.is_empty() {
                act.keys.join("+")
            } else if let Some(cmd) = &act.exec_cmd {
                format!("exec:{}", cmd)
            } else {
                "None".to_string()
            };
            json!({
                "keys": act.keys,
                "exec": act.exec_cmd,
                "label": label,
                "description": act.description.clone().unwrap_or_default(),
            })
        };

        let mut global_combos_list = Vec::new();
        for (c, act) in &cfg.global_combos {
            let buttons: Vec<String> = c.iter().cloned().collect();
            let combo_key = buttons.join("+");
            let mut obj = action_json(act);
            if let Some(map) = obj.as_object_mut() {
                map.insert("combo".to_string(), json!(combo_key));
                map.insert("buttons".to_string(), json!(buttons));
            }
            global_combos_list.push(obj);
        }

        let mut profiles_dict = serde_json::Map::new();
        for pname in &cfg.profile_order {
            let Some(prof) = cfg.profiles.get(pname) else {
                continue;
            };
            let mut btns = serde_json::Map::new();
            for &b in PHYSICAL_BUTTONS {
                if let Some(b_act) = prof.buttons.get(b) {
                    btns.insert(b.to_string(), action_json(b_act));
                } else {
                    btns.insert(
                        b.to_string(),
                        json!({"keys": [], "exec": null, "label": "None", "description": ""}),
                    );
                }
            }
            let mut p_combos = Vec::new();
            for (c, act) in &prof.combos {
                let buttons: Vec<String> = c.iter().cloned().collect();
                let combo_key = buttons.join("+");
                let mut obj = action_json(act);
                if let Some(map) = obj.as_object_mut() {
                    map.insert("combo".to_string(), json!(combo_key));
                    map.insert("buttons".to_string(), json!(buttons));
                }
                p_combos.push(obj);
            }
            profiles_dict.insert(
                pname.clone(),
                json!({
                    "name": prof.name,
                    "description": prof.description,
                    "turn": prof.turn,
                    "match_class": prof.match_class.clone().unwrap_or_default(),
                    "match_title": prof.match_title.clone().unwrap_or_default(),
                    "buttons": btns,
                    "combos": p_combos,
                }),
            );
        }

        let service_mode = if service_systemd {
            "systemd"
        } else if !daemon_pids.is_empty() {
            "standalone"
        } else {
            "inactive"
        };

        let payload = json!({
            "config_path": cfg_path.to_string_lossy(),
            "backend": cfg.backend,
            "fallback_profile": cfg.fallback_profile,
            "active_window": {
                "class": win.window_class,
                "title": win.window_title,
            },
            "active_profile": matched.name,
            "active_turn": matched.turn,
            "service_active": service_active,
            "service_mode": service_mode,
            "uinput_ok": uinput_ok,
            "uinput_message": uinput_msg,
            "connected": !devices_info.is_empty(),
            "devices": devices_info,
            "global_combos": global_combos_list,
            "profiles": profiles_dict,
        });
        println!("{}", serde_json::to_string(&payload).unwrap_or_default());
        return Ok(0);
    }

    println!("=== Hyprland Focus Status ===");
    println!(
        "Socket2 Path     : {}",
        sock.map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "not found".to_string())
    );
    println!(
        "Active Window    : class={:?}, title={:?}",
        win.window_class, win.window_title
    );
    println!(
        "Resolved Profile : {} (turn={}°)",
        matched.name, matched.turn
    );
    println!();

    println!("=== Daemon Background Service ===");
    if service_systemd {
        println!(
            "Service Status   : Active (running via systemd --user 8bitdo-micro-focus-switch.service)"
        );
    } else if !daemon_pids.is_empty() {
        let pids_str: Vec<String> = daemon_pids.iter().map(|p| p.to_string()).collect();
        println!(
            "Service Status   : Active (standalone PID(s): {})",
            pids_str.join(", ")
        );
    } else {
        println!("Service Status   : Inactive");
    }
    println!();

    println!("=== Classic HID Input (/dev/input) ===");
    if devs.is_empty() {
        println!("8BitDo Micro / Zero 2 Keyboard : not connected");
    } else {
        for d in &devs {
            println!(
                "Device           : {} ({:?}, model={}, vendor={}, product={}, uniq={:?})",
                d.path, d.name, d.model, d.vendor, d.product, d.uniq
            );
        }
    }
    println!(
        "uinput Status    : {} ({})",
        if uinput_ok { "OK" } else { "NEEDS PERMISSION" },
        uinput_msg
    );
    println!();

    println!("=== Configured Profiles ===");
    println!("{}", format_profile_table(&cfg_path)?);

    if ble {
        println!("\n=== Live BLE GATT Hardware Snapshot ('80EL') ===");
        let snapshot = read_hardware_snapshot(address, secs(timeout)).await?;
        println!("{}", format_snapshot_report(&snapshot));
    }

    Ok(0)
}

async fn cmd_discover(timeout: f64) -> Result<i32, String> {
    println!("=== 8BitDo Controller Discovery & Connectivity Check ===\n");

    println!("1. Classic Bluetooth HID Devices (/dev/input/event*):");
    let devs = find_micro_input_devices();
    if devs.is_empty() {
        if let Some(diag) = crate::core::daemon::check_linux_input_permissions_diagnostic() {
            println!("   ⚠️  {}", diag.replace('\n', "\n   ⚠️  "));
        } else {
            println!("   • No 8BitDo Micro or Zero 2 paired & connected as HID keyboard right now.");
            println!("     - 8BitDo Micro : ensure slider is set to 'K'");
            println!("     - 8BitDo Zero 2: turn on by holding R + START");
        }
    } else {
        for d in &devs {
            println!(
                "   ✓ Found {}: {:?} [{}] (Vendor={}, Product={}, BT_MAC={})",
                d.path, d.name, d.model, d.vendor, d.product, d.uniq
            );
        }
    }
    println!();

    println!("2. Over-the-Air BLE GATT Peripherals ('80EL' - 8BitDo Micro only):");
    println!(
        "   Scanning 2.4GHz BLE advertisements for {:.1}s ...",
        timeout
    );
    match scan_for_micro_ble_devices(secs(timeout)).await {
        Ok(ble_devs) => {
            if ble_devs.is_empty() {
                println!("   • No '80EL' BLE peripherals detected nearby.");
                println!("     (Note: BLE GATT hardware flashing is specific to 8BitDo Micro)");
            } else {
                for b in ble_devs {
                    let rssi_str = b
                        .rssi
                        .map(|r| format!("RSSI={} dBm", r))
                        .unwrap_or_default();
                    println!(
                        "   ✓ Found {:?} at BLE_MAC={} ({})",
                        b.name, b.address, rssi_str
                    );
                }
            }
        }
        Err(e) => {
            println!("   BLE scan error: {}", e);
        }
    }

    println!("\nSummary:");
    println!(
        " • Controller discovery is 100% dynamic: no MAC addresses or event paths are hard-coded."
    );
    println!(
        " • Any 8BitDo Micro or Zero 2 will be auto-detected upon pairing and powering on."
    );
    Ok(0)
}

async fn cmd_ble_read(
    address: Option<&str>,
    timeout: f64,
    save_bin: Option<&Path>,
) -> Result<i32, String> {
    let snapshot = read_hardware_snapshot(address, secs(timeout)).await?;
    println!("{}", format_snapshot_report(&snapshot));
    if let Some(out_path) = save_bin {
        write_private_atomic(out_path, &snapshot.raw_record)?;
        println!(
            "\nSaved raw 180-byte record_micro to {}",
            out_path.display()
        );
    }
    Ok(0)
}

async fn cmd_ble_write(
    config: Option<&Path>,
    profile_name: &str,
    turn_override: Option<u16>,
    address: Option<&str>,
    timeout: f64,
) -> Result<i32, String> {
    let cfg_path = ensure_default_config(config)?;
    let cfg = load_config(Some(&cfg_path))?;

    let prof = cfg.get_profile(profile_name)?;
    let is_factory = matches!(
        profile_name.trim().to_lowercase().as_str(),
        "factory" | "default" | "reset"
    );
    let mut hw_prof = prof.to_hardware_profile(!is_factory)?;
    if let Some(t) = turn_override {
        hw_prof.turn = match t {
            0 => 0,
            1 | 90 => 1,
            2 | 180 => 2,
            3 | 270 => 3,
            _ => return Err("Turn angle must be 0, 90, 180, or 270".to_string()),
        };
    }

    let (num_combos, num_exec) = prof.software_only_features();
    if num_combos > 0 || num_exec > 0 {
        println!(
            "\n⚠️  Notice: Profile {:?} contains software-only features ({} combo(s), {} shell command(s)).\n\
             The 180-byte 8BitDo Micro hardware memory only supports 1-to-1 button mappings.\n\
             Combos and shell commands are handled by the daemon (uinput backend) and will not exist in hardware.\n",
            prof.name, num_combos, num_exec
        );
    }

    println!(
        "Flashing profile {:?} (enabled={}, turn={}°) to '80EL' ...",
        prof.name,
        hw_prof.enabled,
        hw_prof.turn_degrees()
    );
    let snapshot = write_hardware_profile(&hw_prof, address, secs(timeout), true).await?;
    println!("Flash & Save complete! Verified hardware state:\n");
    println!("{}", format_snapshot_report(&snapshot));
    Ok(0)
}

async fn cmd_ble_reset(address: Option<&str>, timeout: f64) -> Result<i32, String> {
    let cfg_path = default_config_path();
    let backup_path = cfg_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("hardware-backup.bin");
    println!("Reading current hardware profile before reset ...");
    let before = read_hardware_snapshot(address, secs(timeout)).await?;
    let _ = write_private_atomic(&backup_path, &before.raw_record);
    println!(
        "Backed up previous 180-byte hardware config to {}",
        backup_path.display()
    );

    let factory_hw = get_builtin_hardware_preset("factory")?;
    println!("Resetting 8BitDo Micro hardware profile to Factory Default (unique c..t keys) ...");
    let after = write_hardware_profile(&factory_hw, address, secs(timeout), true).await?;
    println!("Reset complete! Verified hardware state:\n");
    println!("{}", format_snapshot_report(&after));
    Ok(0)
}

async fn cmd_daemon(
    config: Option<&Path>,
    backend: Option<String>,
    ensure_factory_ble: bool,
    address: Option<String>,
) -> Result<i32, String> {
    let cfg_path = ensure_default_config(config)?;
    let cfg = load_config(Some(&cfg_path))?;

    if ensure_factory_ble {
        if let Ok(snap) = read_hardware_snapshot(address.as_deref(), Duration::from_secs(4)).await {
            if snap.profile.enabled {
                let backup_path = cfg_path
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join("hardware-backup.bin");
                let _ = write_private_atomic(&backup_path, &snap.raw_record);
                eprintln!(
                    "[8bitdo-micro] Backed up custom hardware profile to {} and resetting controller to factory keys...",
                    backup_path.display()
                );
                if let Ok(factory) = get_builtin_hardware_preset("factory") {
                    let _ = write_hardware_profile(&factory, address.as_deref(), Duration::from_secs(4), true).await;
                }
            }
        }
    }

    let daemon = FocusSwitchDaemon::new(cfg, Some(cfg_path), backend, address);
    let (_stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let (reload_tx, reload_rx) = tokio::sync::mpsc::unbounded_channel();

    #[cfg(unix)]
    {
        let reload_tx_sig = reload_tx.clone();
        tokio::spawn(async move {
            if let Ok(mut sig) =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())
            {
                while sig.recv().await.is_some() {
                    let _ = reload_tx_sig.send(());
                }
            }
        });
    }

    daemon.run(stop_rx, reload_rx).await?;
    Ok(0)
}

fn cmd_config(
    config: Option<&Path>,
    profile: Option<&str>,
    is_init_config: bool,
    path_flag: bool,
    list_flag: bool,
    yes: bool,
    editor_override: Option<&str>,
    no_edit: bool,
) -> Result<i32, String> {
    let cfg_path = ensure_default_config(config)?;

    if is_init_config && profile.is_none() {
        println!("Configuration ready at: {}", cfg_path.display());
        return Ok(0);
    }

    if path_flag {
        println!("{}", cfg_path.display());
        return Ok(0);
    }

    if list_flag {
        let cfg = load_config(Some(&cfg_path))?;
        println!("Config File: {}", cfg_path.display());
        println!("Configured Profiles:");
        for name in &cfg.profile_order {
            let loc = find_profile_line_number(&cfg_path, name)
                .map(|n| format!(" (line {})", n))
                .unwrap_or_default();
            println!("  - {:<14}{}", name, loc);
        }
        return Ok(0);
    }

    let mut target_line = Some(1usize);

    if let Some(target_prof) = profile {
        let norm_name = target_prof.trim().to_lowercase();
        if let Some(existing_line) = find_profile_line_number(&cfg_path, &norm_name) {
            target_line = Some(existing_line);
            println!(
                "Found existing profile [{}] in {} (line {}).",
                norm_name,
                cfg_path.display(),
                existing_line
            );
        } else {
            println!("Profile {:?} not found in {}.", norm_name, cfg_path.display());
            let mut create_new = yes;
            if !create_new {
                print!(
                    "Would you like to create a new profile for {:?}? [Y/n]: ",
                    norm_name
                );
                let _ = io::stdout().flush();
                let mut ans = String::new();
                if io::stdin().read_line(&mut ans).is_err() {
                    println!("\nAborted.");
                    return Ok(130);
                }
                let trimmed = ans.trim().to_lowercase();
                if !matches!(trimmed.as_str(), "" | "y" | "yes") {
                    println!("Aborted.");
                    return Ok(0);
                }
                create_new = true;
            }

            if create_new {
                let mut chars = norm_name.chars();
                let title_name = match chars.next() {
                    None => String::new(),
                    Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
                };
                let desc = format!("{} Shortcuts", title_name);
                let match_cls = norm_name.clone();
                let turn = 0;

                let template = if norm_name == "global" || norm_name == "all" {
                    format!(
                        "\n# -----------------------------------------------------------------------------\n\
                         # Global Base Profile: {}\n\
                         # -----------------------------------------------------------------------------\n\
                         [profiles.{}]\n\
                         description = \"Global defaults inherited by all profiles unless overridden\"\n\
                         \n\
                         [profiles.{}.combos]\n\
                         \"L+R\" = [\"Super\", \"K\"]\n\
                         \"Select+Start\" = \"exec:hyprctl dispatch togglespecialworkspace\"\n",
                        norm_name.to_uppercase(),
                        norm_name,
                        norm_name
                    )
                } else {
                    format!(
                        "\n# -----------------------------------------------------------------------------\n\
                         # Profile: {}\n\
                         # -----------------------------------------------------------------------------\n\
                         [profiles.{}]\n\
                         description = {:?}\n\
                         match_class = {:?}\n\
                         turn = {}\n\
                         \n\
                         [profiles.{}.buttons]\n\
                         # Face buttons\n\
                         A = \"Space\"\n\
                         B = \"Enter\"\n\
                         X = \"PageUp\"\n\
                         Y = \"PageDown\"\n\
                         \n\
                         # Shoulders\n\
                         L = \"Tab\"\n\
                         R = \"Escape\"\n\
                         L2 = \"LeftShift\"\n\
                         R2 = \"LeftCtrl\"\n\
                         \n\
                         # Center & Bottom\n\
                         SELECT = \"Esc\"\n\
                         START = \"Enter\"\n\
                         STAR = \"None\"\n\
                         HOME = \"None\"\n\
                         \n\
                         # D-Pad\n\
                         UP = \"Up\"\n\
                         DOWN = \"Down\"\n\
                         LEFT = \"Left\"\n\
                         RIGHT = \"Right\"\n",
                        norm_name.to_uppercase(),
                        norm_name,
                        desc,
                        match_cls,
                        turn,
                        norm_name
                    )
                };

                let mut existing_text = fs::read_to_string(&cfg_path).unwrap_or_default();
                if !existing_text.ends_with('\n') {
                    existing_text.push('\n');
                }
                existing_text.push_str(template.trim_start());
                write_private_atomic(&cfg_path, existing_text.as_bytes())?;
                println!(
                    "Created new profile [{}] in {}.",
                    norm_name,
                    cfg_path.display()
                );
                target_line = find_profile_line_number(&cfg_path, &norm_name);
            }
        }
    }

    if no_edit {
        return Ok(0);
    }

    let editor = editor_override
        .map(|s| s.to_string())
        .or_else(|| std::env::var("VISUAL").ok())
        .or_else(|| std::env::var("EDITOR").ok())
        .unwrap_or_else(|| "nvim".to_string());
    let cmd = build_editor_command(&editor, &cfg_path, target_line);
    let line_info = target_line
        .map(|n| format!(" at line {}", n))
        .unwrap_or_default();
    println!(
        "Opening {}{} with {}...",
        cfg_path.file_name().unwrap_or_default().to_string_lossy(),
        line_info,
        cmd[0]
    );

    if Command::new(&cmd[0]).args(&cmd[1..]).status().is_err() {
        let fallback = if which_binary("nano").is_some() {
            "nano"
        } else {
            "vi"
        };
        let fallback_cmd = build_editor_command(fallback, &cfg_path, target_line);
        let _ = Command::new(&fallback_cmd[0])
            .args(&fallback_cmd[1..])
            .status();
    }

    match load_config(Some(&cfg_path)) {
        Ok(_) => {
            println!("✓ Configuration syntax is valid ({}).", cfg_path.display());
            let (success, msg) = reload_running_daemon();
            if success {
                println!("✓ {}", msg);
            }
        }
        Err(e) => {
            eprintln!(
                "⚠️ Warning: Configuration syntax error in {}: {}",
                cfg_path.display(),
                e
            );
        }
    }

    Ok(0)
}

async fn cmd_bind(
    config: Option<&Path>,
    profile: Option<&str>,
    button: Option<&str>,
    key_arg: Option<&str>,
    description: Option<&str>,
    list_keys: bool,
) -> Result<i32, String> {
    let cfg_path = ensure_default_config(config)?;
    let cfg = load_config(Some(&cfg_path))?;

    if list_keys {
        println!("{}", format_keys_catalog(None));
        return Ok(0);
    }

    let mut target_profile = profile.map(|s| s.to_string());
    if target_profile.is_none() {
        let active_win = query_active_window().await;
        if !active_win.window_class.is_empty() || !active_win.window_title.is_empty() {
            let matched =
                cfg.resolve_profile_for_window(&active_win.window_class, &active_win.window_title);
            target_profile = Some(matched.name.clone());
            println!(
                "Focused Window: {:?} -> Profile: {:?}",
                active_win.window_class, matched.name
            );
        }
    }
    let target_profile = target_profile.unwrap_or_else(|| cfg.fallback_profile.clone());

    if !cfg.profiles.contains_key(&target_profile) {
        eprintln!(
            "Error: Profile {:?} not found in {}.",
            target_profile,
            cfg_path.display()
        );
        eprintln!("Configured profiles: {}", cfg.profile_order.join(", "));
        return Ok(1);
    }

    let target_btn = match button {
        Some(b) => b.to_string(),
        None => {
            println!("\nRemapping profile [{}]:", target_profile);
            println!(
                "  Buttons: A, B, X, Y, L, R, L2, R2, SELECT, START, STAR, HOME, UP, DOWN, LEFT, RIGHT"
            );
            println!("  Combos : E.g. 'L+R', 'L2+LEFT', 'Select+Start'");
            print!("\nEnter button or combo to bind: ");
            let _ = io::stdout().flush();
            let mut input = String::new();
            if io::stdin().read_line(&mut input).is_err() || input.trim().is_empty() {
                println!("Aborted.");
                return Ok(0);
            }
            input.trim().to_string()
        }
    };

    let keys: Vec<String> = if let Some(k) = key_arg {
        if k.starts_with("exec:") {
            vec![k.to_string()]
        } else if k.contains('+') && !k.starts_with("KEY_") {
            k.split('+')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        } else {
            vec![k.trim().to_string()]
        }
    } else {
        println!(
            "\n>> Listening for keyboard input for [{}.{}]...",
            target_profile, target_btn
        );
        println!(
            ">> Press any key or combination on your keyboard (e.g. Space, Ctrl+Shift+P, Super+K)."
        );
        println!(">> Press ESC to cancel.");
        let res = capture_keyboard_chord(None).await?;
        if res.cancelled {
            println!("\n[ESC pressed] Cancelled. No changes made.");
            return Ok(0);
        }
        res.keys
    };

    if keys.is_empty() {
        println!("No keys captured. Aborted.");
        return Ok(0);
    }

    set_profile_binding(&cfg_path, &target_profile, &target_btn, &keys, description)?;

    let keys_disp = if keys.len() > 1 {
        keys.join(" + ")
    } else {
        keys[0].clone()
    };
    println!("\n✓ Bound [{}] {} = {}", target_profile, target_btn, keys_disp);
    println!("✓ Saved to {}", cfg_path.display());

    let (success, msg) = reload_running_daemon();
    if success {
        println!("✓ {}", msg);
    }
    Ok(0)
}

async fn cmd_record(timeout: Option<f64>) -> Result<i32, String> {
    println!("=== 8BitDo Micro Live Keyboard Chord Recorder ===");
    println!(
        "Press any key or combination on your keyboard to inspect its chord name and TOML format."
    );
    println!("Press ESC to exit.\n");

    loop {
        let res = capture_keyboard_chord(timeout.map(secs)).await?;
        if res.cancelled {
            println!("\n[ESC pressed] Exited.");
            break;
        }
        let chord_str = res.keys.join(" + ");
        println!(
            "  • Captured: {:<24} -->  TOML: {}",
            chord_str,
            res.toml_value()
        );
    }
    Ok(0)
}

fn cmd_reload(config: Option<&Path>) -> Result<i32, String> {
    let cfg_path = ensure_default_config(config)?;
    let cfg = match load_config(Some(&cfg_path)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "Error: Invalid configuration in {}:\n  {}",
                cfg_path.display(),
                e
            );
            return Ok(1);
        }
    };

    println!("Validated configuration syntax: {}", cfg_path.display());
    println!(
        "Configured profiles ({}): {}",
        cfg.profiles.len(),
        cfg.profile_order.join(", ")
    );

    let (success, msg) = reload_running_daemon();
    if success {
        println!("✓ {}", msg);
    } else {
        println!("ℹ {}", msg);
        println!("  To start the background service:");
        println!("    systemctl --user start 8bitdo-micro-focus-switch.service");
        println!("  Or run in foreground:");
        println!("    8bitdo-micro-focus-switch daemon");
    }
    Ok(0)
}

fn cmd_install_udev() -> Result<i32, String> {
    let (uinput_ok, _msg) = check_uinput_access();
    let existing_rule = std::fs::read_to_string(UDEV_RULE_PATH).unwrap_or_default();
    let rule_secure = is_udev_rule_secure_and_current(&existing_rule);
    if uinput_ok && Path::new(MODULES_LOAD_PATH).exists() && rule_secure {
        println!("✓ /dev/uinput and controller evdev seat-scoped uaccess rules are already configured!");
        return Ok(0);
    }

    println!(
        "Configuring seat-scoped uaccess udev rules ({}) and boot module ({}) ...",
        UDEV_RULE_PATH, MODULES_LOAD_PATH
    );
    let backup_path = format!("{}.bak", UDEV_RULE_PATH);
    let cmd = format!(
        "if [ -f {path} ]; then sudo cp -p {path} {bak}; fi && printf '%s\\n' '{rule}' | sudo tee {path} > /dev/null && echo 'uinput' | sudo tee {modpath} > /dev/null && sudo modprobe uinput && if [ -e /dev/uinput ]; then sudo chgrp input /dev/uinput && sudo chmod 0660 /dev/uinput; fi && sudo udevadm control --reload-rules && sudo udevadm trigger --sysname-match=uinput && sudo udevadm trigger --subsystem-match=input",
        path = UDEV_RULE_PATH,
        bak = backup_path,
        rule = UDEV_RULE_CONTENT.trim(),
        modpath = MODULES_LOAD_PATH
    );
    if let Ok(status) = Command::new("sudo").args(["sh", "-c", &cmd]).status() {
        if status.success() {
            println!("✓ Configured seat-scoped uaccess permissions (MODE=0660, GROUP=input, TAG+=uaccess) and enabled uinput module!");
            return Ok(0);
        }
    }

    println!("\nPlease run the following command in your terminal with sudo:");
    println!(
        "  printf '%s\\n' '{}' | sudo tee {}",
        UDEV_RULE_CONTENT.trim(),
        UDEV_RULE_PATH
    );
    println!(
        "  echo 'uinput' | sudo tee {} && sudo modprobe uinput && sudo chgrp input /dev/uinput && sudo chmod 0660 /dev/uinput",
        MODULES_LOAD_PATH
    );
    println!("  sudo udevadm control --reload-rules && sudo udevadm trigger --sysname-match=uinput && sudo udevadm trigger --subsystem-match=input");
    Ok(1)
}

fn cmd_install_service(config: Option<&Path>, backend: Option<&str>) -> Result<i32, String> {
    let cfg_path = ensure_default_config(config)?;
    let service_path = write_systemd_service_unit(&cfg_path, backend)?;
    println!("✓ Wrote systemd user unit: {}", service_path.display());
    let (ok, msg) = enable_and_start_systemd_service();
    if ok {
        println!("✓ {}", msg);
    } else {
        println!(
            "ℹ Could not auto-start systemd service ({}). To start manually:",
            msg
        );
        println!(
            "  systemctl --user daemon-reload && systemctl --user enable --now 8bitdo-micro-focus-switch.service"
        );
    }

    let (uinput_ok, _) = check_uinput_access();
    if !uinput_ok {
        println!("\nNOTE: For multi-button combos or instant <1ms remapping, run:");
        println!("  8bitdo-micro-focus-switch install-udev");
    }
    Ok(0)
}

fn cmd_setup(
    config: Option<&Path>,
    backend: Option<&str>,
    skip_udev: bool,
    skip_service: bool,
) -> Result<i32, String> {
    let cfg_path = ensure_default_config(config)?;
    let cfg = load_config(Some(&cfg_path))?;
    println!(
        "✓ Configuration ready: {} ({} profiles, {} global combos)",
        cfg_path.display(),
        cfg.profiles.len(),
        cfg.global_combos.len()
    );

    if !skip_udev {
        let (uinput_ok, _) = check_uinput_access();
        let rule_secure = std::fs::read_to_string(UDEV_RULE_PATH)
            .map(|s| is_udev_rule_secure_and_current(&s))
            .unwrap_or(false);
        if uinput_ok && rule_secure {
            println!("✓ /dev/uinput & controller evdev seat-scoped permissions: OK");
        } else {
            let _ = cmd_install_udev();
        }
    }

    if !skip_service {
        let service_path = write_systemd_service_unit(&cfg_path, backend)?;
        println!("✓ Installed systemd user service: {}", service_path.display());
        let (ok, msg) = enable_and_start_systemd_service();
        if ok {
            println!("✓ {}", msg);
        } else {
            println!("ℹ Systemd service written ({})", msg);
        }
    }
    Ok(0)
}

#[allow(clippy::too_many_arguments)]
async fn cmd_show(
    config: Option<&Path>,
    profile: Option<&str>,
    format: &str,
    interactive: bool,
    save: Option<&Path>,
    stdout_flag: bool,
    viewer: Option<&str>,
    no_wait: bool,
) -> Result<i32, String> {
    let cfg_path = ensure_default_config(config)?;
    let cfg = load_config(Some(&cfg_path))?;

    let mut target_profile_name = profile.map(|s| s.to_string());
    if target_profile_name.is_none() {
        let active_win = query_active_window().await;
        if !active_win.window_class.is_empty() || !active_win.window_title.is_empty() {
            let matched =
                cfg.resolve_profile_for_window(&active_win.window_class, &active_win.window_title);
            target_profile_name = Some(matched.name.clone());
            eprintln!(
                "Focused Window: class={:?}, title={:?} -> Profile: {}",
                active_win.window_class, active_win.window_title, matched.name
            );
        }
    }
    let target_profile_name =
        target_profile_name.unwrap_or_else(|| cfg.fallback_profile.clone());
    let prof = cfg.get_profile(&target_profile_name)?;

    let is_interactive = interactive || format == "html";
    let (content, ext) = if is_interactive {
        (
            generate_html_viewer(&cfg.profiles, &cfg.profile_order, &prof.name),
            ".html",
        )
    } else {
        (generate_svg_diagram(&prof), ".svg")
    };

    if stdout_flag {
        print!("{}", content);
        return Ok(0);
    }

    if let Some(out_path) = save {
        write_private_atomic(out_path, content.as_bytes())?;
        println!("Saved diagram to: {}", out_path.display());
        return Ok(0);
    }

    let safe_prof_name: String = prof
        .name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    let preview_name = if safe_prof_name.is_empty() {
        "profile".to_string()
    } else {
        safe_prof_name
    };
    let tmp_path = resolve_private_runtime_dir().join(format!("8bitdo-micro-{}{}", preview_name, ext));
    write_private_atomic(&tmp_path, content.as_bytes())
        .map_err(|e| format!("Failed to write temp diagram: {}", e))?;

    let viewer_cmd = match viewer {
        Some(v) => v.to_string(),
        None => {
            if ext == ".html" {
                if which_binary("chromium").is_some() {
                    "chromium".to_string()
                } else {
                    "xdg-open".to_string()
                }
            } else if which_binary("imv").is_some() {
                "imv".to_string()
            } else {
                "xdg-open".to_string()
            }
        }
    };

    eprintln!(
        "Displaying {} ({}) via {} ...",
        prof.name,
        ext.trim_start_matches('.').to_uppercase(),
        viewer_cmd
    );
    if let Ok(mut child) = Command::new(&viewer_cmd).arg(&tmp_path).spawn() {
        if !no_wait {
            let _ = child.wait();
        }
    }
    Ok(0)
}

pub async fn run_cli_async(args: Vec<String>) -> i32 {
    let is_init_config = args.iter().any(|a| a == "init-config");
    let cli = match Cli::try_parse_from(&args) {
        Ok(c) => c,
        Err(e) => {
            let _ = e.print();
            return e.exit_code();
        }
    };

    let config_ref = cli.config.as_deref();

    let res = match cli.command {
        None | Some(Commands::Gui) => Ok(0),
        Some(Commands::Status {
            json,
            ble,
            address,
            timeout,
        }) => cmd_status(config_ref, json, ble, address.as_deref(), timeout).await,
        Some(Commands::Discover { timeout }) => cmd_discover(timeout).await,
        Some(Commands::Daemon {
            backend,
            ensure_factory_ble,
            address,
        }) => cmd_daemon(config_ref, backend, ensure_factory_ble, address).await,
        Some(Commands::Ble { subcommand }) => match subcommand {
            BleCommands::Read {
                address,
                timeout,
                save_bin,
            } => cmd_ble_read(address.as_deref(), timeout, save_bin.as_deref()).await,
            BleCommands::Write {
                profile,
                turn,
                address,
                timeout,
            } => cmd_ble_write(config_ref, &profile, turn, address.as_deref(), timeout).await,
            BleCommands::Reset { address, timeout } => {
                cmd_ble_reset(address.as_deref(), timeout).await
            }
            BleCommands::Scan { timeout } => cmd_discover(timeout).await,
        },
        Some(Commands::Config {
            profile,
            path,
            list,
            yes,
            editor,
            no_edit,
        }) => cmd_config(
            config_ref,
            profile.as_deref(),
            is_init_config,
            path,
            list,
            yes,
            editor.as_deref(),
            no_edit,
        ),
        Some(Commands::Bind {
            profile,
            button,
            key,
            description,
            list_keys,
        }) => {
            cmd_bind(
                config_ref,
                profile.as_deref(),
                button.as_deref(),
                key.as_deref(),
                description.as_deref(),
                list_keys,
            )
            .await
        }
        Some(Commands::Record { timeout }) => cmd_record(timeout).await,
        Some(Commands::Keys { query }) => {
            println!("{}", format_keys_catalog(query.as_deref()));
            Ok(0)
        }
        Some(Commands::Reload) => cmd_reload(config_ref),
        Some(Commands::Setup {
            backend,
            skip_udev,
            skip_service,
        }) => cmd_setup(config_ref, backend.as_deref(), skip_udev, skip_service),
        Some(Commands::InstallService { backend }) => {
            cmd_install_service(config_ref, backend.as_deref())
        }
        Some(Commands::InstallUdev) => cmd_install_udev(),
        Some(Commands::Show {
            profile,
            format,
            interactive,
            save,
            stdout,
            viewer,
            no_wait,
        }) => {
            cmd_show(
                config_ref,
                profile.as_deref(),
                &format,
                interactive,
                save.as_deref(),
                stdout,
                viewer.as_deref(),
                no_wait,
            )
            .await
        }
    };

    match res {
        Ok(code) => code,
        Err(e) => {
            eprintln!("Error: {}", e);
            1
        }
    }
}

pub fn run_cli_from_args(args: Vec<String>) -> i32 {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to initialize tokio runtime");
    rt.block_on(run_cli_async(args))
}
