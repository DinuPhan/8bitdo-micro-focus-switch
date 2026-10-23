// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// Decide whether this invocation should open the GUI (no args, or explicit `gui` / `--gui`)
/// or run as a terminal CLI.
fn wants_gui(args: &[String]) -> bool {
    let rest: Vec<&str> = args
        .iter()
        .skip(1)
        // Ignore platform launcher noise (e.g. macOS Finder `-psn_0_12345`)
        .filter(|a| !a.starts_with("-psn_"))
        .map(|s| s.as_str())
        .collect();
    rest.is_empty() || rest.iter().all(|a| matches!(*a, "gui" | "--gui"))
}

#[cfg(target_os = "windows")]
fn attach_parent_console() {
    use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    // Release builds use the GUI subsystem; re-attach to the launching terminal so CLI
    // subcommands can print to stdout/stderr.
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(target_os = "windows")]
fn setup_windows_file_logger() {
    use std::fs::OpenOptions;
    use std::os::windows::io::IntoRawHandle;
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::{
        GetStdHandle, SetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
    };

    let log_dir = eightbitdo_micro_focus_switch::core::config::get_home_dir()
        .join(".config")
        .join("8bitdo-micro");
    let _ = std::fs::create_dir_all(&log_dir);
    let log_path = log_dir.join("daemon.log");

    if let Ok(file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        let raw_handle = file.into_raw_handle();
        unsafe {
            let current_out = GetStdHandle(STD_OUTPUT_HANDLE);
            if current_out.is_null() || current_out == INVALID_HANDLE_VALUE {
                SetStdHandle(STD_OUTPUT_HANDLE, raw_handle as _);
            }
            let current_err = GetStdHandle(STD_ERROR_HANDLE);
            if current_err.is_null() || current_err == INVALID_HANDLE_VALUE {
                SetStdHandle(STD_ERROR_HANDLE, raw_handle as _);
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    #[cfg(target_os = "windows")]
    setup_windows_file_logger();

    if !wants_gui(&args) {
        #[cfg(target_os = "windows")]
        attach_parent_console();
        #[cfg(unix)]
        unsafe {
            libc::signal(libc::SIGPIPE, libc::SIG_DFL);
        }
        let code = eightbitdo_micro_focus_switch::cli::run_cli_from_args(args);
        std::process::exit(code);
    }

    #[cfg(target_os = "linux")]
    {
        // Fix WebKitGTK DMA-BUF compositing black box artifact on Linux Wayland/X11
        if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }

    eightbitdo_micro_focus_switch::gui::run_gui();
}
