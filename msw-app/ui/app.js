/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const appWindow = window.__TAURI__.window.getCurrentWindow();

const $ = (id) => document.getElementById(id);

/// Build an element in one line: el("div", "class", "text").
function el(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text != null) node.textContent = text;
  return node;
}

// Segoe Fluent Icons code points.
const GLYPH = {
  check: "",
  error: "",
  more: "",
  switch: "",
  rename: "",
  keyboard: "",
  save: "",
  delete: "",
  maximize: "",
  restore: "",
};

// ---------------------------------------------------------------------------
// Window chrome
// ---------------------------------------------------------------------------

/// Mica only exists on Windows 11. WebView2 reports Windows 11 as platform
/// version 13 or later; anything earlier keeps the solid background, because
/// the window is transparent and would otherwise show the desktop through it.
async function detectMica() {
  try {
    const { platformVersion } = await navigator.userAgentData.getHighEntropyValues([
      "platformVersion",
    ]);
    if (Number(platformVersion.split(".")[0]) >= 13) {
      document.documentElement.classList.add("mica");
    }
  } catch {
    // Keep the solid background.
  }
}

async function syncMaximizeGlyph() {
  try {
    const maximized = await appWindow.isMaximized();
    $("win-max-glyph").textContent = maximized ? GLYPH.restore : GLYPH.maximize;
    $("win-max").setAttribute("aria-label", maximized ? "Restore" : "Maximize");
  } catch {
    // Cosmetic only.
  }
}

function wireChrome() {
  $("win-min").addEventListener("click", () => appWindow.minimize());
  $("win-max").addEventListener("click", () => appWindow.toggleMaximize());
  // The application lives in the tray, so closing the window only hides it.
  $("win-close").addEventListener("click", () => invoke("hide_window"));
  appWindow.onResized(syncMaximizeGlyph);
  syncMaximizeGlyph();

  // The sandbox renames the window; show that in the title bar we draw.
  appWindow
    .title()
    .then((title) => {
      if (title) $("window-title").textContent = title;
    })
    .catch(() => {});
}

// ---------------------------------------------------------------------------
// Pages
// ---------------------------------------------------------------------------

function showPage(name) {
  document.querySelectorAll(".rail-item").forEach((item) => {
    if (item.dataset.page === name) {
      item.setAttribute("aria-current", "page");
    } else {
      item.removeAttribute("aria-current");
    }
  });
  document.querySelectorAll(".page").forEach((page) => {
    page.hidden = page.id !== `page-${name}`;
  });
  document.querySelector(".content").scrollTop = 0;
}

function wirePages() {
  document.querySelectorAll(".rail-item").forEach((item) => {
    item.addEventListener("click", () => showPage(item.dataset.page));
  });
}

// ---------------------------------------------------------------------------
// Toast
// ---------------------------------------------------------------------------

let toastTimer = null;

function toast(message, kind = "") {
  const box = $("toast");
  $("toast-text").textContent = message;
  $("toast-icon").textContent =
    kind === "success" ? GLYPH.check : kind === "error" ? GLYPH.error : "";
  box.className = `toast show ${kind}`;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => {
    box.className = `toast ${kind}`;
  }, kind === "error" ? 6000 : 2600);
}

// ---------------------------------------------------------------------------
// Overlays: dialogs and the guide share one stack, so Escape and Tab always
// act on whichever is on top.
// ---------------------------------------------------------------------------

const overlays = [];

function openOverlay(node, { onEscape, focus }) {
  overlays.push({ node, onEscape, returnFocus: document.activeElement });
  closeMenu();
  node.hidden = false;
  (focus ?? node.querySelector("input, button"))?.focus();
}

function closeOverlay(node) {
  const index = overlays.findIndex((o) => o.node === node);
  if (index === -1) return;
  const [entry] = overlays.splice(index, 1);
  node.hidden = true;
  if (entry.returnFocus?.isConnected) entry.returnFocus.focus();
}

function topOverlay() {
  return overlays[overlays.length - 1] ?? null;
}

/// Keep Tab inside the dialog on top.
function trapTab(event) {
  const top = topOverlay();
  if (!top) return;
  const focusable = [
    ...top.node.querySelectorAll("button, input, [tabindex='0']"),
  ].filter((n) => !n.disabled && n.offsetParent !== null);
  if (!focusable.length) return;
  const first = focusable[0];
  const last = focusable[focusable.length - 1];
  if (event.shiftKey && document.activeElement === first) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault();
    first.focus();
  } else if (!top.node.contains(document.activeElement)) {
    event.preventDefault();
    first.focus();
  }
}

/// A yes/no question in the app's own style, in place of window.confirm.
function ask({ title, body, confirmLabel, danger = false }) {
  return new Promise((resolve) => {
    const node = $("confirm-dialog");
    $("confirm-title").textContent = title;
    $("confirm-body").textContent = body;
    const ok = $("confirm-ok");
    ok.textContent = confirmLabel;
    ok.className = danger ? "btn danger" : "btn accent";

    const done = (answer) => {
      ok.onclick = null;
      $("confirm-cancel").onclick = null;
      closeOverlay(node);
      resolve(answer);
    };
    ok.onclick = () => done(true);
    $("confirm-cancel").onclick = () => done(false);

    // Focus the safe choice when the action destroys something.
    openOverlay(node, {
      onEscape: () => done(false),
      focus: danger ? $("confirm-cancel") : ok,
    });
  });
}

// ---------------------------------------------------------------------------
// Desk diagrams
// ---------------------------------------------------------------------------

const SVG_NS = "http://www.w3.org/2000/svg";

function svgEl(tag, attrs) {
  const node = document.createElementNS(SVG_NS, tag);
  for (const [key, value] of Object.entries(attrs)) node.setAttribute(key, value);
  return node;
}

/// Screens in reading order, left to right and then top to bottom, which is
/// the order a person numbers the monitors on their desk.
function readingOrder(screens) {
  return [...screens].sort((a, b) => a.x - b.x || a.y - b.y);
}

/// Windows puts the main display's top-left corner at the origin.
function isMain(screen) {
  return screen.x === 0 && screen.y === 0;
}

/// Draw monitors from above, each at its real relative position and aspect
/// ratio — the same picture Windows' own Display settings shows. The main
/// display carries a taskbar along its bottom edge.
///
/// `screens` is [{ label, x, y, width, height }] in desktop coordinates.
/// `frame` ({ width, height }) draws at a shared scale, so desks drawn side by
/// side compare honestly: one monitor looks smaller than three.
function renderDesk(box, screens, { lit = false, labels = false, frame = null } = {}) {
  box.classList.toggle("is-lit", lit);

  if (!screens.length) {
    box.replaceChildren();
    box.hidden = true;
    return;
  }
  box.hidden = false;

  const minX = Math.min(...screens.map((s) => s.x));
  const minY = Math.min(...screens.map((s) => s.y));
  const maxX = Math.max(...screens.map((s) => s.x + s.width));
  const maxY = Math.max(...screens.map((s) => s.y + s.height));
  const frameW = Math.max(frame?.width ?? 0, maxX - minX);
  const frameH = Math.max(frame?.height ?? 0, maxY - minY);
  const originX = (minX + maxX) / 2 - frameW / 2;
  const originY = (minY + maxY) / 2 - frameH / 2;
  const gap = Math.max(frameW, frameH) * 0.014;

  const svg = svgEl("svg", {
    viewBox: `${originX} ${originY} ${frameW} ${frameH}`,
    preserveAspectRatio: "xMidYMid meet",
    role: "img",
    "aria-label": screens.map((s) => s.label).join(", "),
  });

  readingOrder(screens).forEach((s, i) => {
    const x = s.x + gap / 2;
    const y = s.y + gap / 2;
    const w = s.width - gap;
    const h = s.height - gap;
    const short = Math.min(w, h);
    const radius = short * 0.05;

    const g = svgEl("g", { style: `--i: ${i}` });
    g.append(
      svgEl("rect", {
        x,
        y,
        width: w,
        height: h,
        rx: radius,
        class: "screen",
        "vector-effect": "non-scaling-stroke",
      }),
    );

    if (isMain(s)) {
      const inset = Math.max(radius * 0.6, short * 0.03);
      const bar = h * 0.075;
      g.append(
        svgEl("rect", {
          x: x + inset,
          y: y + h - inset - bar,
          width: w - inset * 2,
          height: bar,
          rx: bar / 2,
          class: "screen-taskbar",
        }),
      );
    }

    if (labels) {
      const numberSize = short * 0.3;
      const number = svgEl("text", {
        x: x + w / 2,
        y: y + h * 0.42,
        "font-size": numberSize,
        class: "screen-number",
      });
      number.textContent = String(i + 1);

      const nameSize = short * 0.13;
      const fits = Math.max(4, Math.floor((w * 0.86) / (nameSize * 0.56)));
      const name = svgEl("text", {
        x: x + w / 2,
        y: y + h * 0.42 + numberSize * 0.78,
        "font-size": nameSize,
        class: "screen-name",
      });
      name.textContent = s.label.length > fits ? `${s.label.slice(0, fits - 1)}…` : s.label;
      g.append(number, name);
    }

    const title = svgEl("title", {});
    title.textContent = `${s.label}\n${s.width} × ${s.height}${isMain(s) ? "\nMain display" : ""}`;
    g.append(title);

    svg.append(g);
  });

  box.replaceChildren(svg);
}

/// The smallest frame every one of these screen sets fits in.
function sharedFrame(sets) {
  let width = 0;
  let height = 0;
  for (const screens of sets) {
    if (!screens.length) continue;
    width = Math.max(width, Math.max(...screens.map((s) => s.x + s.width)) - Math.min(...screens.map((s) => s.x)));
    height = Math.max(height, Math.max(...screens.map((s) => s.y + s.height)) - Math.min(...screens.map((s) => s.y)));
  }
  return { width, height };
}

function newDesk() {
  return el("div", "desk");
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

let lastProfiles = [];
let lastStatus = null;
let lastMonitors = [];
let lastSettings = null;

/// Name of the profile on screen at the previous render, so a change can be
/// marked with the power-on animation. `undefined` until the first render,
/// which should not animate.
let previousActive;
let switching = null;

/// The monitors lit right now, as desk screens.
function currentScreens() {
  return lastMonitors
    .filter((m) => m.active && m.x != null && m.y != null && m.width && m.height)
    .map((m) => ({
      label: m.nickname || m.model,
      x: m.x,
      y: m.y,
      width: m.width,
      height: m.height,
      key: m.key,
    }));
}

// ---------------------------------------------------------------------------
// Profiles page
// ---------------------------------------------------------------------------

/// Turn a stored accelerator into its keys.
/// "Control+Alt+Digit1" becomes ["Ctrl", "Alt", "1"].
function acceleratorKeys(accelerator) {
  return accelerator.split("+").map((part) => {
    if (part === "Control" || part === "CommandOrControl") return "Ctrl";
    if (part === "Super" || part === "Meta") return "Win";
    if (part.startsWith("Digit")) return part.slice(5);
    if (part.startsWith("Key")) return part.slice(3);
    if (part.startsWith("Numpad")) return `Num ${part.slice(6)}`;
    if (part.startsWith("Arrow")) return part.slice(5);
    return part;
  });
}

function prettyAccelerator(accelerator) {
  return acceleratorKeys(accelerator).join("+");
}

function keycaps(accelerator) {
  return acceleratorKeys(accelerator).map((key) => el("kbd", "", key));
}

function renderProfiles() {
  const container = $("profiles");
  const active = lastProfiles.find((p) => p.active)?.name ?? null;
  const justLit = previousActive !== undefined && active !== previousActive ? active : null;
  previousActive = active;

  const screens = currentScreens();
  const tiles = [];
  tileFrame = sharedFrame([screens, ...lastProfiles.map((p) => p.screens)]);

  if (!lastProfiles.length) {
    container.replaceChildren(emptyState(screens));
    return;
  }

  if (!active && screens.length) {
    tiles.push(unsavedTile(screens));
  }

  for (const profile of lastProfiles) {
    tiles.push(profileTile(profile, { justLit: profile.name === justLit }));
  }

  container.replaceChildren(...tiles);
}

/// Shared by every tile in the grid; see `renderDesk`.
let tileFrame = null;

function tileShell({ screens, lit, state }) {
  const tile = el("article", "tile");
  const deskWrap = el("div", "tile-desk");
  const desk = newDesk();
  renderDesk(desk, screens, { lit, frame: tileFrame });
  deskWrap.append(desk);

  if (state) {
    const badge = el("span", "tile-state");
    badge.append(el("span", "led"), document.createTextNode(state));
    deskWrap.append(badge);
  }

  const body = el("div", "tile-body");
  const text = el("div", "tile-text");
  body.append(text);
  tile.append(deskWrap, body);
  return { tile, desk, deskWrap, body, text };
}

function profileTile(profile, { justLit = false, preview = false } = {}) {
  const { tile, desk, deskWrap, body, text } = tileShell({
    screens: profile.screens,
    lit: profile.active,
    state: profile.active ? "On screen" : null,
  });
  if (profile.active) tile.classList.add("is-active");
  if (switching === profile.name) tile.classList.add("is-switching");
  if (justLit) desk.classList.add("just-lit");

  const name = el("h3", "tile-name");
  const meta = el("p", "tile-meta");
  meta.textContent = profile.monitors.length ? profile.monitors.join(", ") : "No screens on";
  if (profile.off.length) {
    meta.append(el("span", "off", `${profile.off.length} off`));
  }
  meta.title = [
    `On: ${profile.monitors.join(", ") || "none"}`,
    profile.off.length ? `Off: ${profile.off.join(", ")}` : "",
  ]
    .filter(Boolean)
    .join("\n");

  if (preview) {
    name.textContent = profile.name;
    text.append(name, meta);
    return tile;
  }

  const switchTo = el("button", "tile-switch", profile.name);
  switchTo.title = profile.name;
  switchTo.setAttribute(
    "aria-label",
    profile.active ? `${profile.name}, on screen now` : `Switch to ${profile.name}`,
  );
  if (profile.active || switching) switchTo.setAttribute("aria-disabled", "true");
  switchTo.addEventListener("click", () => {
    if (profile.active || switching) return;
    applyProfile(profile.name);
  });
  name.append(switchTo);
  text.append(name, meta);

  if (profile.hotkey) {
    const keys = el("button", "tile-keys");
    keys.append(...keycaps(profile.hotkey));
    keys.title = "Change hotkey";
    keys.setAttribute("aria-label", `Hotkey ${prettyAccelerator(profile.hotkey)}. Change it`);
    keys.addEventListener("click", () => openHotkeyDialog(profile));
    deskWrap.append(keys);
  }

  const more = el("button", "icon-btn tile-more");
  more.append(glyph(GLYPH.more));
  more.setAttribute("aria-label", `More options for ${profile.name}`);
  more.setAttribute("aria-haspopup", "menu");
  more.addEventListener("click", () => openProfileMenu(profile, more));
  body.append(more);

  tile.addEventListener("contextmenu", (event) => {
    event.preventDefault();
    openProfileMenu(profile, null, { x: event.clientX, y: event.clientY });
  });

  return tile;
}

/// What is on screen when no profile matches it — the obvious next thing to
/// save, so it goes first.
function unsavedTile(screens) {
  const { tile, text, body } = tileShell({ screens, lit: true, state: "On screen" });
  tile.classList.add("is-unsaved");

  const name = el("h3", "tile-name", "Current layout");
  const meta = el("p", "tile-meta", "Not saved as a profile");
  text.append(name, meta);

  const save = el("button", "btn accent tile-save", "Save");
  save.setAttribute("aria-label", "Save the current layout as a profile");
  save.addEventListener("click", openSaveDialog);
  body.append(save);
  return tile;
}

function emptyState(screens) {
  const box = el("div", "tiles-empty");
  const desk = newDesk();
  renderDesk(desk, screens, { lit: true });

  const copy = el("div");
  copy.append(
    el("h3", "", "Save your first profile"),
    el(
      "p",
      "",
      "Set your screens up in Windows the way you like them, then save that layout here. You can switch back to it any time.",
    ),
  );
  const actions = el("div", "tiles-empty-actions");
  const save = el("button", "btn accent", "Save current layout");
  save.addEventListener("click", openSaveDialog);
  const guide = el("button", "btn", "Walk me through it");
  guide.addEventListener("click", openGuide);
  actions.append(save, guide);
  copy.append(actions);

  box.append(desk, copy);
  return box;
}

function glyph(code) {
  const span = el("span", "glyph", code);
  span.setAttribute("aria-hidden", "true");
  return span;
}

// ---------------------------------------------------------------------------
// Profile menu
// ---------------------------------------------------------------------------

let menuReturnFocus = null;

function openProfileMenu(profile, anchor, point) {
  const items = [];
  if (!profile.active) {
    items.push([GLYPH.switch, "Switch to this profile", () => applyProfile(profile.name)]);
  }
  items.push(
    [GLYPH.rename, "Rename", () => openRenameDialog(profile)],
    [
      GLYPH.keyboard,
      profile.hotkey ? "Change hotkey" : "Set hotkey",
      () => openHotkeyDialog(profile),
    ],
    [GLYPH.save, "Replace with current layout", () => replaceProfile(profile.name)],
    null,
    [GLYPH.delete, "Delete", () => deleteProfile(profile), "is-danger"],
  );
  openMenu(items, anchor, point);
}

function openMenu(items, anchor, point) {
  const menu = $("menu");
  menu.replaceChildren(
    ...items.map((item) => {
      if (!item) return el("div", "menu-sep");
      const [code, label, action, extra] = item;
      const button = el("button", `menu-item ${extra ?? ""}`);
      button.setAttribute("role", "menuitem");
      button.append(glyph(code), document.createTextNode(label));
      button.addEventListener("click", () => {
        closeMenu();
        action();
      });
      return button;
    }),
  );

  menuReturnFocus = anchor ?? document.activeElement;
  menu.hidden = false;

  // Below the anchor, right-aligned with it, or at the pointer; flipped to
  // stay inside the window.
  const { width, height } = menu.getBoundingClientRect();
  let x;
  let y;
  if (anchor) {
    const r = anchor.getBoundingClientRect();
    x = r.right - width;
    y = r.bottom + 4;
    if (y + height > window.innerHeight - 8) y = r.top - height - 4;
  } else {
    x = point.x;
    y = point.y;
    if (y + height > window.innerHeight - 8) y = point.y - height;
  }
  menu.style.left = `${Math.max(8, Math.min(x, window.innerWidth - width - 8))}px`;
  menu.style.top = `${Math.max(8, y)}px`;

  menu.querySelector(".menu-item")?.focus();
}

function closeMenu(restoreFocus = false) {
  const menu = $("menu");
  if (menu.hidden) return;
  menu.hidden = true;
  if (restoreFocus && menuReturnFocus?.isConnected) menuReturnFocus.focus();
  menuReturnFocus = null;
}

function onMenuKeydown(event) {
  const items = [...$("menu").querySelectorAll(".menu-item")];
  const index = items.indexOf(document.activeElement);
  const move = {
    ArrowDown: (index + 1) % items.length,
    ArrowUp: (index - 1 + items.length) % items.length,
    Home: 0,
    End: items.length - 1,
  }[event.key];

  if (move !== undefined) {
    event.preventDefault();
    items[move].focus();
  } else if (event.key === "Tab") {
    event.preventDefault();
    closeMenu(true);
  }
}

// ---------------------------------------------------------------------------
// Displays page
// ---------------------------------------------------------------------------

function renderDisplays() {
  const screens = currentScreens();
  renderDesk($("monitor-layout"), screens, { lit: true, labels: true });

  const line = [];
  if (lastStatus) {
    line.push(
      lastStatus.matching_profile
        ? `This is your ${lastStatus.matching_profile} profile.`
        : "This layout isn't saved as a profile.",
    );
    const off = lastStatus.inactive_monitors.length;
    if (off) line.push(`${off} more connected but turned off.`);
  } else if (!screens.length) {
    line.push("No screens are on.");
  }
  $("status-line").textContent = line.join(" ");

  const container = $("monitors");
  if (!lastMonitors.length) {
    const row = el("div", "row");
    row.append(el("span", "row-desc", "Windows isn't reporting any monitors."));
    container.replaceChildren(row);
    return;
  }

  // Lit screens in the same order as the diagram's numbers, then the rest.
  const numbered = readingOrder(screens);
  const numberOf = new Map(numbered.map((s, i) => [s.key, i + 1]));
  const ordered = [
    ...numbered.map((s) => lastMonitors.find((m) => m.key === s.key)),
    ...lastMonitors.filter((m) => !numberOf.has(m.key)),
  ];

  container.replaceChildren(...ordered.map((m) => monitorRow(m, numberOf.get(m.key))));
}

function monitorRow(monitor, number) {
  const row = el("div", "row");

  const badge = el("span", number ? "monitor-number" : "monitor-number is-off");
  if (number) badge.textContent = String(number);
  badge.setAttribute("aria-hidden", "true");

  const nickname = el("input", "field monitor-name");
  nickname.type = "text";
  nickname.maxLength = 32;
  nickname.spellcheck = false;
  nickname.placeholder = "Add a name";
  nickname.value = monitor.nickname ?? "";
  nickname.setAttribute("aria-label", `Name for ${monitor.model}`);

  let lastSaved = nickname.value;
  const save = async () => {
    const value = nickname.value.trim();
    if (value === lastSaved) return;
    try {
      await invoke("set_monitor_name", { key: monitor.key, nickname: value || null });
      lastSaved = value;
      toast(value ? `Named it ${value}.` : "Name removed.", "success");
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
      // Undo the edit, not close the window.
      event.stopPropagation();
      nickname.value = lastSaved;
      nickname.blur();
    }
  });

  const spec = el("div", "monitor-spec");
  spec.append(el("div", "model", monitor.model));
  spec.append(
    el(
      "div",
      "",
      monitor.active && monitor.resolution
        ? monitor.resolution.replace("x", " × ")
        : "Connected, turned off",
    ),
  );

  row.append(badge, nickname, spec);

  if (monitor.active && monitor.x === 0 && monitor.y === 0) {
    row.append(el("span", "tag", "Main display"));
  } else if (!monitor.active) {
    row.append(el("span", "tag is-standby", "Off"));
  }
  return row;
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

/// Reload everything, then redraw.
///
/// Each read is independent: one failing leaves the others to render, and
/// the error says which one it was instead of blanking the window.
async function refresh() {
  const reads = [
    ["profiles", "list_profiles", (v) => (lastProfiles = v)],
    ["current display", "current_status", (v) => (lastStatus = v)],
    ["monitors", "list_monitors", (v) => (lastMonitors = v)],
  ];

  const failures = [];
  await Promise.all(
    reads.map(async ([label, command, store]) => {
      try {
        store(await invoke(command));
      } catch (e) {
        console.error(`${command} failed`, e);
        failures.push(`${label}: ${e}`);
      }
    }),
  );

  renderProfiles();
  renderDisplays();
  renderGuideLive();

  if (failures.length) {
    toast(`Couldn't load ${failures.join("; ")}`, "error");
  }
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

async function applyProfile(name) {
  if (switching) return;
  switching = name;
  renderProfiles();
  try {
    const message = await invoke("apply_profile", { name });
    toast(`${message}.`, "success");
  } catch (e) {
    toast(String(e), "error");
  } finally {
    switching = null;
    await refresh();
  }
}

async function deleteProfile({ name, hotkey }) {
  const yes = await ask({
    title: `Delete ${name}?`,
    body: `${hotkey ? "The profile and its hotkey are removed." : "The profile is removed."} Your screens stay as they are.`,
    confirmLabel: "Delete",
    danger: true,
  });
  if (!yes) return;
  try {
    await invoke("delete_profile", { name });
    toast(`Deleted ${name}.`);
  } catch (e) {
    toast(String(e), "error");
  }
  await refresh();
}

async function replaceProfile(name) {
  const yes = await ask({
    title: `Replace ${name}?`,
    body: `${name} will switch to the layout on screen now. Its name and hotkey stay the same.`,
    confirmLabel: "Replace",
  });
  if (!yes) return;
  try {
    await invoke("save_profile", { name, overwrite: true });
    toast(`Replaced ${name}.`, "success");
  } catch (e) {
    toast(String(e), "error");
  }
  await refresh();
}

/// Save under a name, asking before replacing one that already exists.
/// Returns false if the user chose not to replace it.
async function saveAs(name) {
  try {
    await invoke("save_profile", { name, overwrite: false });
  } catch (e) {
    if (!String(e).includes("already exists")) throw e;
    const yes = await ask({
      title: `Replace ${name}?`,
      body: `You already have a profile called ${name}. Replace it with the layout on screen now?`,
      confirmLabel: "Replace",
    });
    if (!yes) return false;
    await invoke("save_profile", { name, overwrite: true });
  }
  return true;
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
  openOverlay($("name-dialog"), { onEscape: closeNameDialog, focus: input });
  input.select();
}

function closeNameDialog() {
  closeOverlay($("name-dialog"));
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
    const done = await nameDialogSubmit(name);
    if (done !== false) {
      closeNameDialog();
      await refresh();
    }
  } catch (e) {
    $("name-error").textContent = String(e);
  } finally {
    confirm.disabled = false;
  }
}

function openSaveDialog() {
  if (topOverlay()) return;
  showPage("profiles");
  openNameDialog({
    title: "Save current layout",
    body: "Name this arrangement after when you use it, like Work or Gaming.",
    value: "",
    confirmLabel: "Save",
    onSubmit: async (name) => {
      if (!(await saveAs(name))) return false;
      toast(`Saved ${name}.`, "success");
      return true;
    },
  });
}

function openRenameDialog(profile) {
  openNameDialog({
    title: `Rename ${profile.name}`,
    body: "Its hotkey moves with it.",
    value: profile.name,
    confirmLabel: "Rename",
    onSubmit: async (name) => {
      if (name === profile.name) return true;
      await invoke("rename_profile", { from: profile.name, to: name });
      toast(`Renamed to ${name}.`, "success");
      return true;
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
  if (!modifiers.length) return { error: "Add Ctrl, Alt or Shift to the combination." };
  if (!code) return null;

  return { accelerator: [...modifiers, code].join("+") };
}

function showCaptured(accelerator) {
  const capture = $("hotkey-capture");
  if (accelerator) {
    capture.replaceChildren(...keycaps(accelerator));
  } else {
    capture.textContent = "Press a key combination";
  }
}

function openHotkeyDialog(profile) {
  hotkeyTarget = profile;
  capturedAccelerator = null;

  showCaptured(profile.hotkey);
  $("hotkey-dialog-title").textContent = `Hotkey for ${profile.name}`;
  $("hotkey-error").textContent = "";
  $("hotkey-confirm").disabled = true;
  $("hotkey-clear").hidden = !profile.hotkey;

  openOverlay($("hotkey-dialog"), { onEscape: closeHotkeyDialog, focus: $("hotkey-capture") });
}

function closeHotkeyDialog() {
  closeOverlay($("hotkey-dialog"));
  hotkeyTarget = null;
  capturedAccelerator = null;
}

function onHotkeyKeydown(event) {
  // Plain Tab still moves between the dialog's controls.
  if (event.key === "Tab" && !event.ctrlKey && !event.altKey && !event.metaKey) return;

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
  showCaptured(capturedAccelerator);
  $("hotkey-confirm").disabled = false;
}

async function setHotkey(accelerator) {
  if (!hotkeyTarget) return;
  const name = hotkeyTarget.name;
  try {
    await invoke("set_hotkey", { name, accelerator });
    toast(
      accelerator
        ? `${prettyAccelerator(accelerator)} now switches to ${name}.`
        : `Removed the hotkey from ${name}.`,
      "success",
    );
    closeHotkeyDialog();
    await refresh();
  } catch (e) {
    $("hotkey-error").textContent = String(e);
  }
}

// ---------------------------------------------------------------------------
// First-run guide
// ---------------------------------------------------------------------------

const GUIDE_WELCOME = 0;
const GUIDE_ARRANGE = 1;
const GUIDE_SAVE = 2;
const GUIDE_DONE = 3;

let guideStep = GUIDE_WELCOME;
let guidePoll = null;

function guideOpen() {
  return !$("guide").hidden;
}

function openGuide() {
  if (guideOpen()) return;
  $("guide-name").value = "";
  $("guide-error").textContent = "";
  openOverlay($("guide"), { onEscape: closeGuide, focus: $("guide-next") });
  showGuideStep(GUIDE_WELCOME);
}

/// Close the guide and remember not to open it again on its own.
///
/// Skipping counts the same as finishing: someone who dismissed it once does
/// not want it back every launch, and it is one click away in Settings.
function closeGuide() {
  closeOverlay($("guide"));
  stopGuidePoll();
  invoke("set_onboarding_complete", { complete: true }).catch((e) =>
    console.warn("could not record that the guide was seen", e),
  );
}

function showGuideStep(step) {
  guideStep = step;

  document.querySelectorAll(".guide-page").forEach((page) => {
    page.hidden = Number(page.dataset.step) !== step;
  });
  document.querySelectorAll(".guide-steps li").forEach((item, i) => {
    item.className = i < step ? "is-done" : i === step ? "is-current" : "";
    if (i === step) {
      item.setAttribute("aria-current", "step");
    } else {
      item.removeAttribute("aria-current");
    }
  });

  const next = $("guide-next");
  next.disabled = false;
  next.textContent = ["Get started", "Looks right", "Save profile", "Done"][step];
  $("guide-back").hidden = step === GUIDE_WELCOME || step === GUIDE_DONE;
  $("guide-skip").hidden = step === GUIDE_DONE;

  // Windows does not tell this window when the arrangement changes, and
  // Display settings is usually beside it rather than on top, so the focus
  // refresh alone would leave the preview stale while the user works.
  if (step === GUIDE_ARRANGE) {
    startGuidePoll();
  } else {
    stopGuidePoll();
  }

  renderGuideLive();

  if (step === GUIDE_SAVE) {
    $("guide-name").focus();
  } else {
    next.focus();
  }
}

function startGuidePoll() {
  stopGuidePoll();
  refresh();
  guidePoll = setInterval(refresh, 2000);
}

function stopGuidePoll() {
  clearInterval(guidePoll);
  guidePoll = null;
}

function renderGuideLive() {
  if (!guideOpen()) return;

  const screens = currentScreens();
  const names = lastStatus?.active_monitors ?? [];
  const described = names.length ? names.join(", ") : "No screens are on";

  if (guideStep === GUIDE_WELCOME) {
    renderDesk($("guide-welcome-layout"), screens, { lit: true });
  }

  if (guideStep === GUIDE_ARRANGE) {
    renderDesk($("guide-layout"), screens, { lit: true, labels: true });
    $("guide-monitors").textContent = described;
  }

  if (guideStep === GUIDE_SAVE) {
    renderGuidePreview();
    $("guide-summary").textContent = lastStatus?.matching_profile
      ? `This is the same layout as your ${lastStatus.matching_profile} profile. Saving it under a new name makes a second copy.`
      : "";
  }
}

/// The tile this profile will get, drawn as the name is typed.
function renderGuidePreview() {
  const typed = $("guide-name").value.trim();
  const tile = profileTile(
    {
      name: typed || "Work",
      active: true,
      screens: currentScreens(),
      monitors: lastStatus?.active_monitors ?? [],
      off: lastStatus?.inactive_monitors ?? [],
      hotkey: null,
    },
    { preview: true },
  );
  tile.setAttribute("aria-hidden", "true");
  $("guide-preview").replaceChildren(tile);
}

async function guideNext() {
  if (guideStep === GUIDE_SAVE) {
    await guideSave();
    return;
  }
  if (guideStep === GUIDE_DONE) {
    closeGuide();
    return;
  }
  showGuideStep(guideStep + 1);
}

async function guideSave() {
  const name = $("guide-name").value.trim();
  if (!name) {
    $("guide-error").textContent = "Enter a name.";
    $("guide-name").focus();
    return;
  }

  const next = $("guide-next");
  next.disabled = true;
  $("guide-error").textContent = "";
  try {
    if (!(await saveAs(name))) return;
    $("guide-done-title").textContent = `${name} is saved`;
    showGuideStep(GUIDE_DONE);
    await refresh();
  } catch (e) {
    $("guide-error").textContent = String(e);
  } finally {
    next.disabled = false;
  }
}

function wireGuide() {
  $("guide-next").addEventListener("click", guideNext);
  $("guide-back").addEventListener("click", () => showGuideStep(Math.max(0, guideStep - 1)));
  $("guide-skip").addEventListener("click", closeGuide);
  $("show-guide").addEventListener("click", openGuide);

  $("guide-open-display").addEventListener("click", openDisplaySettings);

  $("guide-name").addEventListener("input", () => {
    $("guide-error").textContent = "";
    renderGuidePreview();
  });
  $("guide-name").addEventListener("keydown", (event) => {
    if (event.key !== "Enter") return;
    // Saving moves focus to the Done button; without this the same key press
    // goes on to click it and closes the guide before its last page is read.
    event.preventDefault();
    guideSave();
  });
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

function syncToggle(input) {
  input.closest(".toggle").querySelector(".toggle-state").textContent = input.checked
    ? "On"
    : "Off";
}

let appVersion = "";

async function loadSettings() {
  try {
    lastSettings = await invoke("get_settings");
    $("check-updates").checked = lastSettings.check_for_updates;
    syncToggle($("check-updates"));
    if (!lastSettings.onboarding_complete) openGuide();
  } catch (e) {
    toast(String(e), "error");
  }

  try {
    $("autostart").checked = await invoke("get_autostart");
  } catch (e) {
    // Not fatal: the switch simply shows the wrong state until used.
    console.warn("could not read the autostart setting", e);
  }
  syncToggle($("autostart"));

  try {
    appVersion = await invoke("app_version");
    $("version").textContent = `You have version ${appVersion}.`;
  } catch {
    $("version").textContent = "";
  }
}

let updateReady = false;

async function checkOrInstall(button) {
  button.disabled = true;

  if (updateReady) {
    button.textContent = "Installing…";
    try {
      // Restarts the application on success, so nothing after this runs.
      await invoke("install_update");
    } catch (e) {
      toast(`Couldn't install the update: ${e}`, "error");
      button.textContent = "Install and restart";
      button.disabled = false;
    }
    return;
  }

  button.textContent = "Checking…";
  try {
    const status = await invoke("check_for_update");
    if (status.available) {
      updateReady = true;
      $("version").textContent = `Version ${status.new_version} is ready to install. You have ${status.current_version}.`;
      button.textContent = "Install and restart";
      button.className = "btn accent";
    } else {
      $("version").textContent = `You have version ${status.current_version}, the latest.`;
      button.textContent = "Check now";
    }
  } catch (e) {
    toast(`Couldn't check for updates: ${e}`, "error");
    button.textContent = "Check now";
  } finally {
    button.disabled = false;
  }
}

function openDisplaySettings() {
  invoke("open_display_settings").catch((e) => toast(String(e), "error"));
}

// ---------------------------------------------------------------------------
// Wiring
// ---------------------------------------------------------------------------

function wire() {
  $("save-new").addEventListener("click", openSaveDialog);

  $("identify-monitors").addEventListener("click", async (event) => {
    const button = event.currentTarget;
    button.disabled = true;
    try {
      await invoke("identify_monitors");
    } catch (e) {
      toast(String(e), "error");
    } finally {
      setTimeout(() => {
        button.disabled = false;
      }, 1000);
    }
  });
  $("open-display-settings").addEventListener("click", openDisplaySettings);

  $("name-cancel").addEventListener("click", closeNameDialog);
  $("name-confirm").addEventListener("click", submitNameDialog);
  $("name-input").addEventListener("keydown", (event) => {
    if (event.key === "Enter") submitNameDialog();
  });
  $("name-input").addEventListener("input", () => {
    $("name-error").textContent = "";
  });

  $("hotkey-capture").addEventListener("keydown", onHotkeyKeydown);
  $("hotkey-cancel").addEventListener("click", closeHotkeyDialog);
  $("hotkey-clear").addEventListener("click", () => setHotkey(null));
  $("hotkey-confirm").addEventListener("click", () => setHotkey(capturedAccelerator));

  $("autostart").addEventListener("change", async (event) => {
    const input = event.target;
    const enabled = input.checked;
    syncToggle(input);
    try {
      await invoke("set_autostart", { enabled });
    } catch (e) {
      input.checked = !enabled;
      syncToggle(input);
      toast(String(e), "error");
    }
  });

  $("check-updates").addEventListener("change", async (event) => {
    syncToggle(event.target);
    await invoke("set_check_for_updates", { enabled: event.target.checked });
  });

  $("check-now").addEventListener("click", (event) => checkOrInstall(event.currentTarget));

  $("open-folder").addEventListener("click", async () => {
    try {
      await invoke("open_profiles_folder");
    } catch (e) {
      toast(String(e), "error");
    }
  });

  $("reset-config").addEventListener("click", async () => {
    const yes = await ask({
      title: "Restore the Windows layout?",
      body: "Your screens go back to the arrangement Windows last used for the monitors connected now. Your profiles aren't changed.",
      confirmLabel: "Restore",
    });
    if (!yes) return;
    try {
      await invoke("reset_display_config");
      toast("Restored the Windows layout.", "success");
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

  $("menu").addEventListener("keydown", onMenuKeydown);
  document.addEventListener("mousedown", (event) => {
    if (!$("menu").contains(event.target)) closeMenu();
  });
  window.addEventListener("blur", () => closeMenu());
  window.addEventListener("resize", () => closeMenu());
  document.querySelector(".content").addEventListener("scroll", () => closeMenu());

  document.addEventListener("keydown", (event) => {
    if (event.key === "Tab") {
      trapTab(event);
      return;
    }
    if (event.key !== "Escape") return;
    if (!$("menu").hidden) {
      closeMenu(true);
      return;
    }
    const top = topOverlay();
    if (top) {
      top.onEscape();
      return;
    }
    // Escape closes the window, matching how tray applications usually behave.
    invoke("hide_window");
  });

  // The tray and the hotkeys change profiles too; reload when they do.
  listen("profiles-changed", refresh);
  // "New profile..." in the tray menu.
  listen("open-save-dialog", openSaveDialog);

  // Displays can also change outside this application entirely.
  window.addEventListener("focus", refresh);
}

detectMica();
wireChrome();
wirePages();
wire();
wireGuide();
loadSettings();
refresh();
