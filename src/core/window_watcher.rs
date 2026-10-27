//! Cross-platform active window watcher and open window enumerator for Linux (Hyprland), Windows (Win32), and macOS.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ActiveWindow {
    pub window_class: String,
    pub window_title: String,
    pub address: String,
    pub process_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenWindowInfo {
    pub class: String,
    pub title: String,
    pub initial_class: Option<String>,
    pub initial_title: Option<String>,
}

// ---------------------------------------------------------------------------
// Linux / Hyprland Focus Watcher (.socket2.sock + hyprctl)
// ---------------------------------------------------------------------------

pub fn find_hyprland_socket2() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{FileTypeExt, MetadataExt};

        let my_uid = unsafe { libc::getuid() };
        let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE")
            .ok()
            .filter(|s| !s.is_empty() && !s.contains('/') && !s.contains('\\') && !s.contains(".."));
        let xdg_runtime = std::env::var("XDG_RUNTIME_DIR")
            .unwrap_or_else(|_| format!("/run/user/{}", my_uid));

        let is_valid_owned_socket = |p: &std::path::Path| -> Option<std::time::SystemTime> {
            let meta = std::fs::symlink_metadata(p).ok()?;
            if !meta.file_type().is_socket() || meta.uid() != my_uid {
                return None;
            }
            if let Some(parent) = p.parent() {
                let p_meta = std::fs::symlink_metadata(parent).ok()?;
                if !p_meta.is_dir() || p_meta.uid() != my_uid {
                    return None;
                }
            }
            Some(meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH))
        };

        let mut candidates = Vec::new();
        if let Some(ref s) = sig {
            candidates.push(PathBuf::from(&xdg_runtime).join("hypr").join(s).join(".socket2.sock"));
            candidates.push(PathBuf::from("/tmp/hypr").join(s).join(".socket2.sock"));
        }

        for base in [PathBuf::from(&xdg_runtime).join("hypr"), PathBuf::from("/tmp/hypr")] {
            if let Ok(entries) = std::fs::read_dir(&base) {
                let mut sock_files: Vec<(std::time::SystemTime, PathBuf)> = entries
                    .flatten()
                    .map(|e| e.path().join(".socket2.sock"))
                    .filter_map(|p| is_valid_owned_socket(&p).map(|mtime| (mtime, p)))
                    .collect();
                sock_files.sort_by(|a, b| b.0.cmp(&a.0));
                for (_, p) in sock_files {
                    candidates.push(p);
                }
            }
        }

        for cand in candidates {
            if is_valid_owned_socket(&cand).is_some() {
                return Some(cand);
            }
        }
    }
    None
}

pub fn parse_socket2_event_line(line: &str) -> Option<ActiveWindow> {
    let stripped = line.trim_end_matches(&['\r', '\n'][..]);
    let payload = stripped.strip_prefix("activewindow>>")?;
    if let Some((w_class, w_title)) = payload.split_once(',') {
        Some(ActiveWindow {
            window_class: w_class.to_string(),
            window_title: w_title.to_string(),
            address: String::new(),
            process_name: String::new(),
        })
    } else {
        Some(ActiveWindow {
            window_class: payload.to_string(),
            window_title: String::new(),
            address: String::new(),
            process_name: String::new(),
        })
    }
}

pub async fn query_hyprland_active_window() -> ActiveWindow {
    let sock = find_hyprland_socket2();
    let mut cmd = tokio::process::Command::new("hyprctl");
    cmd.args(["activewindow", "-j"]);
    if let Some(ref s) = sock {
        if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none() {
            if let Some(parent_name) = s.parent().and_then(|p| p.file_name()) {
                cmd.env("HYPRLAND_INSTANCE_SIGNATURE", parent_name);
            }
        }
    }
    if let Ok(Ok(output)) = tokio::time::timeout(Duration::from_secs(2), cmd.output()).await {
        if output.status.success() {
            if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&output.stdout) {
                let w_class = val
                    .get("class")
                    .or_else(|| val.get("initialClass"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let w_title = val
                    .get("title")
                    .or_else(|| val.get("initialTitle"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let w_addr = val
                    .get("address")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                return ActiveWindow {
                    window_class: w_class,
                    window_title: w_title,
                    address: w_addr,
                    process_name: String::new(),
                };
            }
        }
    }
    ActiveWindow::default()
}

// ---------------------------------------------------------------------------
// Windows Focus Watcher & Window Enumerator (Win32 API)
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
pub fn query_windows_active_window() -> ActiveWindow {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW,
        GetWindowThreadProcessId,
    };

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return ActiveWindow::default();
        }

        let len = GetWindowTextLengthW(hwnd);
        let mut title_buf = vec![0u16; (len as usize) + 2];
        let copied = GetWindowTextW(hwnd, title_buf.as_mut_ptr(), (len + 1) as i32);
        let title = String::from_utf16_lossy(&title_buf[..copied as usize]);

        let mut class_buf = [0u16; 256];
        let class_len = GetClassNameW(hwnd, class_buf.as_mut_ptr(), 256);
        let w_class = String::from_utf16_lossy(&class_buf[..class_len as usize]);

        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        let proc_name = if pid != 0 {
            format!("pid_{}", pid)
        } else {
            String::new()
        };

        ActiveWindow {
            window_class: w_class,
            window_title: title,
            address: format!("0x{:x}", hwnd as usize),
            process_name: proc_name,
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn query_windows_active_window() -> ActiveWindow {
    ActiveWindow::default()
}

// ---------------------------------------------------------------------------
// macOS Focus Watcher
// ---------------------------------------------------------------------------

pub fn query_macos_active_window() -> ActiveWindow {
    #[cfg(target_os = "macos")]
    {
        let script = r#"tell application "System Events" to get {name, title of front window} of first application process whose frontmost is true"#;
        if let Ok(output) = std::process::Command::new("osascript")
            .args(["-e", script])
            .output()
        {
            if output.status.success() {
                let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !s.is_empty() {
                    let mut parts = s.splitn(2, ',');
                    let app_name = parts.next().unwrap_or("").trim().to_string();
                    let win_title = parts
                        .next()
                        .map(|t| t.trim().to_string())
                        .filter(|t| !t.is_empty())
                        .unwrap_or_else(|| app_name.clone());
                    return ActiveWindow {
                        window_class: app_name.clone(),
                        window_title: win_title,
                        address: String::new(),
                        process_name: app_name,
                    };
                }
            }
        }
    }
    ActiveWindow::default()
}

pub async fn query_active_window() -> ActiveWindow {
    #[cfg(target_os = "windows")]
    {
        return query_windows_active_window();
    }
    #[cfg(target_os = "macos")]
    {
        return query_macos_active_window();
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        query_hyprland_active_window().await
    }
}

pub fn spawn_focus_watcher() -> mpsc::UnboundedReceiver<ActiveWindow> {
    let (tx, rx) = mpsc::unbounded_channel();

    tokio::spawn(async move {
        #[cfg(target_os = "linux")]
        {
            use tokio::io::{AsyncBufReadExt, BufReader};
            use tokio::net::UnixStream;

            let initial = query_hyprland_active_window().await;
            let _ = tx.send(initial.clone());
            let mut last = initial;

            loop {
                let Some(sock_path) = find_hyprland_socket2() else {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                };

                match UnixStream::connect(&sock_path).await {
                    Ok(stream) => {
                        let mut reader = BufReader::new(stream).lines();
                        while let Ok(Some(line)) = reader.next_line().await {
                            if let Some(parsed) = parse_socket2_event_line(&line) {
                                if parsed.window_class != last.window_class
                                    || parsed.window_title != last.window_title
                                {
                                    last = parsed.clone();
                                    if tx.send(parsed).is_err() {
                                        return;
                                    }
                                }
                            }
                        }
                    }
                    Err(_) => {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        }

        #[cfg(not(target_os = "linux"))]
        {
            let mut last = query_active_window().await;
            let _ = tx.send(last.clone());
            loop {
                tokio::time::sleep(Duration::from_millis(150)).await;
                let cur = query_active_window().await;
                if cur.window_class != last.window_class || cur.window_title != last.window_title {
                    last = cur.clone();
                    if tx.send(cur).is_err() {
                        return;
                    }
                }
            }
        }
    });

    rx
}

// ---------------------------------------------------------------------------
// Cross-platform Open Windows List (for GUI window picker)
// ---------------------------------------------------------------------------

pub fn query_platform_open_windows() -> Vec<OpenWindowInfo> {
    let mut windows = Vec::new();

    #[cfg(target_os = "linux")]
    {
        if let Ok(output) = std::process::Command::new("hyprctl")
            .args(["clients", "-j"])
            .output()
        {
            if output.status.success() {
                if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&output.stdout) {
                    if let Some(list) = val.as_array() {
                        let mut seen = std::collections::HashSet::new();
                        for item in list {
                            let class = item
                                .get("class")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .trim()
                                .to_string();
                            let title = item
                                .get("title")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .trim()
                                .to_string();
                            let initial_class = item
                                .get("initialClass")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                            let initial_title = item
                                .get("initialTitle")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());

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
    }

    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM};
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            EnumWindows, GetClassNameW, GetWindowTextLengthW, GetWindowTextW, IsWindowVisible,
        };

        unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
            if IsWindowVisible(hwnd) == 0 {
                return 1;
            }
            let len = GetWindowTextLengthW(hwnd);
            if len <= 0 {
                return 1;
            }
            let mut title_buf = vec![0u16; (len as usize) + 2];
            let copied = GetWindowTextW(hwnd, title_buf.as_mut_ptr(), len + 1);
            let title = String::from_utf16_lossy(&title_buf[..copied as usize])
                .trim()
                .to_string();
            if title.is_empty() {
                return 1;
            }

            let mut class_buf = [0u16; 256];
            let class_len = GetClassNameW(hwnd, class_buf.as_mut_ptr(), 256);
            let class = String::from_utf16_lossy(&class_buf[..class_len as usize])
                .trim()
                .to_string();
            if class.is_empty() || class == "Progman" || class == "Windows.UI.Core.CoreWindow" {
                return 1;
            }

            let vec_ptr = lparam as *mut Vec<OpenWindowInfo>;
            (*vec_ptr).push(OpenWindowInfo {
                class: class.clone(),
                title: title.clone(),
                initial_class: Some(class),
                initial_title: Some(title),
            });
            1
        }

        unsafe {
            let mut raw_list: Vec<OpenWindowInfo> = Vec::new();
            EnumWindows(
                Some(enum_proc),
                &mut raw_list as *mut Vec<OpenWindowInfo> as LPARAM,
            );
            let mut seen = std::collections::HashSet::new();
            for item in raw_list {
                if seen.insert(item.class.clone()) {
                    windows.push(item);
                }
            }
        }
    }

    windows
}
