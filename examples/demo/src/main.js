// Multiline Taskband demo frontend — mirrors the multiline-menubar demo's UX:
// a fixed set of preset instances, each listed as one compact row (name, text,
// side, settings, visibility). All per-instance appearance editing happens in
// the popup window opened from the taskbar item (or the row's Settings button).
//
// Persistence: every setting the user changes (global margin, per-instance
// text/appearance/side, shown/hidden, runtime-created instances) is written
// to localStorage right away (see settings.js) and re-applied to the plugin
// on the next launch, so the taskbar looks exactly like it did last session.
import {
  DEFAULT_MARGIN,
  PRESETS,
  instanceDefaults,
  loadSettings,
  saveSettings,
} from "./settings.js";

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
    // The popup may have changed text/side/appearance; re-read and refresh.
    refreshFromStorage();
    renderList();
    updateStatus();
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
  settings.customOrder = settings.customOrder.filter((x) => x !== id);
  persist();
  renderList();
  updateStatus();
}

// ---------------------------------------------------------------------------
// UI
// ---------------------------------------------------------------------------

function renderList() {
  const ul = document.querySelector("#instance-list");
  if (!ul) return;
  ul.innerHTML = "";
  for (const p of PRESETS) {
    if (created.has(p.id)) ul.appendChild(instanceRow(p.id, true));
  }
  for (const id of settings.customOrder) {
    if (created.has(id) && settings.instances[id]) {
      ul.appendChild(instanceRow(id, false));
    }
  }
}

function instanceRow(id, preset) {
  const s = settings.instances[id];
  if (!s) return document.createElement("li");

  const li = document.createElement("li");
  li.className = "instance-row";
  if (s.shown === false) li.classList.add("row-hidden");

  const name = document.createElement("span");
  name.className = "instance-name";
  name.textContent = id;

  const text = document.createElement("span");
  text.className = "instance-text muted";
  text.textContent = `"${s.top}" / "${s.bottom}"`;

  // left/right side switcher — taskband-specific (the menubar plugin has no
  // side concept; this is the left/right setting unique to the taskband).
  const side = document.createElement("select");
  side.className = "instance-side";
  side.title = "Left/right side of the taskbar";
  side.innerHTML = `
    <option value="left" ${s.side === "left" ? "selected" : ""}>left</option>
    <option value="right" ${s.side === "right" ? "selected" : ""}>right</option>
  `;
  side.addEventListener("change", (e) => {
    s.side = e.target.value;
    persist();
    invoke("plugin:multiline-taskband|set_side", {
      payload: { id, side: e.target.value },
    }).catch((err) => console.error(`set_side ${id}:`, err));
  });

  // Open this instance's settings popup without having to click the taskbar.
  const settingsBtn = document.createElement("button");
  settingsBtn.type = "button";
  settingsBtn.className = "settings-btn";
  settingsBtn.textContent = "Settings";
  settingsBtn.title = "Open this instance's settings popup";
  settingsBtn.addEventListener("click", () => {
    invoke("plugin:multiline-taskband|open_popup", {
      payload: { id },
    }).catch((err) => console.error(`open_popup ${id}:`, err));
  });

  const switchLabel = document.createElement("label");
  switchLabel.className = "switch";
  switchLabel.title = "Show / hide this taskbar item";
  const toggle = document.createElement("input");
  toggle.type = "checkbox";
  toggle.checked = s.shown !== false;
  toggle.addEventListener("change", () => setInstanceVisible(id, toggle.checked));
  const slider = document.createElement("span");
  slider.className = "slider";
  switchLabel.appendChild(toggle);
  switchLabel.appendChild(slider);

  li.appendChild(name);
  li.appendChild(text);
  li.appendChild(side);
  li.appendChild(settingsBtn);
  li.appendChild(switchLabel);

  // Runtime-created instances can be removed; the 5 presets are fixed.
  if (!preset) {
    const removeBtn = document.createElement("button");
    removeBtn.type = "button";
    removeBtn.className = "remove-btn";
    removeBtn.textContent = "✕";
    removeBtn.title = "Remove this instance (permanent)";
    removeBtn.addEventListener("click", () => removeInstance(id));
    li.appendChild(removeBtn);
  }
  return li;
}

function updateStatus() {
  const el = document.querySelector("#instance-status");
  if (!el) return;
  const visibleCount = [...created].filter((id) => {
    const s = settings.instances[id];
    return s ? s.shown !== false : false;
  }).length;
  el.textContent = `${created.size} instances · showing ${visibleCount} / hidden ${created.size - visibleCount}`;
}

function log(msg) {
  const el = document.querySelector("#click-log");
  if (el) el.textContent = msg;
}

// ---------------------------------------------------------------------------
// Boot
// ---------------------------------------------------------------------------

window.addEventListener("DOMContentLoaded", async () => {
  document.querySelector("#show-all-btn").addEventListener("click", () => setAllVisible(true));
  document.querySelector("#hide-all-btn").addEventListener("click", () => setAllVisible(false));

  // Pre-fill the persisted global margin and apply it on boot (only when it
  // differs from the plugin default, so a first run is a no-op).
  document.querySelector("#global-margin").value = settings.margin;
  if (settings.margin !== DEFAULT_MARGIN) {
    await invoke("plugin:multiline-taskband|set_margin", {
      payload: { margin: settings.margin },
    }).catch((err) => console.error("set_margin:", err));
  }

  document.querySelector("#margin-btn").addEventListener("click", () => {
    const v = parseInt(document.querySelector("#global-margin").value, 10);
    if (Number.isFinite(v)) {
      settings.margin = v;
      persist();
      invoke("plugin:multiline-taskband|set_margin", {
        payload: { margin: v },
      }).catch((err) => console.error("set_margin:", err));
    }
  });

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
    if (!settings.customOrder.includes(id)) settings.customOrder.push(id);
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

  // Create all presets up front (ordered: left first, then right), then any
  // runtime-created instances persisted from a previous session.
  for (const p of PRESETS) {
    const s = settings.instances[p.id] || instanceDefaults(p.id, p.side);
    await createInstance({ id: p.id, side: s.side, top: s.top, bottom: s.bottom }).catch((err) =>
      console.error(`Failed to create ${p.id}:`, err)
    );
  }
  for (const id of settings.customOrder) {
    const s = settings.instances[id];
    if (!s) continue;
    await createInstance({ id, side: s.side, top: s.top, bottom: s.bottom }).catch((err) =>
      console.error(`Failed to restore ${id}:`, err)
    );
  }
  renderList();
  updateStatus();
});
