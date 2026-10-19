export interface ButtonMeta {
  label: string;
  color: string;
  side: "left" | "right";
  coord: [number, number];
  elbow1: [number, number];
  elbow2: [number, number];
  target: [number, number];
}

export const BUTTON_METADATA: Record<string, ButtonMeta> = {
  // Left Side Buttons
  L2: {
    label: "L2",
    color: "#f38ba8",
    side: "left",
    coord: [474, 272],
    elbow1: [474, 120],
    elbow2: [330, 105],
    target: [280, 105],
  },
  L: {
    label: "L1",
    color: "#fab387",
    side: "left",
    coord: [524, 272],
    elbow1: [524, 180],
    elbow2: [350, 165],
    target: [280, 165],
  },
  UP: {
    label: "▲ UP",
    color: "#a6e3a1",
    side: "left",
    coord: [489, 346],
    elbow1: [430, 265],
    elbow2: [340, 225],
    target: [280, 225],
  },
  LEFT: {
    label: "◀ LEFT",
    color: "#a6e3a1",
    side: "left",
    coord: [460, 375],
    elbow1: [380, 305],
    elbow2: [340, 285],
    target: [280, 285],
  },
  SELECT: {
    label: "- SELECT",
    color: "#89b4fa",
    side: "left",
    coord: [542, 334],
    elbow1: [480, 335],
    elbow2: [380, 345],
    target: [280, 345],
  },
  DOWN: {
    label: "▼ DOWN",
    color: "#a6e3a1",
    side: "left",
    coord: [489, 404],
    elbow1: [430, 405],
    elbow2: [360, 405],
    target: [280, 405],
  },
  RIGHT: {
    label: "▶ RIGHT",
    color: "#a6e3a1",
    side: "left",
    coord: [518, 375],
    elbow1: [480, 445],
    elbow2: [380, 465],
    target: [280, 465],
  },
  STAR: {
    label: "★ STAR",
    color: "#cba6f7",
    side: "left",
    coord: [556, 418],
    elbow1: [480, 495],
    elbow2: [380, 525],
    target: [280, 525],
  },

  // Right Side Buttons
  R2: {
    label: "R2",
    color: "#f38ba8",
    side: "right",
    coord: [694, 272],
    elbow1: [694, 120],
    elbow2: [870, 105],
    target: [920, 105],
  },
  R: {
    label: "R1",
    color: "#fab387",
    side: "right",
    coord: [644, 272],
    elbow1: [644, 180],
    elbow2: [850, 165],
    target: [920, 165],
  },
  START: {
    label: "+ START",
    color: "#89b4fa",
    side: "right",
    coord: [614, 334],
    elbow1: [680, 265],
    elbow2: [840, 225],
    target: [920, 225],
  },
  X: {
    label: "X",
    color: "#f9e2af",
    side: "right",
    coord: [672, 340],
    elbow1: [740, 295],
    elbow2: [840, 285],
    target: [920, 285],
  },
  Y: {
    label: "Y",
    color: "#f9e2af",
    side: "right",
    coord: [636, 376],
    elbow1: [710, 355],
    elbow2: [840, 345],
    target: [920, 345],
  },
  A: {
    label: "A",
    color: "#f9e2af",
    side: "right",
    coord: [710, 376],
    elbow1: [770, 390],
    elbow2: [850, 405],
    target: [920, 405],
  },
  B: {
    label: "B",
    color: "#f9e2af",
    side: "right",
    coord: [672, 414],
    elbow1: [740, 445],
    elbow2: [840, 465],
    target: [920, 465],
  },
  HOME: {
    label: "♥ HOME",
    color: "#cba6f7",
    side: "right",
    coord: [600, 418],
    elbow1: [670, 495],
    elbow2: [820, 525],
    target: [920, 525],
  },
};

export function getButtonMeta(btnKey: string): ButtonMeta | undefined {
  if (btnKey === "L1") return BUTTON_METADATA["L"];
  if (btnKey === "R1") return BUTTON_METADATA["R"];
  return BUTTON_METADATA[btnKey];
}

const DPAD_RING = ["UP", "RIGHT", "DOWN", "LEFT"] as const;

export function rotateDpadButton(btn: string, turnDegrees: number): string {
  const norm = btn.toUpperCase();
  const idx = DPAD_RING.indexOf(norm as any);
  if (idx === -1) return norm;
  const steps = Math.floor((((turnDegrees % 360) + 360) % 360) / 90);
  return DPAD_RING[(idx + steps) % 4];
}

export const CONTROLLER_BUTTON_KEYS = [
  "L2", "L", "R2", "R",
  "UP", "DOWN", "LEFT", "RIGHT",
  "A", "B", "X", "Y",
  "SELECT", "START", "STAR", "HOME"
] as const;

export const KEY_CATEGORIES: Record<string, string[]> = {
  Modifiers: [
    "LEFTCTRL",
    "LEFTSHIFT",
    "LEFTALT",
    "LEFTMETA",
    "RIGHTCTRL",
    "RIGHTSHIFT",
    "RIGHTALT",
    "RIGHTMETA",
  ],
  Common: [
    "SPACE",
    "ENTER",
    "ESC",
    "BACKSPACE",
    "TAB",
    "DELETE",
    "CAPSLOCK",
  ],
  Navigation: [
    "UP",
    "DOWN",
    "LEFT",
    "RIGHT",
    "PAGEUP",
    "PAGEDOWN",
    "HOME",
    "END",
    "INSERT",
  ],
  Function: [
    "F1", "F2", "F3", "F4", "F5", "F6",
    "F7", "F8", "F9", "F10", "F11", "F12",
    "F13", "F14", "F15", "F16", "F17", "F18", "F19", "F20",
  ],
  Letters: [
    "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M",
    "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z",
  ],
  Numbers: [
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9",
  ],
  Media: [
    "PLAYPAUSE",
    "MUTE",
    "VOLUMEUP",
    "VOLUMEDOWN",
    "NEXTSONG",
    "PREVIOUSSONG",
    "STOPCD",
  ],
  Numpad: [
    "KP0", "KP1", "KP2", "KP3", "KP4", "KP5", "KP6", "KP7", "KP8", "KP9",
    "KPENTER", "KPPLUS", "KPMINUS", "KPASTERISK", "KPSLASH", "KPDOT",
  ],
  Symbols: [
    "MINUS",
    "EQUAL",
    "LEFTBRACE",
    "RIGHTBRACE",
    "SEMICOLON",
    "APOSTROPHE",
    "GRAVE",
    "BACKSLASH",
    "COMMA",
    "DOT",
    "SLASH",
  ],
};
