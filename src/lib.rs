//! 8BitDo Micro Focus-Switch
//!
//! Unified library containing:
//! - `core`: controller protocol, config, daemon, BLE GATT, input backends
//! - `cli`: terminal commands, argument parser, key recorder
//! - `gui`: desktop GUI, Tauri IPC commands, system tray

pub mod core;
pub mod cli;
pub mod gui;

// Re-export core modules for ergonomics and internal imports
pub use crate::core::ble;
pub use crate::core::config;
pub use crate::core::daemon;
pub use crate::core::input_backend;
pub use crate::core::keycodes;
pub use crate::core::protocol;
pub use crate::core::recorder;
pub use crate::core::visualizer;
pub use crate::core::window_watcher;

pub use crate::gui::run_gui;

// ---------------------------------------------------------------------------
// Golden tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::time::Duration;

    use crate::core::config::{load_config_from_str, parse_combo_buttons, DEFAULT_TOML_CONTENT};
    use crate::core::daemon::FocusSwitchDaemon;
    use crate::core::input_backend::MockVirtualKeyboard;
    use crate::core::protocol::*;
    use crate::core::recorder;
    use crate::core::visualizer;
    use crate::core::window_watcher::{parse_socket2_event_line, ActiveWindow};
    use crate::cli;

    const KEY_ESC: u16 = 1;
    const KEY_TAB: u16 = 15;
    const KEY_E: u16 = 18;
    const KEY_R: u16 = 19;
    const KEY_T: u16 = 20;
    const KEY_P: u16 = 25;
    const KEY_LEFTCTRL: u16 = 29;
    const KEY_A: u16 = 30;
    const KEY_D: u16 = 32;
    const KEY_F: u16 = 33;
    const KEY_G: u16 = 34;
    const KEY_H: u16 = 35;
    const KEY_J: u16 = 36;
    const KEY_K: u16 = 37;
    const KEY_L: u16 = 38;
    const KEY_LEFTSHIFT: u16 = 42;
    const KEY_M: u16 = 50;
    const KEY_DOT: u16 = 52;
    const KEY_LEFTALT: u16 = 56;
    const KEY_SPACE: u16 = 57;
    const KEY_PAGEUP: u16 = 104;
    const KEY_LEFT: u16 = 105;
    const KEY_RIGHT: u16 = 106;
    const KEY_LEFTMETA: u16 = 125;

    const SYN: (u16, u16, i32) = (EV_SYN, 0, 0);
    fn k(code: u16, v: i32) -> (u16, u16, i32) {
        (EV_KEY, code, v)
    }

    fn live_agy_record() -> Vec<u8> {
        let hex = format!(
            "{}{}{}{}{}{}{}",
            "d50800001109202011092020",
            "2a0000004e0000004b000000e1280000",
            "2b000000e0e10700e100000028000000",
            "0000000000000000290000001fe10000",
            "2c000000380000005200000051000000",
            "500000004f0000000000000000000000",
            "00".repeat(88)
        );
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }

    fn win(class: &str, title: &str) -> ActiveWindow {
        ActiveWindow {
            window_class: class.to_string(),
            window_title: title.to_string(),
            ..Default::default()
        }
    }

    fn test_daemon(toml: &str) -> FocusSwitchDaemon {
        let cfg = load_config_from_str(toml).unwrap();
        let mut d = FocusSwitchDaemon::new(cfg, None, Some("uinput".to_string()), None);
        d.keypress_event_path = None;
        d
    }

    fn combo(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn live_agy_golden_roundtrip() {
        let raw = live_agy_record();
        assert_eq!(raw.len(), RECORD_MICRO_SIZE);
        let decoded = MicroHardwareProfile::from_bytes(&raw).unwrap();
        assert!(decoded.enabled);
        assert_eq!(decoded.turn, 0);
        assert_eq!(decoded.turn_degrees(), 0);
        assert_eq!(decoded.header_prefix, [0xd5, 0x08, 0x00]);

        let agy = get_builtin_hardware_preset("agy").unwrap();
        assert_eq!(decoded.slots, agy.slots);
        assert_eq!(agy.to_bytes(Some(&raw)).unwrap(), raw);
    }

    #[test]
    fn live_chunk_crc16_matches_controller_notifications() {
        let raw = live_agy_record();
        let packets = build_write_config_packets(&raw).unwrap();
        assert_eq!(packets.len(), 4);
        let expected = [12210u16, 58714, 53040, 53040];
        for (idx, (offset, pkt)) in packets.iter().enumerate() {
            assert_eq!(*offset, idx * 45);
            assert_eq!(pkt[0], 0x04);
            let (hdr, payload) = parse_ble_notification(&pkt[1..]).unwrap();
            assert_eq!(hdr.chunk_len, 45);
            assert_eq!(hdr.total_len, 180);
            assert_eq!(hdr.offset as usize, idx * 45);
            assert_eq!(hdr.crc16, expected[idx]);
            assert_eq!(crc16_arc(&payload), expected[idx]);
        }
    }

    #[test]
    fn save_packet_framing() {
        let pkt = build_save_config_packet();
        assert_eq!(pkt.len(), 17);
        assert_eq!(pkt[0], 0x04);
        let (hdr, payload) = parse_ble_notification(&pkt[1..]).unwrap();
        assert_eq!(hdr.cmd, CMD_SAVE_CONFIG);
        assert_eq!(hdr.subcmd, CMD_SAVE_SUBCMD);
        assert_eq!(hdr.chunk_len, 0);
        assert_eq!(hdr.crc16, 0xFFFF);
        assert_eq!(hdr.total_len, 0);
        assert_eq!(hdr.offset, 0);
        assert!(payload.is_empty());
    }

    #[test]
    fn dpad_rotation() {
        assert_eq!(rotate_dpad_button("UP", 0).unwrap(), "UP");
        assert_eq!(rotate_dpad_button("UP", 90).unwrap(), "RIGHT");
        assert_eq!(rotate_dpad_button("UP", 180).unwrap(), "DOWN");
        assert_eq!(rotate_dpad_button("UP", 270).unwrap(), "LEFT");
        assert_eq!(rotate_dpad_button("A", 90).unwrap(), "A");
    }

    #[test]
    fn hyprland_socket2_parser() {
        assert!(parse_socket2_event_line("workspace>>2\n").is_none());
        let w = parse_socket2_event_line("activewindow>>anki,User 1 - Anki\n").unwrap();
        assert_eq!(w.window_class, "anki");
        assert_eq!(w.window_title, "User 1 - Anki");
        let e = parse_socket2_event_line("activewindow>>,\n").unwrap();
        assert_eq!(e.window_class, "");
        assert_eq!(e.window_title, "");
    }

    #[test]
    fn daemon_focus_switch_and_chord_release_safety() {
        let mut d = test_daemon(DEFAULT_TOML_CONTENT);
        let mut ui = MockVirtualKeyboard::new();

        d.on_focus_change(win("anki", "Anki"));
        assert_eq!(d.active_profile.name, "anki");

        d.handle_key_event(&mut ui, KEY_H, 1);
        assert_eq!(
            ui.events,
            vec![k(KEY_LEFTCTRL, 1), k(KEY_LEFTSHIFT, 1), k(KEY_D, 1), SYN]
        );
        ui.events.clear();

        d.on_focus_change(win("firefox", "Mozilla Firefox"));
        assert_eq!(d.active_profile.name, "browser");

        d.handle_key_event(&mut ui, KEY_H, 0);
        assert_eq!(
            ui.events,
            vec![k(KEY_D, 0), k(KEY_LEFTSHIFT, 0), k(KEY_LEFTCTRL, 0), SYN]
        );
        ui.events.clear();

        d.handle_key_event(&mut ui, KEY_H, 1);
        d.handle_key_event(&mut ui, KEY_H, 0);
        assert_eq!(
            ui.events,
            vec![k(KEY_PAGEUP, 1), SYN, k(KEY_PAGEUP, 0), SYN]
        );
    }

    #[test]
    fn youtube_profile_matching_and_actions() {
        let cfg = load_config_from_str(DEFAULT_TOML_CONTENT).unwrap();
        assert_eq!(
            cfg.resolve_profile_for_window("firefox", "Lofi Hip Hop - YouTube").name,
            "youtube"
        );
        assert_eq!(
            cfg.resolve_profile_for_window("firefox", "GitHub - Issues").name,
            "browser"
        );
        assert_eq!(
            cfg.resolve_profile_for_window(
                "chrome-youtube.com__-Profile_3",
                "(729) Matt Pocock - YouTube"
            )
            .name,
            "youtube"
        );
        assert_eq!(
            cfg.resolve_profile_for_window("chrome-youtube.com__-Profile_3", "Arbitrary Video Title")
                .name,
            "youtube"
        );
        assert_eq!(
            cfg.resolve_profile_for_window("freetube", "Playing video").name,
            "youtube"
        );

        let mut d = test_daemon(DEFAULT_TOML_CONTENT);
        let mut ui = MockVirtualKeyboard::new();
        d.on_focus_change(win("firefox", "Some Video - YouTube"));
        assert_eq!(d.active_profile.name, "youtube");

        d.handle_key_event(&mut ui, KEY_G, 1);
        d.handle_key_event(&mut ui, KEY_G, 0);
        assert!(ui.events.contains(&k(KEY_SPACE, 1)));
        ui.events.clear();

        d.handle_key_event(&mut ui, KEY_K, 1);
        d.handle_key_event(&mut ui, KEY_K, 0);
        assert!(ui.events.contains(&k(KEY_J, 1)));
        ui.events.clear();

        d.handle_key_event(&mut ui, KEY_R, 1);
        assert_eq!(ui.events, vec![k(KEY_LEFTSHIFT, 1), k(KEY_DOT, 1), SYN]);
        ui.events.clear();
        d.handle_key_event(&mut ui, KEY_R, 0);
        assert_eq!(ui.events, vec![k(KEY_DOT, 0), k(KEY_LEFTSHIFT, 0), SYN]);

        let yt = get_builtin_hardware_preset("youtube").unwrap();
        let raw = yt.to_bytes(None).unwrap();
        assert_eq!(raw.len(), RECORD_MICRO_SIZE);
        assert!(MicroHardwareProfile::from_bytes(&raw).unwrap().enabled);
    }

    #[test]
    fn visualizer_svg_and_html_diagrams() {
        let cfg = load_config_from_str(DEFAULT_TOML_CONTENT).unwrap();
        let agy = cfg.get_profile("agy").unwrap();
        let svg = visualizer::generate_svg_diagram(&agy);
        assert!(svg.trim_start().starts_with("<svg") || svg.contains("<svg"));
        assert!(svg.contains("8BitDo Micro Keymap — AGY"));
        assert!(svg.contains("Backspace"));
        assert!(svg.contains("Ctrl + Shift + D"));
        assert!(svg.contains("Pageup"));
        assert!(svg.contains("COMBOS"));
        assert!(svg.contains("L1 + R1"));
        assert!(svg.contains("Super + K"));

        let yt = cfg.get_profile("youtube").unwrap();
        let svg_yt = visualizer::generate_svg_diagram(&yt);
        assert!(svg_yt.contains("8BitDo Micro Keymap — YOUTUBE"));
        assert!(svg_yt.contains("Shift + Dot"));
        assert!(svg_yt.contains("Shift + Comma"));
        for b in PHYSICAL_BUTTONS {
            assert!(svg.contains(b));
        }

        let html = visualizer::generate_html_viewer(&cfg.profiles, &cfg.profile_order, "agy");
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("selectProfile"));
        assert!(html.contains("class=\"tab-btn active\""));
        for name in &cfg.profile_order {
            assert!(html.contains(&format!("data-profile=\"{}\"", name)));
            assert!(html.contains(&format!("id=\"profile-{}\"", name)));
        }
    }

    #[test]
    fn combo_parsing_and_normalization() {
        assert_eq!(parse_combo_buttons("L+R").unwrap(), combo(&["L", "R"]));
        assert_eq!(parse_combo_buttons("L1 + R1").unwrap(), combo(&["L", "R"]));
        assert_eq!(parse_combo_buttons("R1+L1").unwrap(), combo(&["L", "R"]));
        assert_eq!(
            parse_combo_buttons("SELECT + START").unwrap(),
            combo(&["SELECT", "START"])
        );
        assert_eq!(
            parse_combo_buttons("MINUS + PLUS").unwrap(),
            combo(&["SELECT", "START"])
        );
        let err = parse_combo_buttons("L").unwrap_err();
        assert!(err.contains("at least 2 simultaneous buttons"));
    }

    #[test]
    fn daemon_combos_and_chording_mechanics() {
        let cfg = load_config_from_str(DEFAULT_TOML_CONTENT).unwrap();
        let agy = cfg.get_profile("agy").unwrap();
        let act = agy.combos.get(&combo(&["L", "R"])).unwrap();
        assert_eq!(act.keys, vec!["LEFTMETA".to_string(), "K".to_string()]);

        let mut d = test_daemon(DEFAULT_TOML_CONTENT);
        let mut ui = MockVirtualKeyboard::new();
        d.on_focus_change(win("ghostty", "agy terminal"));
        assert_eq!(d.active_profile.name, "agy");

        // 1. Non-combo button (STAR -> SPACE) emits immediately
        d.handle_key_event_with_mode(&mut ui, KEY_T, 1, true);
        assert_eq!(ui.events, vec![k(KEY_SPACE, 1), SYN]);
        ui.events.clear();
        d.handle_key_event_with_mode(&mut ui, KEY_T, 0, true);
        assert_eq!(ui.events, vec![k(KEY_SPACE, 0), SYN]);
        ui.events.clear();

        // 2. Simultaneous combo L + R within timeout
        d.handle_key_event_with_mode(&mut ui, KEY_K, 1, true);
        assert!(ui.events.is_empty());
        std::thread::sleep(Duration::from_millis(15));
        d.handle_key_event_with_mode(&mut ui, KEY_M, 1, true);
        assert_eq!(ui.events, vec![k(KEY_LEFTMETA, 1), k(KEY_K, 1), SYN]);
        ui.events.clear();

        d.handle_key_event_with_mode(&mut ui, KEY_M, 2, true);
        assert_eq!(ui.events, vec![k(KEY_K, 2), SYN]);
        ui.events.clear();

        d.handle_key_event_with_mode(&mut ui, KEY_K, 0, true);
        assert_eq!(ui.events, vec![k(KEY_K, 0), k(KEY_LEFTMETA, 0), SYN]);
        ui.events.clear();

        d.handle_key_event_with_mode(&mut ui, KEY_M, 0, true);
        assert!(ui.events.is_empty());

        // 3. Solo hold -> timeout flush
        d.handle_key_event_with_mode(&mut ui, KEY_K, 1, true);
        assert!(ui.events.is_empty());
        std::thread::sleep(Duration::from_millis(65));
        d.flush_expired_pending_buttons(&mut ui);
        assert_eq!(ui.events, vec![k(KEY_TAB, 1), SYN]);
        ui.events.clear();
        d.handle_key_event_with_mode(&mut ui, KEY_K, 0, true);
        assert_eq!(ui.events, vec![k(KEY_TAB, 0), SYN]);
        ui.events.clear();

        // 4. Fast tap
        d.handle_key_event_with_mode(&mut ui, KEY_K, 1, true);
        assert!(ui.events.is_empty());
        std::thread::sleep(Duration::from_millis(10));
        d.handle_key_event_with_mode(&mut ui, KEY_K, 0, true);
        assert_eq!(ui.events, vec![k(KEY_TAB, 1), SYN, k(KEY_TAB, 0), SYN]);
    }

    #[test]
    fn daemon_hold_and_press_combos() {
        let toml = r#"
[general]
backend = "uinput"
fallback_profile = "work"
combo_timeout_ms = 40

[profiles.global.combos]
"L2+LEFT" = ["LEFTMETA", "LEFT"]
"L2+RIGHT" = ["LEFTMETA", "RIGHT"]

[profiles.work]
description = "Work Profile"
turn = 0
[profiles.work.buttons]
L2 = "L"
LEFT = "Left"
RIGHT = "Right"
"#;
        let cfg = load_config_from_str(toml).unwrap();
        let prof = cfg.get_profile("work").unwrap();
        assert!(prof.combos.contains_key(&combo(&["L2", "LEFT"])));
        assert!(prof.combos.contains_key(&combo(&["L2", "RIGHT"])));

        let mut d = test_daemon(toml);
        let mut ui = MockVirtualKeyboard::new();
        d.on_focus_change(win("work", "work"));

        d.handle_key_event_with_mode(&mut ui, KEY_L, 1, true);
        assert!(ui.events.is_empty());
        std::thread::sleep(Duration::from_millis(55));
        d.flush_expired_pending_buttons(&mut ui);
        assert_eq!(ui.events, vec![k(KEY_L, 1), SYN]);
        ui.events.clear();

        d.handle_key_event_with_mode(&mut ui, KEY_E, 1, true);
        assert_eq!(
            ui.events,
            vec![k(KEY_L, 0), SYN, k(KEY_LEFTMETA, 1), k(KEY_LEFT, 1), SYN]
        );
        ui.events.clear();

        d.handle_key_event_with_mode(&mut ui, KEY_E, 0, true);
        assert_eq!(ui.events, vec![k(KEY_LEFT, 0), k(KEY_LEFTMETA, 0), SYN]);
        ui.events.clear();

        d.handle_key_event_with_mode(&mut ui, KEY_F, 1, true);
        assert_eq!(ui.events, vec![k(KEY_LEFTMETA, 1), k(KEY_RIGHT, 1), SYN]);
        ui.events.clear();

        d.handle_key_event_with_mode(&mut ui, KEY_F, 0, true);
        assert_eq!(ui.events, vec![k(KEY_RIGHT, 0), k(KEY_LEFTMETA, 0), SYN]);
        ui.events.clear();

        d.handle_key_event_with_mode(&mut ui, KEY_L, 0, true);
        assert!(ui.events.is_empty());
    }

    #[test]
    fn daemon_in_process_reload() {
        let dir = std::env::temp_dir().join(format!("8bitdo-test-reload-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let cfg_file = dir.join("profiles.toml");
        let v1 = "[general]\nbackend = \"uinput\"\nfallback_profile = \"test\"\n\n[profiles.test]\ndescription = \"Test v1\"\nturn = 0\n[profiles.test.buttons]\nA = \"Space\"\n";
        fs::write(&cfg_file, v1).unwrap();
        let cfg1 = crate::core::config::load_config(Some(&cfg_file)).unwrap();
        let mut d = FocusSwitchDaemon::new(cfg1, Some(cfg_file.clone()), Some("uinput".into()), None);
        d.keypress_event_path = None;
        assert_eq!(d.active_profile.turn, 0);

        let v2 = "[general]\nbackend = \"uinput\"\nfallback_profile = \"test\"\n\n[profiles.test]\ndescription = \"Test v2\"\nturn = 90\n[profiles.test.buttons]\nA = \"Enter\"\n";
        fs::write(&cfg_file, v2).unwrap();
        d.reload_config();
        assert_eq!(d.active_profile.turn, 90);
        assert_eq!(
            d.config.profiles["test"].buttons["A"].keys,
            vec!["ENTER".to_string()]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn chord_recorder_state_machine() {
        use crate::core::recorder::ChordRecorder;

        let mut r1 = ChordRecorder::new();
        assert!(r1.process_event(KEY_A, 2).is_none());
        let res1 = r1.process_event(KEY_SPACE, 1).unwrap();
        assert_eq!(res1.keys, vec!["Space"]);
        assert!(!res1.cancelled);

        let mut r2 = ChordRecorder::new();
        assert!(r2.process_event(KEY_LEFTCTRL, 1).is_none());
        assert!(r2.process_event(KEY_LEFTSHIFT, 1).is_none());
        let res2 = r2.process_event(KEY_P, 1).unwrap();
        assert_eq!(res2.keys, vec!["Ctrl", "Shift", "P"]);

        let mut r3 = ChordRecorder::new();
        assert!(r3.process_event(KEY_LEFTMETA, 1).is_none());
        let res3 = r3.process_event(KEY_LEFTMETA, 0).unwrap();
        assert_eq!(res3.keys, vec!["Super"]);

        let mut r4 = ChordRecorder::new();
        assert!(r4.process_event(KEY_LEFTALT, 1).is_none());
        let res4 = r4.process_event(KEY_ESC, 1).unwrap();
        assert!(res4.cancelled);
    }

    #[test]
    fn keys_catalog_formatting() {
        let cat = recorder::get_bindable_keys_catalog();
        assert!(cat.iter().any(|(n, _)| *n == "Modifiers"));
        let media = cat.iter().find(|(n, _)| *n == "Media & Volume").unwrap();
        assert!(media.1.contains(&"VolumeUp"));

        let all = recorder::format_keys_catalog(None);
        assert!(all.contains("Bindable Keys Catalog"));
        assert!(all.contains("PlayPause"));

        let filtered = recorder::format_keys_catalog(Some("volume"));
        assert!(filtered.contains("VolumeUp"));
        assert!(filtered.contains("PlayPause"));
        assert!(!filtered.contains("Digits"));
    }

    #[test]
    fn bind_writes_buttons_and_combos() {
        let dir = std::env::temp_dir().join(format!("8bitdo-test-bind-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let cfg_file = dir.join("profiles.toml");
        fs::write(&cfg_file, DEFAULT_TOML_CONTENT).unwrap();

        recorder::set_profile_binding(&cfg_file, "agy", "A", &["Space".to_string()], None).unwrap();
        let c = crate::core::config::load_config(Some(&cfg_file)).unwrap();
        assert_eq!(c.profiles["agy"].buttons["A"].keys, vec!["SPACE".to_string()]);

        recorder::set_profile_binding(
            &cfg_file,
            "agy",
            "L+R",
            &["Super".to_string(), "K".to_string()],
            None,
        )
        .unwrap();
        let c = crate::core::config::load_config(Some(&cfg_file)).unwrap();
        assert_eq!(
            c.profiles["agy"].combos[&combo(&["L", "R"])].keys,
            vec!["SUPER".to_string(), "K".to_string()]
        );

        recorder::set_profile_binding(
            &cfg_file,
            "agy",
            "Y",
            &["Ctrl".to_string(), "Shift".to_string(), "P".to_string()],
            None,
        )
        .unwrap();
        let c = crate::core::config::load_config(Some(&cfg_file)).unwrap();
        assert_eq!(
            c.profiles["agy"].buttons["Y"].keys,
            vec!["CTRL".to_string(), "SHIFT".to_string(), "P".to_string()]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cli_config_helpers() {
        let dir = std::env::temp_dir().join(format!("8bitdo-test-cli-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let cfg_file = dir.join("profiles.toml");
        fs::write(&cfg_file, DEFAULT_TOML_CONTENT).unwrap();

        assert!(cli::find_profile_line_number(&cfg_file, "youtube").is_some());
        assert!(cli::find_profile_line_number(&cfg_file, "nonexistent").is_none());

        let cmd = cli::build_editor_command("code", &cfg_file, Some(12));
        assert_eq!(cmd[1], "-g");
        assert!(cmd[2].ends_with(":12"));
        let cmd = cli::build_editor_command("nvim", &cfg_file, Some(12));
        assert_eq!(cmd[1], "+12");

        let table = cli::format_profile_table(&cfg_file).unwrap();
        assert!(table.contains("Available Profiles:"));
        assert!(table.contains("youtube"));

        // `config discord --yes --no-edit` creates a new valid profile
        let code = cli::run_cli_from_args(
            [
                "8bitdo-micro-focus-switch",
                "--config",
                cfg_file.to_str().unwrap(),
                "config",
                "discord",
                "--yes",
                "--no-edit",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        );
        assert_eq!(code, 0);
        let c = crate::core::config::load_config(Some(&cfg_file)).unwrap();
        assert!(c.profiles.contains_key("discord"));
        assert_eq!(c.profiles["discord"].match_class.as_deref(), Some("discord"));
        assert_eq!(c.profiles["discord"].buttons["A"].keys, vec!["SPACE".to_string()]);

        // `show agy -o out.svg`
        let out_svg = dir.join("out.svg");
        let code = cli::run_cli_from_args(
            [
                "8bitdo-micro-focus-switch",
                "--config",
                cfg_file.to_str().unwrap(),
                "show",
                "agy",
                "-o",
                out_svg.to_str().unwrap(),
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        );
        assert_eq!(code, 0);
        assert!(fs::read_to_string(&out_svg).unwrap().contains("<svg"));

        // Broken config is refused by `reload`
        let broken = dir.join("broken.toml");
        fs::write(&broken, "this is invalid toml ::: [[[").unwrap();
        let code = cli::run_cli_from_args(
            [
                "8bitdo-micro-focus-switch",
                "--config",
                broken.to_str().unwrap(),
                "reload",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        );
        assert_eq!(code, 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_windows_hook_evaluation() {
        use crate::core::input_backend::{
            evaluate_win32_hook_event, evaluate_win32_hook_event_ex, MAGIC_EXTRA_INFO,
            LLKHF_INJECTED, WM_KEYDOWN, WM_KEYUP,
        };
        use std::collections::HashSet;

        let mut held = HashSet::new();

        // 1. Sentinel Key (F13 -> "A" -> factory evdev 34)
        // F13 has scancode 0x64
        let ev = evaluate_win32_hook_event(0, 0x64, 0, 0, WM_KEYDOWN, &mut held);
        assert_eq!(ev, Some((34, 1)));
        assert!(held.contains("A"));

        // Repeat event
        let ev_rep = evaluate_win32_hook_event(0, 0x64, 0, 0, WM_KEYDOWN, &mut held);
        assert_eq!(ev_rep, Some((34, 2)));

        // Key up
        let ev_up = evaluate_win32_hook_event(0, 0x64, 0, 0, WM_KEYUP, &mut held);
        assert_eq!(ev_up, Some((34, 0)));
        assert!(!held.contains("A"));

        // 2. Factory Key (G scancode 0x22 -> "A" -> factory evdev 34)
        // With allow_factory = true
        let ev_fac = evaluate_win32_hook_event_ex(0, 0x22, 0, 0, WM_KEYDOWN, &mut held, true);
        assert_eq!(ev_fac, Some((34, 1)));
        assert!(held.contains("A"));

        // With allow_factory = false
        held.clear();
        let ev_fac_dis = evaluate_win32_hook_event_ex(0, 0x22, 0, 0, WM_KEYDOWN, &mut held, false);
        assert_eq!(ev_fac_dis, None);

        // Default evaluate_win32_hook_event should never intercept factory key G
        let ev_fac_default = evaluate_win32_hook_event(0, 0x22, 0, 0, WM_KEYDOWN, &mut held);
        assert_eq!(ev_fac_default, None);

        // 3. Ignored Injected Keys (anti feedback loop)
        let ev_inj = evaluate_win32_hook_event(0, 0x64, LLKHF_INJECTED, 0, WM_KEYDOWN, &mut held);
        assert_eq!(ev_inj, None);

        let ev_magic = evaluate_win32_hook_event(0, 0x64, 0, MAGIC_EXTRA_INFO, WM_KEYDOWN, &mut held);
        assert_eq!(ev_magic, None);
    }

    #[test]
    fn test_sentinel_evdev_resolution_and_daemon_support() {
        use crate::core::protocol::evdev_to_button;
        use crate::core::config::load_config_from_str;
        use crate::core::daemon::FocusSwitchDaemon;

        // Verify evdev_to_button resolves both factory and sentinel keys
        assert_eq!(evdev_to_button(34), Some("A")); // Factory G -> A
        assert_eq!(evdev_to_button(183), Some("A")); // Sentinel F13 -> A
        assert_eq!(evdev_to_button(184), Some("B")); // Sentinel F14 -> B
        assert_eq!(evdev_to_button(98), Some("SELECT")); // Sentinel KPSLASH -> SELECT
        assert_eq!(evdev_to_button(78), Some("HOME")); // Sentinel KPPLUS -> HOME

        // Test daemon remapping Sentinel F13 keycode
        let toml_str = r#"
[general]
fallback_profile = "test"
backend = "uinput"

[profiles.test]
description = "Test Sentinel Support"
buttons.A = "SPACE"
"#;
        let cfg = load_config_from_str(toml_str).unwrap();
        let mut d = FocusSwitchDaemon::new(cfg, None, None, None);
        struct MockUi(Vec<(u16, u16, i32)>);
        impl crate::core::input_backend::VirtualKeyboard for MockUi {
            fn write(&mut self, etype: u16, code: u16, val: i32) {
                self.0.push((etype, code, val));
            }
            fn syn(&mut self) {}
            fn close(&mut self) {}
        }

        let mut ui = MockUi(Vec::new());
        // Press button A via Sentinel keycode 183 (F13)
        d.handle_key_event(&mut ui, 183, 1);
        // Expect SPACE (evdev 57) down
        assert!(ui.0.iter().any(|&(_, c, v)| c == 57 && v == 1));

        // Release button A via Sentinel keycode 183
        d.handle_key_event(&mut ui, 183, 0);
        // Expect SPACE (evdev 57) up
        assert!(ui.0.iter().any(|&(_, c, v)| c == 57 && v == 0));
    }

    #[test]
    fn test_keypress_event_security_permissions_and_payload() {
        let temp_dir = std::env::temp_dir().join(format!("8bitdo-sec-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        let event_file = temp_dir.join("8bitdo-micro").join("last_keypress.json");

        let toml_str = r#"
[general]
fallback_profile = "test"
backend = "uinput"
emit_osd_events = true

[profiles.test]
description = "Test Security"
buttons.A = "SPACE"
"#;
        let cfg = load_config_from_str(toml_str).unwrap();
        let mut d = FocusSwitchDaemon::new(cfg, None, None, None);
        d.keypress_event_path = Some(event_file.clone());

        // Simulate a pre-upgrade world-readable (0644) event file that another local user
        // already opened a read descriptor to before the daemon upgraded/started.
        fs::create_dir_all(event_file.parent().unwrap()).unwrap();
        fs::write(&event_file, "{\"seq\": 0, \"legacy\": true}\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&event_file, fs::Permissions::from_mode(0o644)).unwrap();
        }
        let mut pre_upgrade_reader = fs::File::open(&event_file).unwrap();

        d.ensure_keypress_event_file();

        struct MockUi;
        impl crate::core::input_backend::VirtualKeyboard for MockUi {
            fn write(&mut self, _etype: u16, _code: u16, _val: i32) {}
            fn syn(&mut self) {}
            fn close(&mut self) {}
        }
        let mut ui = MockUi;
        // Press button A (Factory G is evdev 34)
        d.handle_key_event(&mut ui, 34, 1);

        assert!(event_file.exists());
        let content = fs::read_to_string(&event_file).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();

        assert_eq!(parsed["button"], "A");
        assert_eq!(parsed["key"], "SPACE");
        assert!(parsed.get("timestamp").is_none(), "timestamp must be omitted for privacy");

        // Verify the pre-upgrade open file descriptor was severed from the new inode
        // and cannot read any new mapped key/profile events.
        #[cfg(unix)]
        {
            use std::io::{Read, Seek, SeekFrom};
            use std::os::unix::fs::{MetadataExt, PermissionsExt};

            pre_upgrade_reader.seek(SeekFrom::Start(0)).unwrap();
            let mut old_fd_contents = String::new();
            pre_upgrade_reader.read_to_string(&mut old_fd_contents).unwrap();
            assert!(
                !old_fd_contents.contains("SPACE"),
                "Pre-upgrade open file descriptor must not receive new keypress events"
            );

            let old_ino = pre_upgrade_reader.metadata().unwrap().ino();
            let file_meta = fs::metadata(&event_file).unwrap();
            assert_ne!(
                old_ino,
                file_meta.ino(),
                "Event file must use a newly created inode rather than reusing the old inode"
            );

            let file_mode = file_meta.permissions().mode() & 0o777;
            assert_eq!(file_mode, 0o600, "Event file must have mode 0600");

            let parent_meta = fs::metadata(event_file.parent().unwrap()).unwrap();
            let parent_mode = parent_meta.permissions().mode() & 0o777;
            assert_eq!(parent_mode, 0o700, "Parent directory must have mode 0700");
        }

        // Test emit_osd_events = false
        d.config.emit_osd_events = false;
        let _ = fs::remove_file(&event_file);
        d.handle_key_event(&mut ui, 34, 1);
        assert!(!event_file.exists(), "Event file must not be written when emit_osd_events is false");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_udev_rule_seat_scoped_uaccess_and_no_world_writable_mode() {
        assert!(!cli::UDEV_RULE_CONTENT.contains("0666"));
        assert!(!cli::UDEV_RULE_CONTENT.contains("8BitDo*"));
        assert!(cli::UDEV_RULE_CONTENT.contains("MODE=\"0660\""));
        assert!(cli::UDEV_RULE_CONTENT.contains("GROUP=\"input\""));
        assert!(cli::UDEV_RULE_CONTENT.contains("TAG+=\"uaccess\""));
        assert!(cli::UDEV_RULE_CONTENT.contains("ATTRS{id/vendor}==\"2dc8\""));
        assert!(cli::is_udev_rule_secure_and_current(cli::UDEV_RULE_CONTENT));

        let legacy_rule = "KERNEL==\"uinput\", MODE=\"0666\", GROUP=\"input\", OPTIONS+=\"static_node=uinput\"\n";
        assert!(!cli::is_udev_rule_secure_and_current(legacy_rule));

        let missing_group_rule = "KERNEL==\"uinput\", MODE=\"0660\", TAG+=\"uaccess\", ATTRS{id/vendor}==\"2dc8\"\n";
        assert!(!cli::is_udev_rule_secure_and_current(missing_group_rule));

        let wildcard_name_rule = "KERNEL==\"uinput\", GROUP=\"input\", MODE=\"0660\", TAG+=\"uaccess\"\nATTRS{name}==\"8BitDo*\", MODE=\"0660\", TAG+=\"uaccess\"\n";
        assert!(!cli::is_udev_rule_secure_and_current(wildcard_name_rule));
    }

    #[test]
    fn test_toml_binding_sanitizes_newlines_and_blocks_line_injection() {
        let injected_desc = " innocent comment\nB = \"exec: TouchPwned\"\n# trailing";
        let updated = recorder::update_toml_binding(
            DEFAULT_TOML_CONTENT,
            "agy",
            "A",
            &["Space".to_string()],
            Some(injected_desc),
        )
        .unwrap();

        let cfg = load_config_from_str(&updated).unwrap();
        let b_act = cfg.profiles["agy"].buttons.get("B").unwrap();
        assert!(
            b_act.exec_cmd.is_none(),
            "Newline in description must not inject a new TOML key/exec binding"
        );
        assert!(
            recorder::update_toml_binding(
                DEFAULT_TOML_CONTENT,
                "agy]\n[profiles.evil",
                "A",
                &["Space".to_string()],
                None,
            )
            .is_err()
        );
    }

    #[test]
    fn test_write_private_atomic_enforces_0600_and_0700() {
        let temp_dir = std::env::temp_dir().join(format!("8bitdo-atomic-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        let cfg_path = temp_dir.join("8bitdo-micro").join("profiles.toml");

        crate::core::config::write_private_atomic(&cfg_path, DEFAULT_TOML_CONTENT.as_bytes()).unwrap();
        assert!(cfg_path.exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let file_mode = fs::metadata(&cfg_path).unwrap().permissions().mode() & 0o777;
            assert_eq!(file_mode, 0o600);
            let dir_mode = fs::metadata(cfg_path.parent().unwrap()).unwrap().permissions().mode() & 0o777;
            assert_eq!(dir_mode, 0o700);
        }

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_ble_name_matching_excludes_classic_bt_hid_and_baselines_do_not_collide() {
        assert!(crate::core::ble::is_micro_ble_name("80EL"));
        assert!(crate::core::ble::is_micro_ble_name("80el"));
        assert!(!crate::core::ble::is_micro_ble_name("8BitDo Micro gamepad"));
        assert!(!crate::core::ble::is_micro_ble_name("8BitDo Zero 2 gamepad"));

        // Verify sentinel and factory 16-button baselines have 32 distinct non-overlapping evdev codes
        let mut seen_codes = std::collections::HashSet::new();
        for &(btn, key_name) in crate::keycodes::SENTINEL_BUTTON_MAP {
            let code = crate::core::protocol::key_name_to_evdev(key_name).unwrap();
            assert!(seen_codes.insert(code), "Duplicate evdev code in sentinel baseline");
            assert_eq!(crate::core::protocol::evdev_to_button(code), Some(btn));
        }
        for &(btn, _) in crate::keycodes::SENTINEL_BUTTON_MAP {
            let fcode = crate::core::protocol::button_to_factory_evdev(btn).unwrap();
            assert!(seen_codes.insert(fcode), "Factory code collided with sentinel baseline");
            assert_eq!(crate::core::protocol::evdev_to_button(fcode), Some(btn));
        }
        assert_eq!(seen_codes.len(), 32);
    }
}
