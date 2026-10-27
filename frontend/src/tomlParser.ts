export interface ActionData {
  keys: string[];
  execCmd?: string;
  description?: string;
}

export interface ProfileData {
  name: string;
  description?: string;
  matchClass?: string;
  matchTitle?: string;
  turn: number;
  buttons: Record<string, ActionData>;
  combos: Record<string, ActionData>;
}

export interface AppConfigData {
  general: {
    fallbackProfile: string;
    backend: string;
    comboTimeoutMs: number;
    emitOsdEvents?: boolean;
  };
  profiles: Record<string, ProfileData>;
}

export function parseProfilesToml(tomlText: string): AppConfigData {
  const result: AppConfigData = {
    general: {
      fallbackProfile: "agy",
      backend: "uinput",
      comboTimeoutMs: 50,
      emitOsdEvents: true,
    },
    profiles: {},
  };

  const lines = tomlText.split(/\r?\n/);
  let currentSection = "";
  let currentProfile = "";
  let currentSub = ""; // 'buttons' | 'combos'

  for (let i = 0; i < lines.length; i++) {
    const rawLine = lines[i];
    const trimmed = rawLine.trim();

    if (!trimmed || trimmed.startsWith("#")) {
      continue;
    }

    // Section header: [general], [profiles.xxx], [profiles.xxx.buttons], [profiles.xxx.combos]
    const secMatch = trimmed.match(/^\[([^\]]+)\]$/);
    if (secMatch) {
      currentSection = secMatch[1].trim();
      if (currentSection === "general") {
        currentProfile = "";
        currentSub = "";
      } else if (currentSection.startsWith("profiles.")) {
        const parts = currentSection.split(".");
        currentProfile = parts[1];
        currentSub = parts.length > 2 ? parts[2] : "";

        if (currentProfile && !result.profiles[currentProfile]) {
          result.profiles[currentProfile] = {
            name: currentProfile,
            turn: 0,
            buttons: {},
            combos: {},
          };
        }
      }
      continue;
    }

    // Key-value pair with optional inline comment: key = value # comment
    const kvMatch = rawLine.match(/^([^=]+)=(.*)$/);
    if (!kvMatch) continue;

    const rawKey = kvMatch[1].trim();
    let valPart = kvMatch[2].trim();
    let inlineComment: string | undefined = undefined;

    // Extract inline comment if not inside string
    let inQuotes = false;
    let quoteChar = "";
    let splitIdx = -1;

    for (let c = 0; c < valPart.length; c++) {
      const char = valPart[c];
      if ((char === '"' || char === "'") && (c === 0 || valPart[c - 1] !== "\\")) {
        if (!inQuotes) {
          inQuotes = true;
          quoteChar = char;
        } else if (char === quoteChar) {
          inQuotes = false;
        }
      } else if (char === "#" && !inQuotes) {
        splitIdx = c;
        break;
      }
    }

    if (splitIdx !== -1) {
      inlineComment = valPart.substring(splitIdx + 1).trim();
      valPart = valPart.substring(0, splitIdx).trim();
    }

    // Parse value (string, array, number, boolean)
    let parsedVal: any;
    try {
      parsedVal = JSON.parse(valPart);
    } catch {
      // Clean quotes
      parsedVal = valPart.replace(/^["']|["']$/g, "");
    }

    // Assign to structure
    if (currentSection === "general") {
      if (rawKey === "fallback_profile") result.general.fallbackProfile = String(parsedVal);
      else if (rawKey === "backend") result.general.backend = String(parsedVal);
      else if (rawKey === "combo_timeout_ms") result.general.comboTimeoutMs = Number(parsedVal) || 50;
      else if (rawKey === "emit_osd_events") result.general.emitOsdEvents = parsedVal === true || parsedVal === "true";
    } else if (currentProfile && result.profiles[currentProfile]) {
      const prof = result.profiles[currentProfile];
      if (currentSub === "buttons") {
        const btnKey = rawKey.replace(/^["']|["']$/g, "").toUpperCase();
        prof.buttons[btnKey] = toActionData(parsedVal, inlineComment);
      } else if (currentSub === "combos") {
        const comboKey = rawKey.replace(/^["']|["']$/g, "").toUpperCase();
        prof.combos[comboKey] = toActionData(parsedVal, inlineComment);
      } else {
        if (rawKey === "description") prof.description = String(parsedVal);
        else if (rawKey === "match_class") prof.matchClass = String(parsedVal);
        else if (rawKey === "match_title") prof.matchTitle = String(parsedVal);
        else if (rawKey === "turn") prof.turn = Number(parsedVal) || 0;
      }
    }
  }

  return result;
}

function toActionData(val: any, comment?: string): ActionData {
  if (typeof val === "string") {
    if (val.toLowerCase().startsWith("exec:")) {
      return { keys: [], execCmd: val.substring(5).trim(), description: comment };
    }
    return { keys: [val.trim()], description: comment };
  } else if (Array.isArray(val)) {
    return { keys: val.map((k) => String(k).trim()), description: comment };
  }
  return { keys: [], description: comment };
}

export function serializeProfilesToml(config: AppConfigData): string {
  const lines: string[] = [
    "# 8BitDo Micro Focus-Aware Profile Configuration",
    "# Managed by 8BitDo Micro Focus Switch GUI & Daemon",
    "",
    "[general]",
    `fallback_profile = "${escapeTomlString(config.general.fallbackProfile)}"`,
    `backend = "${escapeTomlString(config.general.backend)}"`,
    `combo_timeout_ms = ${Number(config.general.comboTimeoutMs) || 50}`,
    `emit_osd_events = ${config.general.emitOsdEvents !== false}`,
    "",
  ];

  for (const [rawProfName, prof] of Object.entries(config.profiles)) {
    const profName = sanitizeTomlIdentifier(rawProfName);
    if (!profName) continue;
    lines.push(`[profiles.${profName}]`);
    if (prof.description) {
      lines.push(`description = "${escapeTomlString(prof.description)}"`);
    }
    if (prof.matchClass) {
      lines.push(`match_class = "${escapeTomlString(prof.matchClass)}"`);
    }
    if (prof.matchTitle) {
      lines.push(`match_title = "${escapeTomlString(prof.matchTitle)}"`);
    }
    if (prof.turn !== undefined && prof.turn !== 0) {
      lines.push(`turn = ${Number(prof.turn) || 0}`);
    }
    lines.push("");

    if (Object.keys(prof.combos).length > 0) {
      lines.push(`[profiles.${profName}.combos]`);
      for (const [comboKey, act] of Object.entries(prof.combos)) {
        const line = formatActionEntry(comboKey, act, true);
        lines.push(line);
      }
      lines.push("");
    }

    if (Object.keys(prof.buttons).length > 0) {
      lines.push(`[profiles.${profName}.buttons]`);
      for (const [btnKey, act] of Object.entries(prof.buttons)) {
        const line = formatActionEntry(btnKey, act, false);
        lines.push(line);
      }
      lines.push("");
    }
  }

  return lines.join("\n").trim() + "\n";
}

function formatActionEntry(key: string, act: ActionData, quoteKey: boolean): string {
  const cleanKey = key.replace(/[\r\n\0-\x1f\x7f]/g, "");
  const kStr = quoteKey || cleanKey.includes("+") ? `"${escapeTomlString(cleanKey)}"` : sanitizeTomlIdentifier(cleanKey);
  let valStr = "";

  if (act.execCmd) {
    valStr = `"exec: ${escapeTomlString(act.execCmd)}"`;
  } else if (act.keys.length === 1) {
    valStr = `"${escapeTomlString(act.keys[0])}"`;
  } else if (act.keys.length > 1) {
    valStr = JSON.stringify(act.keys);
  } else {
    valStr = `"NONE"`;
  }

  const cleanComment = act.description
    ? act.description.replace(/[\r\n]+/g, " ").replace(/[\0-\x1f\x7f]/g, "").trim()
    : "";
  const commentStr = cleanComment ? `  # ${cleanComment}` : "";
  return `${kStr} = ${valStr}${commentStr}`;
}

function sanitizeTomlIdentifier(s: string): string {
  return s.replace(/[^A-Za-z0-9_-]/g, "");
}

function escapeTomlString(s: string): string {
  return s
    .replace(/\\/g, "\\\\")
    .replace(/"/g, '\\"')
    .replace(/\r/g, "\\r")
    .replace(/\n/g, "\\n")
    .replace(/\t/g, "\\t")
    .replace(/[\0-\x08\x0b\x0c\x0e-\x1f\x7f]/g, "");
}
