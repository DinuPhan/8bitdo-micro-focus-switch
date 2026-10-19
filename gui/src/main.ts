import { invoke } from "@tauri-apps/api/core";
import { BUTTON_METADATA, KEY_CATEGORIES } from "./keyCatalog";
import controllerPng from "./assets/micro-controller.png";
import {
  AppConfigData,
  parseProfilesToml,
  serializeProfilesToml,
  ActionData,
} from "./tomlParser";

// Application State
let appConfig: AppConfigData = {
  general: { fallbackProfile: "default", backend: "uinput", comboTimeoutMs: 50 },
  profiles: {},
};
let activeProfileName = "default";
let editingTarget: { type: "button" | "combo"; key: string } | null = null;
let isRecording = false;
const currentlyPressedKeys = new Set<string>();

// DOM Elements
const profileSelect = document.getElementById("profile-select") as HTMLSelectElement;
const turnSelect = document.getElementById("turn-select") as HTMLSelectElement;
const btnNewProfile = document.getElementById("btn-new-profile") as HTMLButtonElement;
const btnDeleteProfile = document.getElementById("btn-delete-profile") as HTMLButtonElement;
const btnPickWindow = document.getElementById("btn-pick-window") as HTMLButtonElement;
const btnDaemonStatus = document.getElementById("btn-daemon-status") as HTMLButtonElement;
const btnSaveAll = document.getElementById("btn-save-all") as HTMLButtonElement;

// Canvas SVG Layers
const svgCalloutsLayer = document.getElementById("svg-callouts-layer") as unknown as SVGGElement;
const svgPinsLayer = document.getElementById("svg-pins-layer") as unknown as SVGGElement;

// Modals
const modalBinding = document.getElementById("modal-binding") as HTMLDivElement;
const modalBindingTitle = document.getElementById("modal-binding-title") as HTMLElement;
const inputBindingKeys = document.getElementById("input-binding-keys") as HTMLInputElement;
const inputBindingExec = document.getElementById("input-binding-exec") as HTMLInputElement;
const inputBindingDesc = document.getElementById("input-binding-desc") as HTMLInputElement;
const keyRecorderBox = document.getElementById("key-recorder-box") as HTMLDivElement;
const btnBindingApply = document.getElementById("btn-binding-apply") as HTMLButtonElement;
const btnBindingClear = document.getElementById("btn-binding-clear") as HTMLButtonElement;

// Window Picker Modal
const modalWindowPicker = document.getElementById("modal-window-picker") as HTMLDivElement;
const windowSearchInput = document.getElementById("window-search-input") as HTMLInputElement;
const openWindowsList = document.getElementById("open-windows-list") as HTMLDivElement;

// New Profile Modal
const modalNewProfile = document.getElementById("modal-new-profile") as HTMLDivElement;
const newProfileName = document.getElementById("new-profile-name") as HTMLInputElement;
const newProfileDesc = document.getElementById("new-profile-desc") as HTMLInputElement;
const newProfileClass = document.getElementById("new-profile-class") as HTMLInputElement;
const newProfileTitle = document.getElementById("new-profile-title") as HTMLInputElement;
const btnCreateProfileConfirm = document.getElementById("btn-create-profile-confirm") as HTMLButtonElement;

// Toast
const toast = document.getElementById("toast") as HTMLDivElement;

// Raw TOML
const rawTomlEditor = document.getElementById("raw-toml-editor") as HTMLTextAreaElement;
const configPathLabel = document.getElementById("config-path-label") as HTMLElement;
const btnFormatToml = document.getElementById("btn-format-toml") as HTMLButtonElement;

// Combos
const combosContainer = document.getElementById("combos-list-container") as HTMLDivElement;
const btnAddCombo = document.getElementById("btn-add-combo") as HTMLButtonElement;

// BLE
const btnFlashBle = document.getElementById("btn-flash-ble") as HTMLButtonElement;
const blePresetSelect = document.getElementById("ble-preset-select") as HTMLSelectElement;
const bleTurnSelect = document.getElementById("ble-turn-select") as HTMLSelectElement;
const bleOutputBox = document.getElementById("ble-output-box") as HTMLDivElement;

// ----------------------------------------------------------------------------
// Initialization
// ----------------------------------------------------------------------------
async function init() {
  setupTabs();
  setupCatalogChips();
  setupKeyRecorder();
  setupModalEvents();

  const controllerImgEl = document.getElementById("svg-controller-img") as SVGImageElement | null;
  if (controllerImgEl) {
    controllerImgEl.setAttribute("href", controllerPng);
  }

  try {
    const path = await invoke<string>("get_config_path");
    if (configPathLabel) configPathLabel.textContent = path;
  } catch (err) {
    console.error("Failed to get config path:", err);
  }

  await loadConfigFromDisk();
  await updateDaemonStatus();

  // Periodic daemon check every 4 seconds
  setInterval(updateDaemonStatus, 4000);
}

// ----------------------------------------------------------------------------
// Config Load & Save
// ----------------------------------------------------------------------------
async function loadConfigFromDisk() {
  try {
    const rawToml = await invoke<string>("read_config");
    appConfig = parseProfilesToml(rawToml);
    rawTomlEditor.value = rawToml;

    // Populate profile selector
    const profNames = Object.keys(appConfig.profiles);
    profileSelect.innerHTML = "";
    for (const name of profNames) {
      const opt = document.createElement("option");
      opt.value = name;
      opt.textContent = name === "global" ? "🌐 global (inherited)" : name;
      profileSelect.appendChild(opt);
    }

    if (profNames.includes(appConfig.general.fallbackProfile)) {
      activeProfileName = appConfig.general.fallbackProfile;
    } else if (profNames.length > 0) {
      activeProfileName = profNames[0];
    }

    profileSelect.value = activeProfileName;
    updateActiveProfileView();
  } catch (err) {
    showToast(`Error reading config: ${err}`);
  }
}

async function saveConfigToDisk() {
  try {
    const serialized = serializeProfilesToml(appConfig);
    await invoke("write_config", { content: serialized });
    rawTomlEditor.value = serialized;
    showToast("✓ Config saved & daemon reloaded!");
  } catch (err) {
    showToast(`Error saving config: ${err}`);
  }
}

// ----------------------------------------------------------------------------
// Daemon Management
// ----------------------------------------------------------------------------
async function updateDaemonStatus() {
  try {
    const status = await invoke<{ is_running: boolean; pid?: number }>("get_daemon_status");
    if (status.is_running) {
      btnDaemonStatus.className = "success";
      btnDaemonStatus.textContent = `🟢 Daemon (PID ${status.pid})`;
      btnDaemonStatus.title = "Daemon is active. Click to restart/reload.";
    } else {
      btnDaemonStatus.className = "danger";
      btnDaemonStatus.textContent = "🔴 Daemon Stopped";
      btnDaemonStatus.title = "Click to start background daemon.";
    }
  } catch (err) {
    console.debug("get_daemon_status error:", err);
  }
}

btnDaemonStatus.addEventListener("click", async () => {
  try {
    const status = await invoke<{ is_running: boolean }>("get_daemon_status");
    if (status.is_running) {
      await invoke("reload_daemon");
      showToast("✓ Daemon SIGHUP reloaded");
    } else {
      await invoke("toggle_daemon", { start: true });
      showToast("✓ Started daemon in background");
    }
    await updateDaemonStatus();
  } catch (err) {
    showToast(`Daemon control error: ${err}`);
  }
});

// ----------------------------------------------------------------------------
// Visual Mapper Rendering
// ----------------------------------------------------------------------------
function updateActiveProfileView() {
  const profile = appConfig.profiles[activeProfileName];
  if (!profile) return;

  turnSelect.value = String(profile.turn || 0);

  renderSvgCallouts();
  renderCombosList();
}

function renderSvgCallouts() {
  svgCalloutsLayer.innerHTML = "";
  svgPinsLayer.innerHTML = "";

  const profile = appConfig.profiles[activeProfileName];
  if (!profile) return;

  const globalProfile = appConfig.profiles["global"];

  for (const [btnKey, meta] of Object.entries(BUTTON_METADATA)) {
    // Resolve button action (profile action > global action)
    const action = profile.buttons[btnKey] || globalProfile?.buttons[btnKey];
    const isInherited = !profile.buttons[btnKey] && !!globalProfile?.buttons[btnKey];

    // 1. Draw glowing pin on controller image
    const [px, py] = meta.coord;
    const pin = document.createElementNS("http://www.w3.org/2000/svg", "circle");
    pin.setAttribute("cx", String(px));
    pin.setAttribute("cy", String(py));
    pin.setAttribute("r", "5");
    pin.setAttribute("fill", meta.color);
    pin.setAttribute("filter", "url(#glow)");
    pin.addEventListener("click", (e) => {
      e.stopPropagation();
      openBindingModal("button", btnKey);
    });
    svgPinsLayer.appendChild(pin);

    // 2. Draw leader polyline
    const [e1x, e1y] = meta.elbow1;
    const [e2x, e2y] = meta.elbow2;
    const [tx, ty] = meta.target;
    const polyline = document.createElementNS("http://www.w3.org/2000/svg", "polyline");
    polyline.setAttribute(
      "points",
      `${px},${py} ${e1x},${e1y} ${e2x},${e2y} ${tx},${ty}`
    );
    polyline.setAttribute("fill", "none");
    polyline.setAttribute("stroke", meta.color);
    polyline.setAttribute("stroke-width", "1.8");
    polyline.setAttribute("stroke-linecap", "round");
    polyline.setAttribute("stroke-linejoin", "round");
    polyline.setAttribute("opacity", "0.85");
    svgCalloutsLayer.appendChild(polyline);

    // 3. Draw Callout Card Group
    const cardGroup = document.createElementNS("http://www.w3.org/2000/svg", "g");
    cardGroup.classList.add("callout-card-group");
    const bx = meta.side === "left" ? tx - 250 : tx;
    const by = ty - 18;
    cardGroup.setAttribute("transform", `translate(${bx}, ${by})`);

    // Card background
    const cardBg = document.createElementNS("http://www.w3.org/2000/svg", "rect");
    cardBg.classList.add("card-bg");
    cardBg.setAttribute("width", "250");
    cardBg.setAttribute("height", "36");
    cardBg.setAttribute("rx", "8");
    cardBg.setAttribute("fill", "url(#card-grad)");
    cardBg.setAttribute("stroke", isInherited ? "#45475a" : "#313244");
    cardBg.setAttribute("stroke-width", "1.2");
    cardGroup.appendChild(cardBg);

    // Button badge
    const badgeBg = document.createElementNS("http://www.w3.org/2000/svg", "rect");
    badgeBg.setAttribute("x", "6");
    badgeBg.setAttribute("y", "6");
    badgeBg.setAttribute("width", "68");
    badgeBg.setAttribute("height", "24");
    badgeBg.setAttribute("rx", "5");
    badgeBg.setAttribute("fill", meta.color);
    badgeBg.setAttribute("fill-opacity", "0.18");
    badgeBg.setAttribute("stroke", meta.color);
    badgeBg.setAttribute("stroke-width", "1");
    cardGroup.appendChild(badgeBg);

    const badgeText = document.createElementNS("http://www.w3.org/2000/svg", "text");
    badgeText.setAttribute("x", "40");
    badgeText.setAttribute("y", "22");
    badgeText.setAttribute("text-anchor", "middle");
    badgeText.setAttribute("fill", meta.color);
    badgeText.setAttribute("font-size", "11");
    badgeText.setAttribute("font-weight", "bold");
    badgeText.textContent = meta.label;
    cardGroup.appendChild(badgeText);

    // Action text & description
    const actionStr = formatActionText(action);
    const hasDesc = !!(action && action.description);

    if (hasDesc && action?.description) {
      const txtMain = document.createElementNS("http://www.w3.org/2000/svg", "text");
      txtMain.setAttribute("x", "84");
      txtMain.setAttribute("y", "17");
      txtMain.setAttribute("fill", isInherited ? "#a6adc8" : "#cdd6f4");
      txtMain.setAttribute("font-size", "11");
      txtMain.setAttribute("font-weight", "bold");
      txtMain.textContent = truncate(actionStr, 20);
      cardGroup.appendChild(txtMain);

      const txtDesc = document.createElementNS("http://www.w3.org/2000/svg", "text");
      txtDesc.setAttribute("x", "84");
      txtDesc.setAttribute("y", "29");
      txtDesc.setAttribute("fill", "#6c7086");
      txtDesc.setAttribute("font-size", "9.5");
      txtDesc.textContent = truncate(action.description, 24);
      cardGroup.appendChild(txtDesc);
    } else {
      const txtMain = document.createElementNS("http://www.w3.org/2000/svg", "text");
      txtMain.setAttribute("x", "84");
      txtMain.setAttribute("y", "23");
      txtMain.setAttribute("fill", isInherited ? "#a6adc8" : "#cdd6f4");
      txtMain.setAttribute("font-size", "12");
      txtMain.setAttribute("font-weight", "bold");
      txtMain.textContent = truncate(actionStr, 22);
      cardGroup.appendChild(txtMain);
    }

    cardGroup.addEventListener("click", (e) => {
      e.stopPropagation();
      openBindingModal("button", btnKey);
    });
    svgCalloutsLayer.appendChild(cardGroup);
  }
}

// ----------------------------------------------------------------------------
// Combos List Rendering
// ----------------------------------------------------------------------------
function renderCombosList() {
  combosContainer.innerHTML = "";
  const profile = appConfig.profiles[activeProfileName];
  if (!profile) return;

  const combos = profile.combos;
  const comboKeys = Object.keys(combos);

  if (comboKeys.length === 0) {
    combosContainer.innerHTML = `
      <div style="grid-column: 1 / -1; padding: 20px; text-align: center; color: var(--text-muted); font-size: 13px;">
        No simultaneous combos defined for profile '${activeProfileName}'. Click <b>+ Add Combo</b> to create one!
      </div>
    `;
    return;
  }

  for (const comboKey of comboKeys) {
    const act = combos[comboKey];
    const card = document.createElement("div");
    card.className = "combo-card";

    const displayKey = comboKey
      .split("+")
      .map((k) => BUTTON_METADATA[k]?.label || k)
      .join(" + ");

    card.innerHTML = `
      <div>
        <div style="font-size: 13px; font-weight: 700; color: var(--accent-orange);">${displayKey}</div>
        <div style="font-size: 12px; color: var(--text-main); margin-top: 2px;">➔ ${formatActionText(act)}</div>
        ${act.description ? `<div style="font-size: 11px; color: var(--text-sub); margin-top: 2px;">• ${act.description}</div>` : ""}
      </div>
      <button class="danger btn-del-combo" style="padding: 4px 8px; font-size: 11px;">✕</button>
    `;

    card.querySelector(".btn-del-combo")?.addEventListener("click", (e) => {
      e.stopPropagation();
      delete profile.combos[comboKey];
      renderCombosList();
      showToast(`Removed combo ${comboKey}`);
    });

    card.addEventListener("click", () => openBindingModal("combo", comboKey));
    combosContainer.appendChild(card);
  }
}

// ----------------------------------------------------------------------------
// Modals & Key Recording
// ----------------------------------------------------------------------------
function openBindingModal(type: "button" | "combo", key: string) {
  editingTarget = { type, key };
  const profile = appConfig.profiles[activeProfileName];
  const action: ActionData | undefined =
    type === "button" ? profile?.buttons[key] : profile?.combos[key];

  const titlePrefix = type === "button" ? `Button [${key}]` : `Combo [${key}]`;
  modalBindingTitle.textContent = `Configure ${titlePrefix} in '${activeProfileName}'`;

  inputBindingKeys.value = action ? action.keys.join("+") : "";
  inputBindingExec.value = action?.execCmd || "";
  inputBindingDesc.value = action?.description || "";

  modalBinding.classList.add("open");
  inputBindingKeys.focus();
}

function closeAllModals() {
  document.querySelectorAll(".modal-backdrop").forEach((m) => m.classList.remove("open"));
  isRecording = false;
  currentlyPressedKeys.clear();
  keyRecorderBox.style.borderColor = "var(--border-focus)";
  keyRecorderBox.querySelector("span")!.textContent = "⌨️ Click here & press keys on your keyboard";
}

function setupModalEvents() {
  document.querySelectorAll(".modal-close-btn, .modal-cancel-btn").forEach((btn) => {
    btn.addEventListener("click", closeAllModals);
  });

  document.querySelectorAll(".modal-backdrop").forEach((backdrop) => {
    backdrop.addEventListener("click", (e) => {
      if (e.target === backdrop) closeAllModals();
    });
  });

  // Apply button binding
  btnBindingApply.addEventListener("click", () => {
    if (!editingTarget) return;
    const profile = appConfig.profiles[activeProfileName];
    if (!profile) return;

    const rawKeys = inputBindingKeys.value.trim();
    const execCmd = inputBindingExec.value.trim();
    const desc = inputBindingDesc.value.trim();

    const keysArray = rawKeys ? rawKeys.split("+").map((k) => k.trim().toUpperCase()) : [];
    const newAction: ActionData = {
      keys: keysArray,
      execCmd: execCmd || undefined,
      description: desc || undefined,
    };

    if (editingTarget.type === "button") {
      profile.buttons[editingTarget.key] = newAction;
      renderSvgCallouts();
    } else {
      profile.combos[editingTarget.key] = newAction;
      renderCombosList();
    }

    closeAllModals();
    showToast(`✓ Updated binding for ${editingTarget.key}`);
  });

  // Clear button binding
  btnBindingClear.addEventListener("click", () => {
    if (!editingTarget) return;
    const profile = appConfig.profiles[activeProfileName];
    if (!profile) return;

    if (editingTarget.type === "button") {
      delete profile.buttons[editingTarget.key];
      renderSvgCallouts();
    } else {
      delete profile.combos[editingTarget.key];
      renderCombosList();
    }

    closeAllModals();
    showToast(`Cleared ${editingTarget.key}`);
  });

  // Add Combo
  btnAddCombo.addEventListener("click", () => {
    const comboKey = prompt("Enter button combination (e.g. L+R, L2+UP, SELECT+START):");
    if (!comboKey) return;
    const norm = comboKey.trim().toUpperCase();
    openBindingModal("combo", norm);
  });
}

function setupKeyRecorder() {
  keyRecorderBox.addEventListener("click", () => {
    isRecording = true;
    currentlyPressedKeys.clear();
    keyRecorderBox.style.borderColor = "var(--accent-orange)";
    keyRecorderBox.querySelector("span")!.textContent = "🔴 Listening... Press keys on your keyboard";
  });

  window.addEventListener("keydown", (e) => {
    if (!isRecording) return;
    e.preventDefault();

    if (e.key === "Escape") {
      isRecording = false;
      keyRecorderBox.style.borderColor = "var(--border-focus)";
      keyRecorderBox.querySelector("span")!.textContent = "⌨️ Click here & press keys on your keyboard";
      return;
    }

    const keyName = translateDomKey(e.key);
    currentlyPressedKeys.add(keyName);

    // Build combination string
    const chord = Array.from(currentlyPressedKeys).join("+");
    inputBindingKeys.value = chord;
  });

  window.addEventListener("keyup", (e) => {
    if (!isRecording) return;
    currentlyPressedKeys.delete(translateDomKey(e.key));
    if (currentlyPressedKeys.size === 0) {
      isRecording = false;
      keyRecorderBox.style.borderColor = "var(--border-focus)";
      keyRecorderBox.querySelector("span")!.textContent = "✓ Captured! Click again to re-record";
    }
  });
}

function translateDomKey(key: string): string {
  const map: Record<string, string> = {
    Control: "LEFTCTRL",
    Shift: "LEFTSHIFT",
    Alt: "LEFTALT",
    Meta: "LEFTMETA",
    ArrowUp: "UP",
    ArrowDown: "DOWN",
    ArrowLeft: "LEFT",
    ArrowRight: "RIGHT",
    " ": "SPACE",
    Enter: "ENTER",
    Backspace: "BACKSPACE",
    Tab: "TAB",
    Delete: "DELETE",
    Escape: "ESC",
  };
  return map[key] || key.toUpperCase();
}

function setupCatalogChips() {
  const chipsMod = document.getElementById("chips-modifiers");
  const chipsCommon = document.getElementById("chips-common");
  const chipsFn = document.getElementById("chips-fn");

  function addChips(container: HTMLElement | null, keys: string[]) {
    if (!container) return;
    for (const k of keys) {
      const chip = document.createElement("span");
      chip.className = "key-chip";
      chip.textContent = k;
      chip.addEventListener("click", () => {
        const cur = inputBindingKeys.value.trim();
        inputBindingKeys.value = cur ? `${cur}+${k}` : k;
      });
      container.appendChild(chip);
    }
  }

  addChips(chipsMod, KEY_CATEGORIES["Modifiers"]);
  addChips(chipsCommon, [...KEY_CATEGORIES["Common"], ...KEY_CATEGORIES["Navigation"]]);
  addChips(chipsFn, KEY_CATEGORIES["Function"]);
}

// ----------------------------------------------------------------------------
// Window Picker Modal
// ----------------------------------------------------------------------------
btnPickWindow.addEventListener("click", async () => {
  modalWindowPicker.classList.add("open");
  openWindowsList.innerHTML = `<div style="text-align: center; color: var(--text-muted); padding: 20px;">Fetching open windows from OS...</div>`;

  try {
    const windows = await invoke<
      Array<{ class: string; title: string; initial_class?: string; initial_title?: string }>
    >("get_open_windows");

    renderOpenWindows(windows);

    windowSearchInput.value = "";
    windowSearchInput.oninput = () => {
      const q = windowSearchInput.value.toLowerCase();
      const filtered = windows.filter(
        (w) => w.class.toLowerCase().includes(q) || w.title.toLowerCase().includes(q)
      );
      renderOpenWindows(filtered);
    };
  } catch (err) {
    openWindowsList.innerHTML = `<div style="color: var(--accent-red); padding: 10px;">Error: ${err}</div>`;
  }
});

function renderOpenWindows(
  windows: Array<{ class: string; title: string; initial_class?: string; initial_title?: string }>
) {
  openWindowsList.innerHTML = "";
  if (windows.length === 0) {
    openWindowsList.innerHTML = `<div style="text-align: center; color: var(--text-muted); padding: 20px;">No matching open windows detected.</div>`;
    return;
  }

  for (const win of windows) {
    const item = document.createElement("div");
    item.style.cssText =
      "background-color: var(--bg-overlay); border: 1px solid var(--border); border-radius: 8px; padding: 10px 14px; cursor: pointer; transition: all 0.15s ease;";
    item.innerHTML = `
      <div style="font-weight: 700; color: var(--accent-blue);">${win.class}</div>
      <div style="font-size: 11px; color: var(--text-muted); margin-top: 2px;">${win.title || "No Title"}</div>
    `;

    item.addEventListener("mouseenter", () => {
      item.style.borderColor = "var(--border-focus)";
      item.style.backgroundColor = "var(--bg-card)";
    });
    item.addEventListener("mouseleave", () => {
      item.style.borderColor = "var(--border)";
      item.style.backgroundColor = "var(--bg-overlay)";
    });

    item.addEventListener("click", () => {
      const profile = appConfig.profiles[activeProfileName];
      if (profile) {
        profile.matchClass = `^(${win.class})$`;
        if (win.title) profile.matchTitle = win.title;
        showToast(`Matched '${activeProfileName}' to window class '${win.class}'`);
      }
      closeAllModals();
    });

    openWindowsList.appendChild(item);
  }
}

// ----------------------------------------------------------------------------
// Profile Management
// ----------------------------------------------------------------------------
btnNewProfile.addEventListener("click", () => {
  newProfileName.value = "";
  newProfileDesc.value = "";
  newProfileClass.value = "";
  newProfileTitle.value = "";
  modalNewProfile.classList.add("open");
  newProfileName.focus();
});

btnCreateProfileConfirm.addEventListener("click", () => {
  const name = newProfileName.value.trim().toLowerCase();
  if (!name) return;

  if (appConfig.profiles[name]) {
    alert(`Profile '${name}' already exists!`);
    return;
  }

  appConfig.profiles[name] = {
    name,
    description: newProfileDesc.value.trim() || undefined,
    matchClass: newProfileClass.value.trim() || undefined,
    matchTitle: newProfileTitle.value.trim() || undefined,
    turn: 0,
    buttons: {},
    combos: {},
  };

  const opt = document.createElement("option");
  opt.value = name;
  opt.textContent = name;
  profileSelect.appendChild(opt);

  activeProfileName = name;
  profileSelect.value = name;
  updateActiveProfileView();
  closeAllModals();
  showToast(`Created profile '${name}'`);
});

btnDeleteProfile.addEventListener("click", () => {
  if (activeProfileName === "default" || activeProfileName === "global") {
    alert("Cannot delete system default profiles.");
    return;
  }
  if (!confirm(`Are you sure you want to delete profile '${activeProfileName}'?`)) return;

  delete appConfig.profiles[activeProfileName];
  const opt = profileSelect.querySelector(`option[value="${activeProfileName}"]`);
  if (opt) opt.remove();

  activeProfileName = Object.keys(appConfig.profiles)[0] || "default";
  profileSelect.value = activeProfileName;
  updateActiveProfileView();
  showToast(`Deleted profile`);
});

profileSelect.addEventListener("change", () => {
  activeProfileName = profileSelect.value;
  updateActiveProfileView();
});

turnSelect.addEventListener("change", () => {
  const profile = appConfig.profiles[activeProfileName];
  if (profile) {
    profile.turn = Number(turnSelect.value) || 0;
  }
});

btnSaveAll.addEventListener("click", saveConfigToDisk);

// ----------------------------------------------------------------------------
// BLE Hardware Tab
// ----------------------------------------------------------------------------
btnFlashBle.addEventListener("click", async () => {
  const preset = blePresetSelect.value;
  const turnVal = bleTurnSelect.value ? Number(bleTurnSelect.value) : undefined;

  bleOutputBox.textContent = `Connecting to 8BitDo Micro over BLE GATT and flashing '${preset}'...`;
  btnFlashBle.disabled = true;

  try {
    const res = await invoke<string>("flash_preset", { preset, turn: turnVal });
    bleOutputBox.textContent = `✓ Success!\n${res}`;
    showToast("✓ Flashed to controller over BLE!");
  } catch (err) {
    bleOutputBox.textContent = `❌ Error: ${err}`;
  } finally {
    btnFlashBle.disabled = false;
  }
});

// ----------------------------------------------------------------------------
// Raw TOML Tab
// ----------------------------------------------------------------------------
btnFormatToml.addEventListener("click", () => {
  try {
    const parsed = parseProfilesToml(rawTomlEditor.value);
    appConfig = parsed;
    rawTomlEditor.value = serializeProfilesToml(appConfig);
    updateActiveProfileView();
    showToast("✓ TOML Formatted and validated");
  } catch (err) {
    alert(`TOML Syntax error: ${err}`);
  }
});

rawTomlEditor.addEventListener("change", () => {
  try {
    appConfig = parseProfilesToml(rawTomlEditor.value);
    updateActiveProfileView();
  } catch {
    // Wait until formatted
  }
});

// ----------------------------------------------------------------------------
// Utilities
// ----------------------------------------------------------------------------
function setupTabs() {
  document.querySelectorAll(".tab-btn").forEach((btn) => {
    btn.addEventListener("click", () => {
      document.querySelectorAll(".tab-btn").forEach((b) => b.classList.remove("active"));
      document.querySelectorAll(".tab-pane").forEach((p) => p.classList.remove("active"));

      btn.classList.add("active");
      const target = btn.getAttribute("data-tab");
      if (target) {
        document.getElementById(target)?.classList.add("active");
      }
    });
  });
}

function formatActionText(action?: ActionData): string {
  if (!action) return "None";
  if (action.execCmd) return `⚡ ${action.execCmd}`;
  if (!action.keys || action.keys.length === 0) return "None";
  return action.keys.join(" + ");
}

function truncate(s: string, maxLen: number): string {
  return s.length > maxLen ? s.substring(0, maxLen - 1) + "…" : s;
}

function showToast(msg: string) {
  toast.textContent = msg;
  toast.classList.add("show");
  setTimeout(() => toast.classList.remove("show"), 2500);
}

// Start app
window.addEventListener("DOMContentLoaded", init);
