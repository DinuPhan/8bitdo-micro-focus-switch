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
            assert!(html.contains(&format!("selectProfile('{}')", name)));
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
}
