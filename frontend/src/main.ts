import { invoke } from "@tauri-apps/api/core";
import {
  BUTTON_METADATA,
  KEY_CATEGORIES,
  CONTROLLER_BUTTON_KEYS,
  getButtonMeta,
  rotateDpadButton,
} from "./keyCatalog";
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
let editingTarget: { type: "button"; key: string } | null = null;
let editingComboKey: string | null = null;
const selectedComboButtons = new Set<string>();
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

// Modals - Single Button
const modalBinding = document.getElementById("modal-binding") as HTMLDivElement;
const modalBindingTitle = document.getElementById("modal-binding-title") as HTMLElement;
const inputBindingKeys = document.getElementById("input-binding-keys") as HTMLInputElement;
const inputBindingExec = document.getElementById("input-binding-exec") as HTMLInputElement;
const inputBindingDesc = document.getElementById("input-binding-desc") as HTMLInputElement;
const keyRecorderBox = document.getElementById("key-recorder-box") as HTMLDivElement;
const btnBindingApply = document.getElementById("btn-binding-apply") as HTMLButtonElement;
const btnBindingClear = document.getElementById("btn-binding-clear") as HTMLButtonElement;

// Modals - Combo & Chords
const modalCombo = document.getElementById("modal-combo") as HTMLDivElement;
const modalComboTitle = document.getElementById("modal-combo-title") as HTMLElement;
const comboTriggerCount = document.getElementById("combo-trigger-count") as HTMLElement;
const comboButtonsGrid = document.getElementById("combo-buttons-grid") as HTMLDivElement;
const inputComboTrigger = document.getElementById("input-combo-trigger") as HTMLInputElement;
const comboKeyRecorderBox = document.getElementById("combo-key-recorder-box") as HTMLDivElement;
const inputComboKeys = document.getElementById("input-combo-keys") as HTMLInputElement;
const comboChipsModifiers = document.getElementById("combo-chips-modifiers") as HTMLDivElement;
const comboChipsCommon = document.getElementById("combo-chips-common") as HTMLDivElement;
const comboChipsFn = document.getElementById("combo-chips-fn") as HTMLDivElement;
const inputComboExec = document.getElementById("input-combo-exec") as HTMLInputElement;
const inputComboDesc = document.getElementById("input-combo-desc") as HTMLInputElement;
const btnComboDelete = document.getElementById("btn-combo-delete") as HTMLButtonElement;
const btnComboApply = document.getElementById("btn-combo-apply") as HTMLButtonElement;

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
const btnQuickFlash = document.getElementById("btn-quick-flash") as HTMLButtonElement | null;
const blePresetSelect = document.getElementById("ble-preset-select") as HTMLSelectElement;
const bleTurnSelect = document.getElementById("ble-turn-select") as HTMLSelectElement;
const bleOutputBox = document.getElementById("ble-output-box") as HTMLDivElement;
let platformInfo = { os: "linux", supports_focus_daemon: true };

// Visual Mapper Zoom
const mapperSvg = document.getElementById("mapper-svg") as SVGElement | null;
const zoomLevelLabel = document.getElementById("zoom-level-label") as HTMLSpanElement | null;
const btnZoomIn = document.getElementById("btn-zoom-in") as HTMLButtonElement | null;
const btnZoomOut = document.getElementById("btn-zoom-out") as HTMLButtonElement | null;
const btnZoomFit = document.getElementById("btn-zoom-fit") as HTMLButtonElement | null;
let mapperZoomLevel = 1.0;

function setMapperZoom(level: number) {
  mapperZoomLevel = Math.max(0.6, Math.min(2.5, Math.round(level * 20) / 20));
  if (mapperSvg) {
    mapperSvg.style.transform = `scale(${mapperZoomLevel})`;
  }
  if (zoomLevelLabel) {
    zoomLevelLabel.textContent = `${Math.round(mapperZoomLevel * 100)}%`;
  }
}

function setupMapperZoom() {
  btnZoomIn?.addEventListener("click", () => setMapperZoom(mapperZoomLevel + 0.1));
  btnZoomOut?.addEventListener("click", () => setMapperZoom(mapperZoomLevel - 0.1));
  btnZoomFit?.addEventListener("click", () => setMapperZoom(1.0));

  const canvasWrapper = document.querySelector(".canvas-wrapper") as HTMLElement | null;
  canvasWrapper?.addEventListener(
    "wheel",
    (e: WheelEvent) => {
      if (e.ctrlKey) {
        e.preventDefault();
        const delta = e.deltaY < 0 ? 0.08 : -0.08;
        setMapperZoom(mapperZoomLevel + delta);
      }
    },
    { passive: false }
  );
}

// ----------------------------------------------------------------------------
// Initialization
// ----------------------------------------------------------------------------
async function init() {
  setupTabs();
  setupMapperZoom();
  initComboButtonsGrid();
  setupCatalogChips();
  setupKeyRecorder();
  setupModalEvents();

  const controllerImgEl = document.getElementById("svg-controller-img") as SVGImageElement | null;
  if (controllerImgEl) {
    controllerImgEl.setAttribute("href", controllerPng);
  }

  try {
    platformInfo = await invoke<{ os: string; supports_focus_daemon: boolean }>("get_platform_info");
    if (!platformInfo.supports_focus_daemon) {
      const brandBadge = document.querySelector(".brand-badge") as HTMLElement | null;
      if (brandBadge) {
        brandBadge.textContent = "Hardware Manager";
      }
      if (btnPickWindow) {
        btnPickWindow.style.display = "none";
      }
      const combosNotice = document.getElementById("combos-linux-notice") as HTMLElement | null;
      if (combosNotice) {
        combosNotice.style.display = "block";
      }
    }
  } catch (err) {
    console.debug("Could not get platform info:", err);
  }

  // Quick Flash button in top bar
  btnQuickFlash?.addEventListener("click", async () => {
    btnQuickFlash.disabled = true;
    const origText = btnQuickFlash.textContent;
    btnQuickFlash.textContent = "⏳ Flashing...";
    showToast(`Connecting to 8BitDo Micro over BLE and flashing '${activeProfileName}'...`);
    try {
      await saveConfigToDisk();
      const res = await invoke<string>("flash_preset", {
        preset: activeProfileName,
        turn: appConfig.profiles[activeProfileName]?.turn
      });
      showToast("✓ Flashed to controller over BLE!");
      alert(`✓ Flashed profile '${activeProfileName}' to 8BitDo Micro hardware over BLE!\n\n${res}`);
    } catch (err) {
      showToast(`❌ BLE Flash failed: ${err}`);
      alert(`❌ Flash error:\n${err}`);
    } finally {
      btnQuickFlash.disabled = false;
      btnQuickFlash.textContent = origText;
    }
  });

  try {
    const path = await invoke<string>("get_config_path");
    if (configPathLabel) configPathLabel.textContent = path;
  } catch (err) {
    console.error("Failed to get config path:", err);
  }

  await loadConfigFromDisk();
  await updateDaemonStatus();

  // Periodic daemon check every 4 seconds (Linux only)
  if (platformInfo.supports_focus_daemon) {
    setInterval(updateDaemonStatus, 4000);
  }
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
    const origText = btnSaveAll.textContent;
    btnSaveAll.textContent = "✓ Saved & Reloaded!";
    setTimeout(() => {
      btnSaveAll.textContent = origText;
    }, 1500);
  } catch (err) {
    showToast(`Error saving config: ${err}`);
  }
}

// ----------------------------------------------------------------------------
// Daemon Management
// ----------------------------------------------------------------------------
async function updateDaemonStatus() {
  if (!platformInfo.supports_focus_daemon) {
    btnDaemonStatus.className = "success";
    btnDaemonStatus.textContent = "📡 BLE Flasher Mode";
    btnDaemonStatus.title = "Direct BLE hardware flasher active. Controller stores mappings in internal EEPROM.";
    return;
  }
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
  if (!platformInfo.supports_focus_daemon) {
    const bleTabBtn = document.querySelector("[data-tab='tab-ble']") as HTMLButtonElement | null;
    bleTabBtn?.click();
    return;
  }
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
function refreshBlePresetSelect() {
  if (!blePresetSelect) return;
  const currentVal = blePresetSelect.value;
  blePresetSelect.innerHTML = "";

  const optActive = document.createElement("option");
  optActive.value = activeProfileName;
  optActive.textContent = `★ Current Profile: ${activeProfileName}`;
  blePresetSelect.appendChild(optActive);

  const optGroupUser = document.createElement("optgroup");
  optGroupUser.label = "Custom Profiles";
  for (const name of Object.keys(appConfig.profiles)) {
    if (name === activeProfileName || name === "global") continue;
    const opt = document.createElement("option");
    opt.value = name;
    opt.textContent = name;
    optGroupUser.appendChild(opt);
  }
  if (optGroupUser.children.length > 0) {
    blePresetSelect.appendChild(optGroupUser);
  }

  const optGroupBuiltin = document.createElement("optgroup");
  optGroupBuiltin.label = "Built-in Presets";
  const builtins = [
    { id: "sentinel", label: "sentinel (F13..F20 + Numpad for host baseline)" },
    { id: "factory", label: "factory (Default keyboard mode: c..t keys)" },
    { id: "anki", label: "anki (Official Anki preset: 1, 2, 3, 4, Space)" },
    { id: "supermemo", label: "supermemo (SuperMemo navigation preset)" },
  ];
  for (const b of builtins) {
    const opt = document.createElement("option");
    opt.value = b.id;
    opt.textContent = b.label;
    optGroupBuiltin.appendChild(opt);
  }
  blePresetSelect.appendChild(optGroupBuiltin);

  if (currentVal && Array.from(blePresetSelect.options).some((o) => o.value === currentVal)) {
    blePresetSelect.value = currentVal;
  } else {
    blePresetSelect.value = activeProfileName;
  }
}

function updateActiveProfileView() {
  const profile = appConfig.profiles[activeProfileName];
  if (!profile) return;

  turnSelect.value = String(profile.turn || 0);

  renderSvgCallouts();
  renderCombosList();
  refreshBlePresetSelect();
}

function renderSvgCallouts() {
  svgCalloutsLayer.innerHTML = "";
  svgPinsLayer.innerHTML = "";

  const profile = appConfig.profiles[activeProfileName];
  if (!profile) return;

  const globalProfile = appConfig.profiles["global"];
  const turn = profile.turn || 0;

  for (const btnKey of CONTROLLER_BUTTON_KEYS) {
    const meta = BUTTON_METADATA[btnKey];
    if (!meta) continue;

    // Resolve logical key if D-Pad was rotated by profile.turn
    const logicalKey = rotateDpadButton(btnKey, turn);

    // Resolve button action (profile action > global action)
    const action = profile.buttons[logicalKey] || globalProfile?.buttons[logicalKey];
    const isInherited = !profile.buttons[logicalKey] && !!globalProfile?.buttons[logicalKey];

    // Format action and description
    const actionStr = formatActionText(action);
    const hasDesc = !!(action && action.description);
    const descText = action?.description?.trim() || "";

    const fullTooltip = hasDesc
      ? `${meta.label}${logicalKey !== btnKey ? ` (➔ ${logicalKey})` : ""}: ${actionStr} — ${descText}${isInherited ? " [Inherited from global]" : ""}`
      : `${meta.label}${logicalKey !== btnKey ? ` (➔ ${logicalKey})` : ""}: ${actionStr}${isInherited ? " [Inherited from global]" : ""}`;

    // 1. Draw glowing pin on controller image
    const [px, py] = meta.coord;
    const pin = document.createElementNS("http://www.w3.org/2000/svg", "circle");
    pin.setAttribute("cx", String(px));
    pin.setAttribute("cy", String(py));
    pin.setAttribute("r", "5");
    pin.setAttribute("fill", meta.color);
    pin.setAttribute("filter", "url(#glow)");

    const pinTitle = document.createElementNS("http://www.w3.org/2000/svg", "title");
    pinTitle.textContent = fullTooltip;
    pin.appendChild(pinTitle);

    pin.addEventListener("click", (e) => {
      e.stopPropagation();
      openBindingModal("button", logicalKey);
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
    const by = ty - 20;
    cardGroup.setAttribute("transform", `translate(${bx}, ${by})`);

    // SVG Title tooltip for card hover
    const cardTitle = document.createElementNS("http://www.w3.org/2000/svg", "title");
    cardTitle.textContent = fullTooltip;
    cardGroup.appendChild(cardTitle);

    // Card background
    const cardBg = document.createElementNS("http://www.w3.org/2000/svg", "rect");
    cardBg.classList.add("card-bg");
    cardBg.setAttribute("width", "250");
    cardBg.setAttribute("height", "40");
    cardBg.setAttribute("rx", "8");
    cardBg.setAttribute("fill", "url(#card-grad)");
    cardBg.setAttribute("stroke", isInherited ? "#45475a" : "#313244");
    cardBg.setAttribute("stroke-width", "1.2");
    cardGroup.appendChild(cardBg);

    // Button badge
    const badgeBg = document.createElementNS("http://www.w3.org/2000/svg", "rect");
    badgeBg.setAttribute("x", "6");
    badgeBg.setAttribute("y", "7");
    badgeBg.setAttribute("width", "68");
    badgeBg.setAttribute("height", "26");
    badgeBg.setAttribute("rx", "5");
    badgeBg.setAttribute("fill", meta.color);
    badgeBg.setAttribute("fill-opacity", "0.18");
    badgeBg.setAttribute("stroke", meta.color);
    badgeBg.setAttribute("stroke-width", "1");
    cardGroup.appendChild(badgeBg);

    const badgeText = document.createElementNS("http://www.w3.org/2000/svg", "text");
    badgeText.setAttribute("x", "40");
    badgeText.setAttribute("y", "24");
    badgeText.setAttribute("text-anchor", "middle");
    badgeText.setAttribute("fill", meta.color);
    badgeText.setAttribute("font-size", "11.5");
    badgeText.setAttribute("font-weight", "bold");
    badgeText.textContent = meta.label;
    cardGroup.appendChild(badgeText);

    if (hasDesc && descText) {
      // Line 1: Action (e.g. "Space" or "Ctrl + Shift + D")
      const txtMain = document.createElementNS("http://www.w3.org/2000/svg", "text");
      txtMain.setAttribute("x", "84");
      txtMain.setAttribute("y", "18");
      txtMain.setAttribute("fill", isInherited ? "#a6adc8" : "#cdd6f4");
      txtMain.setAttribute("font-size", "12");
      txtMain.setAttribute("font-weight", "bold");
      txtMain.textContent = truncate(actionStr, 20);
      cardGroup.appendChild(txtMain);

      // Line 2: Inline description / comment (e.g. "Start, Stop video")
      const txtDesc = document.createElementNS("http://www.w3.org/2000/svg", "text");
      txtDesc.setAttribute("x", "84");
      txtDesc.setAttribute("y", "32");
      txtDesc.setAttribute("fill", "#a6adc8");
      txtDesc.setAttribute("font-size", "10.5");
      txtDesc.setAttribute("font-weight", "600");
      txtDesc.textContent = truncate(descText, 25);
      cardGroup.appendChild(txtDesc);
    } else {
      // Single centered line: Action
      const txtMain = document.createElementNS("http://www.w3.org/2000/svg", "text");
      txtMain.setAttribute("x", "84");
      txtMain.setAttribute("y", "25");
      txtMain.setAttribute("fill", isInherited ? "#a6adc8" : (actionStr === "None" ? "#6c7086" : "#cdd6f4"));
      txtMain.setAttribute("font-size", "13");
      txtMain.setAttribute("font-weight", "bold");
      txtMain.textContent = truncate(actionStr, 22);
      cardGroup.appendChild(txtMain);
    }

    cardGroup.addEventListener("click", (e) => {
      e.stopPropagation();
      openBindingModal("button", logicalKey);
    });
    svgCalloutsLayer.appendChild(cardGroup);
  }
}

// ----------------------------------------------------------------------------
// Combos List Rendering & Controller Buttons Picker
// ----------------------------------------------------------------------------
function initComboButtonsGrid() {
  comboButtonsGrid.innerHTML = "";
  for (const btn of CONTROLLER_BUTTON_KEYS) {
    const meta = BUTTON_METADATA[btn];
    const label = meta?.label || btn;
    const chip = document.createElement("button");
    chip.type = "button";
    chip.className = "ctrl-btn-chip";
    chip.dataset.btn = btn;
    chip.textContent = label;
    chip.addEventListener("click", () => {
      if (selectedComboButtons.has(btn)) {
        selectedComboButtons.delete(btn);
      } else {
        selectedComboButtons.add(btn);
      }
      updateComboTriggerFromSelection();
    });
    comboButtonsGrid.appendChild(chip);
  }

  // Preset chips
  modalCombo.querySelectorAll<HTMLButtonElement>(".preset-chip").forEach((btn) => {
    btn.addEventListener("click", () => {
      const trigger = btn.dataset.trigger;
      if (trigger) {
        setComboTrigger(trigger);
      }
    });
  });

  // Manual typing in trigger input
  inputComboTrigger.addEventListener("input", () => {
    syncSelectionFromTrigger(inputComboTrigger.value);
  });
}

function setComboTrigger(trigger: string) {
  inputComboTrigger.value = trigger;
  syncSelectionFromTrigger(trigger);
}

function syncSelectionFromTrigger(trigger: string) {
  selectedComboButtons.clear();
  const parts = trigger.split("+").map((p) => p.trim().toUpperCase()).filter(Boolean);
  for (const p of parts) {
    const normalized = p === "L1" ? "L" : p === "R1" ? "R" : p;
    selectedComboButtons.add(normalized);
  }
  updateComboButtonsUI();
}

function updateComboTriggerFromSelection() {
  inputComboTrigger.value = Array.from(selectedComboButtons).join("+");
  updateComboButtonsUI();
}

function updateComboButtonsUI() {
  const count = selectedComboButtons.size;
  comboTriggerCount.textContent = `${count} selected${count >= 2 ? " (valid combo)" : " (need 2+ buttons)"}`;
  comboTriggerCount.style.color = count >= 2 ? "var(--accent-green)" : "var(--accent-orange)";

  comboButtonsGrid.querySelectorAll<HTMLButtonElement>(".ctrl-btn-chip").forEach((chip) => {
    const btn = chip.dataset.btn;
    if (btn && selectedComboButtons.has(btn)) {
      chip.classList.add("selected");
    } else {
      chip.classList.remove("selected");
    }
  });
}

function renderCombosList() {
  combosContainer.innerHTML = "";
  const profile = appConfig.profiles[activeProfileName];
  if (!profile) return;

  const combos = profile.combos;
  const comboKeys = Object.keys(combos);

  if (comboKeys.length === 0) {
    combosContainer.innerHTML = `
      <div style="grid-column: 1 / -1; padding: 24px; text-align: center; color: var(--text-muted); font-size: 13px;">
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
      <div style="flex: 1; display: flex; flex-direction: column; gap: 4px;">
        <div style="display: flex; align-items: center; gap: 8px;">
          <span class="combo-trigger-badge">${displayKey}</span>
          <span style="font-size: 11px; color: var(--text-muted); font-family: monospace;">(Trigger)</span>
        </div>
        <div style="font-size: 13px; color: var(--text-main); font-weight: 600; display: flex; align-items: center; gap: 6px;">
          <span style="color: var(--accent-blue);">➔</span>
          <span class="combo-action-badge">${formatActionText(act)}</span>
        </div>
        ${act.description ? `<div style="font-size: 11px; color: var(--text-muted);">• ${act.description}</div>` : ""}
      </div>
      <div style="display: flex; gap: 6px; align-items: center;">
        <button class="btn-edit-combo" title="Configure binding & key capture">⌨️ Record / Edit</button>
        <button class="danger btn-del-combo" title="Delete Combo" style="padding: 4px 8px; font-size: 11px;">✕</button>
      </div>
    `;

    card.querySelector(".btn-del-combo")?.addEventListener("click", (e) => {
      e.stopPropagation();
      delete profile.combos[comboKey];
      renderCombosList();
      showToast(`Removed combo [${comboKey}]`);
    });

    card.querySelector(".btn-edit-combo")?.addEventListener("click", (e) => {
      e.stopPropagation();
      openComboModal(comboKey);
    });

    card.addEventListener("click", () => openComboModal(comboKey));
    combosContainer.appendChild(card);
  }
}

// ----------------------------------------------------------------------------
// Modals & Key Recording
// ----------------------------------------------------------------------------
function openBindingModal(type: "button", key: string) {
  editingTarget = { type, key };
  const profile = appConfig.profiles[activeProfileName];
  const globalProfile = appConfig.profiles["global"];
  const action: ActionData | undefined = profile?.buttons[key] || globalProfile?.buttons[key];

  const meta = getButtonMeta(key);
  const label = meta?.label || key;
  modalBindingTitle.textContent = `Configure Button ${label} [${key}] in '${activeProfileName}'`;

  inputBindingKeys.value = action ? action.keys.join("+") : "";
  inputBindingExec.value = action?.execCmd || "";
  inputBindingDesc.value = action?.description || "";

  modalBinding.classList.add("open");
  inputBindingKeys.focus();
}

function openComboModal(comboKey?: string) {
  const profile = appConfig.profiles[activeProfileName];

  if (comboKey) {
    editingComboKey = comboKey;
    modalComboTitle.textContent = `Edit Combo [${comboKey}] in '${activeProfileName}'`;
    setComboTrigger(comboKey);

    const act = profile?.combos[comboKey];
    inputComboKeys.value = act?.keys ? act.keys.join("+") : "";
    inputComboExec.value = act?.execCmd || "";
    inputComboDesc.value = act?.description || "";
    btnComboDelete.style.display = "inline-block";
  } else {
    editingComboKey = null;
    modalComboTitle.textContent = `Add New Combo in '${activeProfileName}'`;
    setComboTrigger("");
    inputComboKeys.value = "";
    inputComboExec.value = "";
    inputComboDesc.value = "";
    btnComboDelete.style.display = "none";
  }

  modalCombo.classList.add("open");
  inputComboKeys.focus();
}

let activeRecorderBox: HTMLElement | null = null;
let activeRecorderInput: HTMLInputElement | null = null;

function startRecording(box: HTMLElement, input: HTMLInputElement) {
  isRecording = true;
  activeRecorderBox = box;
  activeRecorderInput = input;
  currentlyPressedKeys.clear();
  box.classList.add("recording");
  box.style.borderColor = "var(--accent-orange)";
  const span = box.querySelector("span");
  if (span) span.textContent = "🔴 Listening... Press keys on your keyboard";
}

function stopRecording(captured: boolean) {
  if (!activeRecorderBox) return;
  isRecording = false;
  activeRecorderBox.classList.remove("recording");
  activeRecorderBox.style.borderColor = "var(--border-focus)";
  const span = activeRecorderBox.querySelector("span");
  if (span) {
    span.textContent = captured
      ? "✓ Captured! Click again to re-record"
      : "⌨️ Click here & press keys on your keyboard";
  }
  activeRecorderBox = null;
  activeRecorderInput = null;
  currentlyPressedKeys.clear();
}

function closeAllModals() {
  document.querySelectorAll(".modal-backdrop").forEach((m) => m.classList.remove("open"));
  if (activeRecorderBox) {
    stopRecording(false);
  }
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

  // Apply single button binding
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

    profile.buttons[editingTarget.key] = newAction;
    renderSvgCallouts();

    closeAllModals();
    showToast(`✓ Updated binding for ${editingTarget.key}`);
  });

  // Clear single button binding
  btnBindingClear.addEventListener("click", () => {
    if (!editingTarget) return;
    const profile = appConfig.profiles[activeProfileName];
    if (!profile) return;

    delete profile.buttons[editingTarget.key];
    renderSvgCallouts();

    closeAllModals();
    showToast(`Cleared ${editingTarget.key}`);
  });

  // Apply Combo
  btnComboApply.addEventListener("click", () => {
    const profile = appConfig.profiles[activeProfileName];
    if (!profile) return;

    const trigger = inputComboTrigger.value.trim().toUpperCase();
    if (!trigger) {
      showToast("⚠️ Please select at least 2 trigger buttons");
      return;
    }

    const rawKeys = inputComboKeys.value.trim();
    const execCmd = inputComboExec.value.trim();
    const desc = inputComboDesc.value.trim();

    if (!rawKeys && !execCmd) {
      showToast("⚠️ Please record a key chord or enter a shell command");
      return;
    }

    const keysArray = rawKeys ? rawKeys.split("+").map((k) => k.trim().toUpperCase()) : [];
    const newAction: ActionData = {
      keys: keysArray,
      execCmd: execCmd || undefined,
      description: desc || undefined,
    };

    if (editingComboKey && editingComboKey !== trigger) {
      delete profile.combos[editingComboKey];
    }

    profile.combos[trigger] = newAction;
    renderCombosList();
    closeAllModals();
    showToast(`✓ Saved combo [${trigger}]`);
  });

  // Delete Combo from Modal
  btnComboDelete.addEventListener("click", () => {
    const profile = appConfig.profiles[activeProfileName];
    if (!profile) return;

    if (editingComboKey) {
      delete profile.combos[editingComboKey];
      renderCombosList();
      closeAllModals();
      showToast(`Removed combo [${editingComboKey}]`);
    }
  });

  // Add Combo Button
  btnAddCombo.addEventListener("click", () => {
    openComboModal();
  });
}

function setupKeyRecorder() {
  keyRecorderBox.addEventListener("click", () => startRecording(keyRecorderBox, inputBindingKeys));
  comboKeyRecorderBox.addEventListener("click", () => startRecording(comboKeyRecorderBox, inputComboKeys));

  window.addEventListener("keydown", (e) => {
    if (!isRecording || !activeRecorderInput) return;
    e.preventDefault();

    if (e.key === "Escape") {
      stopRecording(false);
      return;
    }

    const keyName = translateDomKey(e.key);
    currentlyPressedKeys.add(keyName);

    // Build combination string
    const chord = Array.from(currentlyPressedKeys).join("+");
    activeRecorderInput.value = chord;
  });

  window.addEventListener("keyup", (e) => {
    if (!isRecording || !activeRecorderInput) return;
    currentlyPressedKeys.delete(translateDomKey(e.key));
    if (currentlyPressedKeys.size === 0) {
      stopRecording(true);
    }
  });
}

function translateDomKey(key: string): string {
  const map: Record<string, string> = {
    Control: "LEFTCTRL",
    Shift: "LEFTSHIFT",
    Alt: "LEFTALT",
    AltGraph: "RIGHTALT",
    Meta: "LEFTMETA",
    Super: "LEFTMETA",
    OS: "LEFTMETA",
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
    CapsLock: "CAPSLOCK",
    PageUp: "PAGEUP",
    PageDown: "PAGEDOWN",
    Home: "HOME",
    End: "END",
    Insert: "INSERT",
    ",": "COMMA",
    ".": "DOT",
    "/": "SLASH",
    ";": "SEMICOLON",
    "'": "APOSTROPHE",
    "[": "LEFTBRACE",
    "]": "RIGHTBRACE",
    "\\": "BACKSLASH",
    "-": "MINUS",
    "=": "EQUAL",
    "`": "GRAVE",
  };
  return map[key] || key.toUpperCase();
}

function setupCatalogChips() {
  const chipsMod = document.getElementById("chips-modifiers");
  const chipsCommon = document.getElementById("chips-common");
  const chipsFn = document.getElementById("chips-fn");

  function addChips(container: HTMLElement | null, inputEl: HTMLInputElement, keys: string[]) {
    if (!container) return;
    container.innerHTML = "";
    for (const k of keys) {
      const chip = document.createElement("span");
      chip.className = "key-chip";
      chip.textContent = k;
      chip.addEventListener("click", () => {
        const cur = inputEl.value.trim();
        inputEl.value = cur ? `${cur}+${k}` : k;
      });
      container.appendChild(chip);
    }
  }

  // Populate Single Button Modal
  addChips(chipsMod, inputBindingKeys, KEY_CATEGORIES["Modifiers"]);
  addChips(chipsCommon, inputBindingKeys, [...KEY_CATEGORIES["Common"], ...KEY_CATEGORIES["Navigation"]]);
  addChips(chipsFn, inputBindingKeys, KEY_CATEGORIES["Function"]);

  // Populate Combo Modal
  addChips(comboChipsModifiers, inputComboKeys, KEY_CATEGORIES["Modifiers"]);
  addChips(comboChipsCommon, inputComboKeys, [...KEY_CATEGORIES["Common"], ...KEY_CATEGORIES["Navigation"]]);
  addChips(comboChipsFn, inputComboKeys, KEY_CATEGORIES["Function"]);
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
    renderSvgCallouts();
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

function prettyKeyName(k: string): string {
  const clean = k.replace(/^KEY_/, "").toUpperCase();
  const map: Record<string, string> = {
    LEFTCTRL: "Ctrl",
    RIGHTCTRL: "RCtrl",
    LEFTSHIFT: "Shift",
    RIGHTSHIFT: "RShift",
    LEFTALT: "Alt",
    RIGHTALT: "RAlt",
    LEFTMETA: "Super",
    RIGHTMETA: "RSuper",
    SUPER: "Super",
    SPACE: "Space",
    ENTER: "Enter",
    BACKSPACE: "Backspace",
    TAB: "Tab",
    ESC: "Esc",
    ESCAPE: "Esc",
    DELETE: "Del",
    PAGEUP: "PgUp",
    PAGEDOWN: "PgDn",
    HOME: "Home",
    END: "End",
    INSERT: "Ins",
    CAPSLOCK: "CapsLock",
    KPASTERISK: "Num *",
    KPMINUS: "Num -",
    KPPLUS: "Num +",
    KPENTER: "Num Enter",
    KPSLASH: "Num /",
    KPDOT: "Num .",
  };
  return map[clean] || (clean.length === 1 ? clean : clean.charAt(0) + clean.slice(1).toLowerCase());
}

function formatActionText(action?: ActionData): string {
  if (!action) return "None";
  if (action.execCmd) return `⚡ ${action.execCmd}`;
  if (!action.keys || action.keys.length === 0) return "None";
  return action.keys.map(prettyKeyName).join(" + ");
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
