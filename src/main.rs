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

fn main() {
    let args: Vec<String> = std::env::args().collect();

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
