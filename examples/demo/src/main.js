// Multiline Taskband demo frontend — a console for the preset instances plus
// any runtime-created ones. Each row renders the instance as the two-line
// chip it actually draws on the taskbar (real color / weight / size), with a
// side-tinted edge marker; all per-instance appearance editing happens in the
// popup window opened from the taskbar item (or the row's gear).
//
// Ordering: rows are drag-reorderable (grip handle, or ↑/↓ on it). The list
// sequence is the layout order per side — after a reorder the demo derives
// 0..n keys for the affected side and pushes them via set_order, which the
// plugin uses to relayout that side's instances.
//
// Persistence: every setting the user changes (global margin, per-instance
// text/appearance/side, shown/hidden, list order, runtime-created instances)
// is written to localStorage right away (see settings.js) and re-applied to
// the plugin on the next launch, so the taskbar looks exactly like it did
// last session.
import {
  DEFAULT_EDGE_MARGIN,
  DEFAULT_MARGIN,
  PRESETS,
  instanceDefaults,
  loadSettings,
  saveSettings,
} from "./settings.js";

// Browser preview shim: opened as plain HTML (outside Tauri) the console
// still renders with mocked plugin calls so layout work doesn't need Windows.
// Never active inside the real app.
if (!window.__TAURI__) {
  // Seed a believable demo state (first plain-browser visit only) so the
  // chip previews show real colors/weights instead of blank defaults.
  try {
    if (!window.localStorage.getItem("multiline-taskband-demo:settings-v1")) {
      window.localStorage.setItem(
        "multiline-taskband-demo:settings-v1",
        JSON.stringify({
          margin: 4,
          leftEdgeMargin: 0,
          rightEdgeMargin: 0,
          instances: {
            "mb-1": { side: "left", top: "HOLDINGS A", bottom: "+1.23%", bottomColor: { type: "solid", value: "#18a058" }, bottomBold: true },
            "mb-2": { side: "left", top: "HOLDINGS B", bottom: "-0.87%", bottomColor: { type: "solid", value: "#e5484d" } },
            "mb-3": { side: "right", top: "BTC", bottom: "¥412,300" },
            "mb-4": { side: "right", top: "QQQ", bottom: "318.42", topAlign: 2, bottomAlign: 2 },
            "mb-5": { side: "right", shown: false },
          },
          order: ["mb-1", "mb-2", "mb-3", "mb-4", "mb-5"],
        })
      );
    }
  } catch (_) {}
  window.__TAURI__ = {
    core: {
      invoke: async (cmd, args) => {
        console.debug("[preview]", cmd, args?.payload ?? args);
        return null;
      },
    },
    event: {
      listen: async () => async () => {},
    },
  };
}

window.addEventListener("error", (e) => { document.title = "ERR: " + e.message + " @L" + e.lineno; });
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

// ---------------------------------------------------------------------------
// 5 preset instances (mb-1 … mb-5): 2 on the left edge, 3 on the right edge.
// Each shows its own id on both lines, so we can talk about "mb-3" without
// ambiguity — same convention as the multiline-menubar demo. The list lives
// in settings.js so the popup's persistence path shares the same canon.
// ---------------------------------------------------------------------------

// Single source of truth for the demo's settings; saved to localStorage on
// every mutation and re-applied to the plugin on boot.
let settings = loadSettings(PRESETS);

const created = new Set(); // ids whose overlay window exists on the taskbar
const presetIds = new Set(PRESETS.map((p) => p.id));

// ---------------------------------------------------------------------------
// List order — the list sequence is the per-side layout order on the taskbar.
// The plugin sorts same-side instances by an ascending per-instance key
// (`set_order`); whenever a side's relative sequence changes in the list, the
// side's keys are re-derived (0..n) and pushed to the plugin.
// ---------------------------------------------------------------------------

function sideSequence(side) {
  return settings.order.filter(
    (id) => created.has(id) && settings.instances[id] && settings.instances[id].side === side,
  );
}

function applySideSequence(side) {
  sideSequence(side).forEach((id, index) => {
    invoke("plugin:multiline-taskband|set_order", { payload: { id, order: index } }).catch(
      (err) => console.error(`set_order ${id}:`, err),
    );
  });
}

/**
 * Apply one mutation to settings.order, then persist, re-render and resequence
 * every side whose relative order changed. `focusId` re-focuses that row's
 * grip afterwards so keyboard reordering can continue.
 */
function commitOrder(mutate, focusId) {
  const before = { left: sideSequence("left"), right: sideSequence("right") };
  if (mutate() === false) return;
  persist();
  renderList();
  updateStatus();
  const changed = [];
  for (const side of ["left", "right"]) {
    if (before[side].join("|") !== sideSequence(side).join("|")) {
      applySideSequence(side);
      changed.push(side);
    }
  }
  if (changed.length) {
    log(`Order updated — ${changed.join(" + ")} side resequenced on the taskbar`);
  }
  if (focusId) {
    const grip = document.querySelector(
      `.instance-row[data-id="${CSS.escape(focusId)}"] .grip-btn`,
    );
    if (grip) grip.focus();
  }
}

/** Swap a row with its neighbor in the list (keyboard reorder). */
function moveBy(id, delta) {
  const order = settings.order;
  const from = order.indexOf(id);
  const to = from + delta;
  if (from === -1 || to < 0 || to >= order.length) return false;
  [order[from], order[to]] = [order[to], order[from]];
  return true;
}

// --- drag & drop (rows only become draggable while the grip is held) --------

let dragState = null; // { id } while a grip-initiated drag is active
let dropTarget = null; // { id, place: "before" | "after" } from the last dragover

function clearDropIndicators() {
  for (const li of document.querySelectorAll(".instance-row")) {
    li.classList.remove("drop-above", "drop-below");
  }
}

function bindListDnd(ul) {
  ul.addEventListener("dragstart", (e) => {
    const li = e.target.closest?.(".instance-row");
    if (!li || !li.draggable) {
      e.preventDefault();
      return;
    }
    dragState = { id: li.dataset.id };
    e.dataTransfer.effectAllowed = "move";
    e.dataTransfer.setData("text/plain", dragState.id);
    li.classList.add("dragging");
  });

  ul.addEventListener("dragover", (e) => {
    if (!dragState) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
    clearDropIndicators();
    const li = e.target.closest?.(".instance-row");
    if (li && li.dataset.id === dragState.id) {
      dropTarget = null; // dropping back onto the dragged row: no-op
      return;
    }
    if (!li) {
      // below all rows: offer the end of the list
      const last = [...ul.querySelectorAll(".instance-row:not(.dragging)")].at(-1);
      dropTarget = last ? { id: last.dataset.id, place: "after" } : null;
      if (last) last.classList.add("drop-below");
      return;
    }
    const rect = li.getBoundingClientRect();
    const before = e.clientY < rect.top + rect.height / 2;
    li.classList.add(before ? "drop-above" : "drop-below");
    dropTarget = { id: li.dataset.id, place: before ? "before" : "after" };
  });

  ul.addEventListener("dragleave", (e) => {
    if (!dragState || ul.contains(e.relatedTarget)) return;
    clearDropIndicators();
    dropTarget = null;
  });

  ul.addEventListener("drop", (e) => {
    e.preventDefault();
    const target = dropTarget;
    clearDropIndicators();
    if (!dragState || !target || target.id === dragState.id) return;
    const src = dragState.id;
    commitOrder(() => {
      const order = settings.order;
      const from = order.indexOf(src);
      if (from === -1) return false;
      order.splice(from, 1);
      let to = order.indexOf(target.id);
      if (to === -1) return false;
      if (target.place === "after") to += 1;
      order.splice(to, 0, src);
      return true;
    });
  });

  ul.addEventListener("dragend", (e) => {
    const li = e.target.closest?.(".instance-row");
    if (li) {
      li.classList.remove("dragging");
      li.draggable = false;
    }
    dragState = null;
    dropTarget = null;
    clearDropIndicators();
  });
}

// ---------------------------------------------------------------------------
// Persistence helpers
// ---------------------------------------------------------------------------

function persist() {
  saveSettings(settings);
}

function refreshFromStorage() {
  // The popup window writes instance fields to the same store; re-read so the
  // list reflects what was changed there.
  settings = loadSettings(PRESETS);
}

// ---------------------------------------------------------------------------
// Instance lifecycle
// ---------------------------------------------------------------------------

async function createInstance(p) {
  // Register every event listener BEFORE creating the instance: the plugin's
  // UI thread can emit events right after the overlay is created, which would
  // beat a `listen` registered after `create`.
  await listen(`multiline-taskband://${p.id}//click`, (e) => {
    const pos = e.payload.position || {};
    log(`Click ${p.id} (${e.payload.button}) @ ${pos.x},${pos.y}`);
  }).catch(() => {});
  await listen(`multiline-taskband://${p.id}//popup-open`, () => {
    log(`${p.id} — settings popup opened`);
  }).catch(() => {});
  await listen(`multiline-taskband://${p.id}//popup-close`, () => {
    log(`${p.id} — settings popup closed`);
    // The popup may have changed text/side/appearance; re-read, refresh, and
    // re-apply the per-side layout keys (a side switch lands by creation key).
    refreshFromStorage();
    renderList();
    updateStatus();
    applySideSequence("left");
    applySideSequence("right");
  }).catch(() => {});
  await listen(`multiline-taskband://${p.id}//menu`, (e) => {
    log(`${p.id} menu: ${e.payload.itemId}`);
  }).catch(() => {});

  await invoke("plugin:multiline-taskband|create", {
    payload: { id: p.id, side: p.side, top: p.top, bottom: p.bottom },
  }).catch((err) => console.error(`create ${p.id}:`, err));
  created.add(p.id);

  // Right-click context menu (actions handled on the Rust side).
  await invoke("plugin:multiline-taskband|set_menu", {
    payload: {
      id: p.id,
      items: [
        { type: "item", id: "open-settings", text: "Open settings window" },
        { type: "separator" },
        { type: "item", id: "quit", text: "Quit app" },
      ],
    },
  }).catch((err) => console.error(`set_menu ${p.id}:`, err));

  // Re-apply the persisted appearance that differs from the plugin defaults.
  await applyAppearance(p.id);

  // Honor the persisted state: a previously hidden instance is created
  // invisible right away.
  const s = settings.instances[p.id];
  if (s && s.shown === false) {
    await invoke("plugin:multiline-taskband|set_visible", {
      payload: { id: p.id, visible: false },
    }).catch((err) => console.error(`set_visible failed for ${p.id}:`, err));
  }

  renderList();
  updateStatus();
}

/** Re-apply every persisted style that differs from the plugin's defaults. */
async function applyAppearance(id) {
  const s = settings.instances[id];
  if (!s) return;
  const jobs = [];
  if (s.topSize !== 11 || s.bottomSize !== 11) {
    jobs.push(
      invoke("plugin:multiline-taskband|set_font_sizes", {
        payload: { id, top: s.topSize, bottom: s.bottomSize },
      })
    );
  }
  if (s.topFontFamily || s.bottomFontFamily) {
    jobs.push(
      invoke("plugin:multiline-taskband|set_font_family", {
        payload: { id, top: s.topFontFamily, bottom: s.bottomFontFamily },
      })
    );
  }
  if (s.leftPadding !== 4 || s.rightPadding !== 4) {
    jobs.push(
      invoke("plugin:multiline-taskband|set_padding", {
        payload: { id, left: s.leftPadding, right: s.rightPadding },
      })
    );
  }
  const topSolid = s.topColor && s.topColor.type === "solid";
  const bottomSolid = s.bottomColor && s.bottomColor.type === "solid";
  if (topSolid || bottomSolid) {
    jobs.push(
      invoke("plugin:multiline-taskband|set_colors", {
        payload: {
          id,
          top: topSolid ? { type: "solid", value: s.topColor.value } : { type: "default" },
          bottom: bottomSolid ? { type: "solid", value: s.bottomColor.value } : { type: "default" },
        },
      })
    );
  }
  if (s.topBold || s.bottomBold) {
    jobs.push(
      invoke("plugin:multiline-taskband|set_bold", {
        payload: { id, top: !!s.topBold, bottom: !!s.bottomBold },
      })
    );
  }
  if (s.topAlign || s.bottomAlign) {
    jobs.push(
      invoke("plugin:multiline-taskband|set_alignment", {
        payload: { id, top: s.topAlign, bottom: s.bottomAlign },
      })
    );
  }
  if (s.topShown === false || s.bottomShown === false) {
    jobs.push(
      invoke("plugin:multiline-taskband|set_line_visible", {
        payload: { id, top: s.topShown !== false, bottom: s.bottomShown !== false },
      })
    );
  }
  await Promise.all(jobs).catch((err) => console.error(`applyAppearance ${id}:`, err));
}

async function setInstanceVisible(id, visible) {
  if (!settings.instances[id]) return;
  settings.instances[id].shown = visible;
  persist();
  if (created.has(id)) {
    await invoke("plugin:multiline-taskband|set_visible", {
      payload: { id, visible },
    }).catch((err) => console.error(`set_visible failed for ${id}:`, err));
  }
  renderList();
  updateStatus();
}

async function setAllVisible(visible) {
  for (const id of created) {
    await setInstanceVisible(id, visible);
  }
}

/** Permanently remove a runtime-created instance (presets stay). */
async function removeInstance(id) {
  if (!created.has(id)) return;
  await invoke("plugin:multiline-taskband|remove", {
    payload: { id },
  }).catch((err) => console.error(`remove ${id}:`, err));
  created.delete(id);
  delete settings.instances[id];
  settings.order = settings.order.filter((x) => x !== id);
  persist();
  renderList();
  updateStatus();
}

// ---------------------------------------------------------------------------
// UI
// ---------------------------------------------------------------------------

const ALIGN_NAMES = ["left", "center", "right"];
const GEAR_SVG = `<svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor"
  stroke-width="1.5" stroke-linecap="round" aria-hidden="true">
  <path d="M2.5 4.5h11M2.5 8h11M2.5 11.5h11"/>
  <path d="M10 2.7v3.6M5.5 6.2v3.6M11 9.7v3.6"/>
</svg>`;

function renderList() {
  const ul = document.querySelector("#instance-list");
  if (!ul) return;
  ul.innerHTML = "";
  for (const id of settings.order) {
    if (created.has(id) && settings.instances[id]) {
      ul.appendChild(instanceRow(id, presetIds.has(id)));
    }
  }
}

/** One line of the mini taskband chip, styled with the instance's settings. */
function chipLine(text, s, which) {
  const span = document.createElement("span");
  span.className = "chip-line";
  const size = which === "top" ? s.topSize : s.bottomSize;
  const bold = which === "top" ? s.topBold : s.bottomBold;
  const align = which === "top" ? s.topAlign : s.bottomAlign;
  const family = which === "top" ? s.topFontFamily : s.bottomFontFamily;
  const color = which === "top" ? s.topColor : s.bottomColor;
  span.textContent = text || "\u00a0";
  span.style.fontSize = `${Math.min(Math.max(Math.round(size * 1.05), 8), 13)}px`;
  span.style.fontWeight = bold ? "700" : "400";
  span.style.textAlign = ALIGN_NAMES[align] || "left";
  if (family) span.style.fontFamily = `'${family}'`;
  if (color && color.type === "solid") span.style.color = color.value;
  return span;
}

function colorDot(color, label) {
  const dot = document.createElement("span");
  dot.className = "color-dot";
  dot.title = label;
  if (color && color.type === "solid") dot.style.background = color.value;
  else dot.classList.add("sys");
  return dot;
}

const GRIP_SVG = `<svg viewBox="0 0 8 14" width="8" height="14" fill="currentColor" aria-hidden="true">
  <circle cx="2" cy="2" r="1.3"/><circle cx="6" cy="2" r="1.3"/>
  <circle cx="2" cy="7" r="1.3"/><circle cx="6" cy="7" r="1.3"/>
  <circle cx="2" cy="12" r="1.3"/><circle cx="6" cy="12" r="1.3"/>
</svg>`;

function instanceRow(id, preset) {
  const s = settings.instances[id];
  if (!s) return document.createElement("li");

  const li = document.createElement("li");
  li.className = "instance-row";
  li.dataset.id = id;
  li.dataset.side = s.side;
  if (s.shown === false) li.classList.add("row-hidden");

  // side-tinted edge marker (amber = Start side, teal = tray side)
  const marker = document.createElement("span");
  marker.className = "edge-marker";
  marker.setAttribute("aria-hidden", "true");

  // reorder grip: arms the row for native dragging while held; ↑/↓ moves it
  const grip = document.createElement("button");
  grip.type = "button";
  grip.className = "grip-btn";
  grip.title = "Drag to reorder — or press ↑ / ↓";
  grip.setAttribute("aria-label", `Reorder ${id}`);
  grip.innerHTML = GRIP_SVG;
  grip.addEventListener("mousedown", () => {
    li.draggable = true;
    window.addEventListener("mouseup", () => {
      li.draggable = false;
    }, { once: true });
  });
  grip.addEventListener("keydown", (e) => {
    if (e.key !== "ArrowUp" && e.key !== "ArrowDown") return;
    e.preventDefault();
    commitOrder(() => moveBy(id, e.key === "ArrowUp" ? -1 : 1), id);
  });

  // the instance rendered as its own two-line taskband chip
  const chip = document.createElement("span");
  chip.className = "taskband-chip";
  chip.setAttribute("aria-hidden", "true");
  chip.title = `"${s.top}" / "${s.bottom}"`;
  chip.style.paddingLeft = `${Math.min(s.leftPadding ?? 4, 12)}px`;
  chip.style.paddingRight = `${Math.min(s.rightPadding ?? 4, 12)}px`;
  // hidden lines render nothing — matching the taskband, where the chip
  // shrinks to the remaining line (both off = no chip at all)
  if (s.topShown !== false) chip.appendChild(chipLine(s.top, s, "top"));
  if (s.bottomShown !== false) chip.appendChild(chipLine(s.bottom, s, "bottom"));

  const info = document.createElement("div");
  info.className = "instance-info";
  const name = document.createElement("span");
  name.className = "instance-name";
  name.textContent = id;
  const meta = document.createElement("span");
  meta.className = "instance-meta";
  const sideLabel = document.createElement("span");
  sideLabel.textContent = s.side;
  const sizeLabel = document.createElement("span");
  sizeLabel.className = "mono";
  sizeLabel.textContent = `${s.topSize}/${s.bottomSize} pt`;
  meta.append(
    sideLabel,
    document.createTextNode("·"),
    sizeLabel,
    document.createTextNode("·"),
    colorDot(s.topColor, "Top line color"),
    colorDot(s.bottomColor, "Bottom line color"),
  );
  info.appendChild(name);
  info.appendChild(meta);

  const controls = document.createElement("div");
  controls.className = "instance-controls";

  // left/right side switcher — taskband-specific (the menubar plugin has no
  // side concept; this is the left/right setting unique to the taskband).
  const seg = document.createElement("div");
  seg.className = "seg sm";
  seg.setAttribute("role", "group");
  seg.setAttribute("aria-label", `Side of ${id}`);
  for (const sideVal of ["left", "right"]) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.textContent = sideVal === "left" ? "L" : "R";
    btn.title =
      sideVal === "left" ? "Left edge (next to Start)" : "Right edge (next to the tray)";
    btn.setAttribute("aria-pressed", String(s.side === sideVal));
    btn.addEventListener("click", () => {
      if (s.side === sideVal) return;
      s.side = sideVal;
      persist();
      invoke("plugin:multiline-taskband|set_side", {
        payload: { id, side: sideVal },
      })
        .then(() => {
          // a side switch lands by creation key — resequence both sides so
          // the taskbar keeps following the list order
          applySideSequence("left");
          applySideSequence("right");
        })
        .catch((err) => console.error(`set_side ${id}:`, err));
      renderList();
    });
    seg.appendChild(btn);
  }

  // Open this instance's settings popup without having to click the taskbar.
  const settingsBtn = document.createElement("button");
  settingsBtn.type = "button";
  settingsBtn.className = "icon-btn settings-btn";
  settingsBtn.title = "Open this instance's settings popup";
  settingsBtn.setAttribute("aria-label", `Settings for ${id}`);
  settingsBtn.innerHTML = GEAR_SVG;
  settingsBtn.addEventListener("click", () => {
    invoke("plugin:multiline-taskband|open_popup", {
      payload: { id },
    }).catch((err) => console.error(`open_popup ${id}:`, err));
  });

  const switchLabel = document.createElement("label");
  switchLabel.className = "switch";
  switchLabel.title = "Show / hide this taskband item";
  const toggle = document.createElement("input");
  toggle.type = "checkbox";
  toggle.checked = s.shown !== false;
  toggle.setAttribute("aria-label", `Show ${id}`);
  toggle.addEventListener("change", () => setInstanceVisible(id, toggle.checked));
  const slider = document.createElement("span");
  slider.className = "slider";
  switchLabel.appendChild(toggle);
  switchLabel.appendChild(slider);

  controls.appendChild(seg);
  controls.appendChild(settingsBtn);
  controls.appendChild(switchLabel);

  // Runtime-created instances can be removed; the 5 presets are fixed.
  if (!preset) {
    const removeBtn = document.createElement("button");
    removeBtn.type = "button";
    removeBtn.className = "icon-btn danger remove-btn";
    removeBtn.title = "Remove this instance (permanent)";
    removeBtn.setAttribute("aria-label", `Remove ${id}`);
    removeBtn.textContent = "✕";
    removeBtn.addEventListener("click", () => removeInstance(id));
    controls.appendChild(removeBtn);
  }

  li.appendChild(marker);
  li.appendChild(grip);
  li.appendChild(chip);
  li.appendChild(info);
  li.appendChild(controls);
  return li;
}

function updateStatus() {
  const el = document.querySelector("#instance-status");
  if (!el) return;
  const visibleCount = [...created].filter((id) => {
    const s = settings.instances[id];
    return s ? s.shown !== false : false;
  }).length;
  el.textContent = `${created.size} items · ${visibleCount} shown · ${created.size - visibleCount} hidden`;
}

let logTimer = null;
function log(msg) {
  const el = document.querySelector("#click-log");
  if (!el) return;
  el.textContent = msg;
  // flash the status dot so it reads as live activity, not decoration
  const dot = document.querySelector("#status-dot");
  if (dot) {
    dot.classList.add("live");
    clearTimeout(logTimer);
    logTimer = setTimeout(() => dot.classList.remove("live"), 1200);
  }
}

// ---------------------------------------------------------------------------
// Boot
// ---------------------------------------------------------------------------

window.addEventListener("DOMContentLoaded", async () => {
  bindListDnd(document.querySelector("#instance-list"));

  document.querySelector("#show-all-btn").addEventListener("click", () => setAllVisible(true));
  document.querySelector("#hide-all-btn").addEventListener("click", () => setAllVisible(false));

  // Pre-fill the persisted margins; like everything else in this console they
  // apply as soon as a value is committed (Enter, blur, or the spinners).
  document.querySelector("#global-margin").value = settings.margin;
  if (settings.margin !== DEFAULT_MARGIN) {
    await invoke("plugin:multiline-taskband|set_margin", {
      payload: { margin: settings.margin },
    }).catch((err) => console.error("set_margin:", err));
  }

  const bindMargin = (sel, onCommit) => {
    document.querySelector(sel).addEventListener("change", (e) => {
      const v = parseInt(e.target.value, 10);
      if (Number.isFinite(v)) onCommit(v);
    });
  };
  bindMargin("#global-margin", (v) => {
    settings.margin = v;
    persist();
    invoke("plugin:multiline-taskband|set_margin", {
      payload: { margin: v },
    }).catch((err) => console.error("set_margin:", err));
  });
  bindMargin("#edge-margin-left", (v) => {
    settings.leftEdgeMargin = v;
    persist();
    invoke("plugin:multiline-taskband|set_edge_margins", {
      payload: { left: v, right: settings.rightEdgeMargin },
    }).catch((err) => console.error("set_edge_margins:", err));
  });
  bindMargin("#edge-margin-right", (v) => {
    settings.rightEdgeMargin = v;
    persist();
    invoke("plugin:multiline-taskband|set_edge_margins", {
      payload: { left: settings.leftEdgeMargin, right: v },
    }).catch((err) => console.error("set_edge_margins:", err));
  });

  // Pre-apply the persisted edge margins on boot (only when either differs
  // from the plugin default, so a first run is a no-op).
  if (
    settings.leftEdgeMargin !== DEFAULT_EDGE_MARGIN ||
    settings.rightEdgeMargin !== DEFAULT_EDGE_MARGIN
  ) {
    await invoke("plugin:multiline-taskband|set_edge_margins", {
      payload: { left: settings.leftEdgeMargin, right: settings.rightEdgeMargin },
    }).catch((err) => console.error("set_edge_margins:", err));
  }

  document.querySelector("#create-btn").addEventListener("click", async () => {
    const id = document.querySelector("#new-id").value.trim();
    if (!id) return;
    if (created.has(id)) {
      log(`Instance "${id}" already exists`);
      return;
    }
    const side = document.querySelector("#new-side").value;
    const top = document.querySelector("#new-top").value.trim() || id;
    const bottom = document.querySelector("#new-bottom").value.trim() || id;
    settings.instances[id] = {
      ...instanceDefaults(id, side),
      top,
      bottom,
      shown: true,
    };
    if (!settings.order.includes(id)) settings.order.push(id);
    persist();
    await createInstance({ id, side, top, bottom }).catch((err) =>
      console.error(`Failed to create ${id}:`, err)
    );
    document.querySelector("#new-id").value = "";
    document.querySelector("#new-top").value = "";
    document.querySelector("#new-bottom").value = "";
    renderList();
    updateStatus();
  });

  // The settings popup window is declared in tauri.conf.json (label "popup").
  // Register it with the plugin and keep auto-popup on left click enabled —
  // hosts must call set_popup_window before the first click.
  await invoke("plugin:multiline-taskband|set_popup_window", {
    payload: { label: "popup" },
  }).catch((err) => console.error("set_popup_window:", err));
  await invoke("plugin:multiline-taskband|set_auto_popup", {
    payload: { enabled: true },
  }).catch(() => {});

  // Create everything in the persisted list order — creation order is the
  // plugin's default per-side layout key, so this boot already matches the
  // list without extra set_order calls.
  for (const id of settings.order) {
    const s = settings.instances[id];
    if (!s) continue;
    await createInstance({ id, side: s.side, top: s.top, bottom: s.bottom }).catch((err) =>
      console.error(`Failed to create ${id}:`, err)
    );
  }
  renderList();
  updateStatus();
});
