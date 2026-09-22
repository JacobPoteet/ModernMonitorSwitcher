/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);

// ---------------------------------------------------------------------------
// Toast
// ---------------------------------------------------------------------------

let toastTimer = null;

function toast(message, kind = "") {
  const el = $("toast");
  el.textContent = message;
  el.className = `toast show ${kind}`;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => {
    el.className = "toast";
  }, kind === "error" ? 6000 : 2600);
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// Turn a stored accelerator into something readable.
/// "Control+Alt+Digit1" becomes "Ctrl+Alt+1".
function prettyAccelerator(accelerator) {
  return accelerator
    .split("+")
    .map((part) => {
      if (part === "Control" || part === "CommandOrControl") return "Ctrl";
      if (part === "Super" || part === "Meta") return "Win";
      if (part.startsWith("Digit")) return part.slice(5);
      if (part.startsWith("Key")) return part.slice(3);
      if (part.startsWith("Numpad")) return `Num${part.slice(6)}`;
      if (part.startsWith("Arrow")) return part.slice(5);
      return part;
    })
    .join("+");
}

function renderStatus(status) {
  const monitors = status.active_monitors;
  $("status-monitors").textContent = monitors.length
    ? monitors.join(", ")
    : "No monitors active";

  const parts = [];
  if (status.matching_profile) {
    parts.push(`Matches your ${status.matching_profile} profile`);
  } else {
    parts.push("Does not match any saved profile");
  }
  if (status.inactive_monitors.length) {
    parts.push(`${status.inactive_monitors.length} connected but unused`);
  }
  $("status-profile").textContent = parts.join(" · ");
}

function renderProfiles(profiles) {
  const container = $("profiles");
  container.replaceChildren();

  if (!profiles.length) {
    const empty = document.createElement("div");
    empty.className = "empty";
    empty.textContent =
      "No profiles yet. Arrange your monitors how you like them, then save the layout.";
    container.append(empty);
    return;
  }

  for (const profile of profiles) {
    container.append(profileRow(profile));
  }
}

function profileRow(profile) {
  const row = document.createElement("div");
  row.className = profile.active ? "profile is-active" : "profile";

  const main = document.createElement("div");
  main.className = "profile-main";

  const name = document.createElement("div");
  name.className = "profile-name";
  name.append(document.createTextNode(profile.name));
  if (profile.active) {
    const badge = document.createElement("span");
    badge.className = "badge";
    badge.textContent = "Active";
    name.append(badge);
  }

  const detail = document.createElement("div");
  detail.className = "profile-detail";
  detail.textContent = profile.summary;
  detail.title = profile.monitors.join(", ");

  main.append(name, detail);

  const actions = document.createElement("div");
  actions.className = "profile-actions";

  const hotkey = document.createElement("button");
  hotkey.className = profile.hotkey ? "hotkey" : "hotkey unset";
  hotkey.textContent = profile.hotkey ? prettyAccelerator(profile.hotkey) : "No hotkey";
  hotkey.title = "Set a global hotkey for this profile";
  hotkey.addEventListener("click", () => openHotkeyDialog(profile));

  const switchTo = document.createElement("button");
  switchTo.className = "primary";
  switchTo.textContent = "Switch";
  switchTo.disabled = profile.active;
  switchTo.addEventListener("click", () => applyProfile(profile.name, switchTo));

  const rename = document.createElement("button");
  rename.className = "subtle";
  rename.textContent = "Rename";
  rename.addEventListener("click", () => openRenameDialog(profile));

  const remove = document.createElement("button");
  remove.className = "subtle danger";
  remove.textContent = "Delete";
  remove.addEventListener("click", () => deleteProfile(profile.name));

  actions.append(hotkey, switchTo, rename, remove);
  row.append(main, actions);
  return row;
}

function renderMonitors(monitors) {
  const container = $("monitors");
  container.replaceChildren();

  if (!monitors.length) {
    const empty = document.createElement("div");
    empty.className = "empty";
    empty.textContent = "No monitors detected.";
    container.append(empty);
    return;
  }

  for (const monitor of monitors) {
    container.append(monitorRow(monitor));
  }
}

function monitorRow(monitor) {
  const row = document.createElement("div");
  row.className = monitor.active ? "monitor" : "monitor is-off";

  const main = document.createElement("div");
  main.className = "monitor-main";

  const model = document.createElement("div");
  model.className = "monitor-model";
  model.append(document.createTextNode(monitor.model));
  if (!monitor.active) {
    const badge = document.createElement("span");
    badge.className = "badge off";
    badge.textContent = "Off";
    model.append(badge);
  }

  // Resolution and position are how you tell two identical monitors apart:
  // the one at x=-2560 is the one on the left.
  const detail = document.createElement("div");
  detail.className = "monitor-detail";
  detail.textContent = monitor.active
    ? `${monitor.resolution} at ${monitor.position}`
    : "Connected, not in use";

  main.append(model, detail);

  const nickname = document.createElement("input");
  nickname.type = "text";
  nickname.className = "nickname";
  nickname.maxLength = 32;
  nickname.spellcheck = false;
  nickname.placeholder = "Nickname";
  nickname.value = monitor.nickname ?? "";

  let lastSaved = nickname.value;
  const save = async () => {
    const value = nickname.value.trim();
    if (value === lastSaved) return;
    try {
      await invoke("set_monitor_name", { key: monitor.key, nickname: value || null });
      lastSaved = value;
      toast(value ? `Named "${value}".` : "Nickname removed.");
      await refresh();
    } catch (e) {
      nickname.value = lastSaved;
      toast(String(e), "error");
    }
  };

  nickname.addEventListener("blur", save);
  nickname.addEventListener("keydown", (event) => {
    if (event.key === "Enter") nickname.blur();
    if (event.key === "Escape") {
      nickname.value = lastSaved;
      nickname.blur();
    }
  });

  row.append(main, nickname);
  return row;
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

/// Reload each section independently.
///
/// Deliberately not `Promise.all`: that rejects as soon as any one call fails,
/// which meant a single failing command left the whole window blank, profiles
/// included. Each section now renders if its own call succeeded, and a failure
/// says which one it was instead of showing a bare error.
async function refresh() {
  const sections = [
    ["profiles", "list_profiles", renderProfiles],
    ["current display", "current_status", renderStatus],
    ["monitors", "list_monitors", renderMonitors],
  ];

  const failures = [];

  await Promise.all(
    sections.map(async ([label, command, render]) => {
      try {
        render(await invoke(command));
      } catch (e) {
        console.error(`${command} failed`, e);
        failures.push(`${label}: ${e}`);
      }
    }),
  );

  if (failures.length) {
    toast(`Could not load ${failures.join("; ")}`, "error");
  }
}

async function applyProfile(name, button) {
  if (button) {
    button.disabled = true;
    button.textContent = "Switching…";
  }
  try {
    const message = await invoke("apply_profile", { name });
    toast(message, "success");
  } catch (e) {
    toast(String(e), "error");
  } finally {
    await refresh();
  }
}

async function deleteProfile(name) {
  // A profile is a few seconds of work to recreate, but deleting the wrong one
  // silently would be worse than one extra click.
  if (!window.confirm(`Delete the profile "${name}"?`)) return;
  try {
    await invoke("delete_profile", { name });
    toast(`Deleted ${name}.`);
  } catch (e) {
    toast(String(e), "error");
  }
  await refresh();
}

// ---------------------------------------------------------------------------
// Name dialog, shared by save and rename
// ---------------------------------------------------------------------------

let nameDialogSubmit = null;

function openNameDialog({ title, body, value, confirmLabel, onSubmit }) {
  $("name-dialog-title").textContent = title;
  $("name-dialog-body").textContent = body;
  $("name-confirm").textContent = confirmLabel;
  $("name-error").textContent = "";

  const input = $("name-input");
  input.value = value ?? "";

  nameDialogSubmit = onSubmit;
  $("name-dialog").hidden = false;
  input.focus();
  input.select();
}

function closeNameDialog() {
  $("name-dialog").hidden = true;
  nameDialogSubmit = null;
}

async function submitNameDialog() {
  if (!nameDialogSubmit) return;

  const name = $("name-input").value.trim();
  if (!name) {
    $("name-error").textContent = "Enter a name.";
    return;
  }

  const confirm = $("name-confirm");
  confirm.disabled = true;
  try {
    await nameDialogSubmit(name);
    closeNameDialog();
    await refresh();
  } catch (e) {
    $("name-error").textContent = String(e);
  } finally {
    confirm.disabled = false;
  }
}

function openSaveDialog() {
  openNameDialog({
    title: "Save current layout",
    body: "Give this monitor arrangement a name, such as Work or Play.",
    value: "",
    confirmLabel: "Save",
    onSubmit: async (name) => {
      try {
        await invoke("save_profile", { name, overwrite: false });
      } catch (e) {
        const message = String(e);
        if (!message.includes("already exists")) throw e;
        if (!window.confirm(`Replace the existing "${name}" profile?`)) return;
        await invoke("save_profile", { name, overwrite: true });
      }
      toast(`Saved ${name}.`, "success");
    },
  });
}

function openRenameDialog(profile) {
  openNameDialog({
    title: "Rename profile",
    body: `Choose a new name for "${profile.name}".`,
    value: profile.name,
    confirmLabel: "Rename",
    onSubmit: async (name) => {
      if (name === profile.name) return;
      await invoke("rename_profile", { from: profile.name, to: name });
      toast(`Renamed to ${name}.`);
    },
  });
}

// ---------------------------------------------------------------------------
// Hotkey dialog
// ---------------------------------------------------------------------------

let hotkeyTarget = null;
let capturedAccelerator = null;

/// Build an accelerator from a keydown event, or null if it is not usable.
///
/// Requires a modifier: a bare letter would be swallowed system-wide, which
/// would make the machine close to unusable.
function acceleratorFromEvent(event) {
  const modifiers = [];
  if (event.ctrlKey) modifiers.push("Control");
  if (event.altKey) modifiers.push("Alt");
  if (event.shiftKey) modifiers.push("Shift");
  if (event.metaKey) modifiers.push("Super");

  const code = event.code;
  const isModifierItself =
    code.startsWith("Control") ||
    code.startsWith("Alt") ||
    code.startsWith("Shift") ||
    code.startsWith("Meta") ||
    code.startsWith("OS");

  if (isModifierItself) return null;
  if (!modifiers.length) return { error: "Include Ctrl, Alt or Shift in the combination." };
  if (!code) return null;

  return { accelerator: [...modifiers, code].join("+") };
}

function openHotkeyDialog(profile) {
  hotkeyTarget = profile;
  capturedAccelerator = null;

  const capture = $("hotkey-capture");
  capture.textContent = profile.hotkey
    ? prettyAccelerator(profile.hotkey)
    : "Press a combination…";
  capture.className = profile.hotkey ? "hotkey-capture captured" : "hotkey-capture";

  $("hotkey-dialog-title").textContent = `Hotkey for ${profile.name}`;
  $("hotkey-error").textContent = "";
  $("hotkey-confirm").disabled = true;
  $("hotkey-clear").hidden = !profile.hotkey;

  $("hotkey-dialog").hidden = false;
  capture.focus();
}

function closeHotkeyDialog() {
  $("hotkey-dialog").hidden = true;
  hotkeyTarget = null;
  capturedAccelerator = null;
}

function onHotkeyKeydown(event) {
  event.preventDefault();
  event.stopPropagation();

  if (event.key === "Escape") {
    closeHotkeyDialog();
    return;
  }

  const result = acceleratorFromEvent(event);
  if (!result) return;

  if (result.error) {
    $("hotkey-error").textContent = result.error;
    return;
  }

  capturedAccelerator = result.accelerator;
  $("hotkey-error").textContent = "";
  const capture = $("hotkey-capture");
  capture.textContent = prettyAccelerator(capturedAccelerator);
  capture.className = "hotkey-capture captured";
  $("hotkey-confirm").disabled = false;
}

async function setHotkey(accelerator) {
  if (!hotkeyTarget) return;
  const name = hotkeyTarget.name;
  try {
    await invoke("set_hotkey", { name, accelerator });
    toast(accelerator ? `Hotkey assigned to ${name}.` : `Hotkey removed from ${name}.`);
    closeHotkeyDialog();
    await refresh();
  } catch (e) {
    $("hotkey-error").textContent = String(e);
  }
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

async function loadSettings() {
  try {
    const settings = await invoke("get_settings");
    $("check-updates").checked = settings.check_for_updates;
  } catch (e) {
    toast(String(e), "error");
  }

  try {
    $("autostart").checked = await invoke("get_autostart");
  } catch (e) {
    // Not fatal: the checkbox simply shows the wrong state until toggled.
    console.warn("could not read the autostart setting", e);
  }

  try {
    $("version").textContent = `Version ${await invoke("app_version")}`;
  } catch {
    $("version").textContent = "";
  }
}

async function checkForUpdates(button) {
  button.disabled = true;
  const original = button.textContent;
  button.textContent = "Checking…";
  try {
    const status = await invoke("check_for_update");
    if (status.available) {
      toast(`Version ${status.new_version} is available.`, "success");
    } else {
      toast(`You are on the latest version (${status.current_version}).`);
    }
  } catch (e) {
    toast(`Could not check for updates: ${e}`, "error");
  } finally {
    button.disabled = false;
    button.textContent = original;
  }
}

// ---------------------------------------------------------------------------
// Wiring
// ---------------------------------------------------------------------------

function wire() {
  $("save-new").addEventListener("click", openSaveDialog);

  $("name-cancel").addEventListener("click", closeNameDialog);
  $("name-confirm").addEventListener("click", submitNameDialog);
  $("name-input").addEventListener("keydown", (event) => {
    if (event.key === "Enter") submitNameDialog();
    if (event.key === "Escape") closeNameDialog();
  });

  $("hotkey-capture").addEventListener("keydown", onHotkeyKeydown);
  $("hotkey-cancel").addEventListener("click", closeHotkeyDialog);
  $("hotkey-clear").addEventListener("click", () => setHotkey(null));
  $("hotkey-confirm").addEventListener("click", () => setHotkey(capturedAccelerator));

  $("autostart").addEventListener("change", async (event) => {
    const enabled = event.target.checked;
    try {
      await invoke("set_autostart", { enabled });
      toast(enabled ? "Will start with Windows." : "Will no longer start with Windows.");
    } catch (e) {
      event.target.checked = !enabled;
      toast(String(e), "error");
    }
  });

  $("check-updates").addEventListener("change", async (event) => {
    await invoke("set_check_for_updates", { enabled: event.target.checked });
  });

  $("check-now").addEventListener("click", (event) => checkForUpdates(event.target));

  $("open-folder").addEventListener("click", async () => {
    try {
      await invoke("open_profiles_folder");
    } catch (e) {
      toast(String(e), "error");
    }
  });

  $("reset-config").addEventListener("click", async () => {
    if (
      !window.confirm(
        "Restore the layout Windows remembers for the monitors connected now?\n\n" +
          "Use this if a profile left your displays in an unusable state.",
      )
    ) {
      return;
    }
    try {
      await invoke("reset_display_config");
      toast("Restored.", "success");
    } catch (e) {
      toast(String(e), "error");
    }
    await refresh();
  });

  $("repo-link").addEventListener("click", (event) => {
    // Open in the real browser, not inside this window.
    event.preventDefault();
    invoke("open_repository").catch((e) => toast(String(e), "error"));
  });

  // Escape closes the window, matching how tray applications usually behave.
  document.addEventListener("keydown", (event) => {
    if (event.key !== "Escape") return;
    if (!$("name-dialog").hidden || !$("hotkey-dialog").hidden) return;
    invoke("hide_window");
  });

  // The tray and the hotkeys change profiles too; reload when they do.
  listen("profiles-changed", refresh);

  // Displays can also change outside this application entirely.
  window.addEventListener("focus", refresh);
}

wire();
loadSettings();
refresh();
