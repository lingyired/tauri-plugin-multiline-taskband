// Taskband settings popup — edits whichever taskband item opened it.
// The plugin emits `multiline-taskband://popup//open` (to this window only)
// with the instance's id and current state every time the popup opens, so
// every instance gets its own prefilled form.
//
// Interaction model: there are no Apply buttons — every control applies its
// plugin command as soon as it is committed (change / click), patches the
// shared localStorage store (see settings.js) and flashes "Saved" in the
// footer. Two-line commands always carry both lines' current values, since
// the API is per-property across both lines. The taskbar strip at the top is
// a live local preview (text, size, weight, family, color, alignment,
// padding) that follows the window's light / dark theme — the same theme the
// plugin's "system default" color mode follows.
import { saveInstanceState } from "./settings.js";

// Browser preview shim: opened as plain HTML (outside Tauri) the popup still
// renders for layout work — plugin calls become logged no-ops and a fake
// open event fills the form. Never active inside the real app.
if (!window.__TAURI__) {
  window.__TAURI__ = {
    core: {
      invoke: async (cmd, args) => {
        console.debug("[preview]", cmd, args?.payload ?? args);
        return null;
      },
    },
    event: {
      listen: async (_name, cb) => {
        setTimeout(() => {
          cb({
            payload: {
              id: "mb-3",
              top: "HOLDINGS",
              bottom: "+1.23%",
              topSize: 11,
              bottomSize: 12,
              topFontFamily: null,
              bottomFontFamily: null,
              leftPadding: 4,
              rightPadding: 4,
              topColor: { type: "default" },
              bottomColor: { type: "solid", value: "#18a058" },
              topBold: false,
              bottomBold: true,
              topAlign: 2,
              bottomAlign: 2,
              side: "right",
            },
          });
        }, 80);
        return async () => {};
      },
    },
  };
}

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

// Which taskband item opened this popup; mirror of its persisted state that
// every successful apply patches and saves back to the store.
let currentInstanceId = null;
let currentState = null;

// UI-only mirrors of the segmented controls (the source of truth while
// editing; synced from the open event and reset by "Reset appearance").
const colorMode = { top: "default", bottom: "default" };
const alignSel = { top: 0, bottom: 0 };

const ALIGN_NAMES = ["left", "center", "right"];
const HEX_RE = /^#?([0-9a-fA-F]{6})$/;

function persistState() {
  if (!currentInstanceId || !currentState) return;
  saveInstanceState(currentInstanceId, currentState);
}

// --- element refs -----------------------------------------------------------

const $ = (id) => document.getElementById(id);
const els = {
  id: $("pop-id"),
  sideDot: $("pop-side-dot"),
  saved: $("pop-saved"),
  simTop: $("sim-top"),
  simBottom: $("sim-bottom"),
  simChip: $("sim-chip"),
  topDot: $("top-dot"),
  bottomDot: $("bottom-dot"),
  top: $("popup-top"),
  bottom: $("popup-bottom"),
  topSize: $("popup-top-size"),
  topSizeVal: $("popup-top-size-value"),
  bottomSize: $("popup-bottom-size"),
  bottomSizeVal: $("popup-bottom-size-value"),
  topFamily: $("popup-top-family"),
  bottomFamily: $("popup-bottom-family"),
  topSolidRow: $("popup-top-solid-row"),
  bottomSolidRow: $("popup-bottom-solid-row"),
  topColor: $("popup-top-color"),
  topHex: $("popup-top-hex"),
  bottomColor: $("popup-bottom-color"),
  bottomHex: $("popup-bottom-hex"),
  topBold: $("popup-top-bold"),
  bottomBold: $("popup-bottom-bold"),
  padLeft: $("popup-pad-left"),
  padRight: $("popup-pad-right"),
};
const segGroups = {};
for (const name of ["cmode-top", "cmode-bottom", "align-top", "align-bottom", "side"]) {
  segGroups[name] = document.querySelector(`[data-group="${name}"]`);
}

function setPressed(group, value, attr) {
  if (!group) return;
  for (const btn of group.querySelectorAll("button")) {
    btn.setAttribute("aria-pressed", String(btn.dataset[attr] === String(value)));
  }
}

// --- live taskbar preview ----------------------------------------------------

// Effective color for a line: the hex field wins (it also receives the
// swatch picker's picks), falling back to the picker itself.
function resolveColor(line) {
  const hex = els[`${line}Hex`].value.trim();
  if (HEX_RE.test(hex)) return `#${hex.match(HEX_RE)[1]}`;
  return els[`${line}Color`].value;
}

function renderPreview() {
  const lines = [
    {
      el: els.simTop,
      dot: els.topDot,
      text: els.top.value,
      size: Number(els.topSize.value) || 11,
      bold: els.topBold.checked,
      family: els.topFamily.value.trim(),
      solid: colorMode.top === "solid",
      color: resolveColor("top"),
      align: alignSel.top,
    },
    {
      el: els.simBottom,
      dot: els.bottomDot,
      text: els.bottom.value,
      size: Number(els.bottomSize.value) || 11,
      bold: els.bottomBold.checked,
      family: els.bottomFamily.value.trim(),
      solid: colorMode.bottom === "solid",
      color: resolveColor("bottom"),
      align: alignSel.bottom,
    },
  ];
  for (const l of lines) {
    l.el.textContent = l.text || "\u00a0";
    // pt -> px at ~1.05, clamped so the preview strip keeps its shape
    l.el.style.fontSize = `${Math.min(Math.max(Math.round(l.size * 1.05), 8), 17)}px`;
    l.el.style.fontWeight = l.bold ? "700" : "400";
    l.el.style.textAlign = ALIGN_NAMES[l.align] || "left";
    l.el.style.fontFamily = l.family ? `'${l.family}'` : "";
    l.el.style.color = l.solid ? l.color : "var(--tb-ink)";
    l.dot.style.background = l.solid ? l.color : "var(--tb-ink)";
  }
  const left = Math.min(Math.max(parseInt(els.padLeft.value, 10) || 0, 0), 24);
  const right = Math.min(Math.max(parseInt(els.padRight.value, 10) || 0, 0), 24);
  els.simChip.style.padding = `3px ${right}px 3px ${left}px`;
}

// --- apply + saved flash ------------------------------------------------------

function flashSaved(ok = true) {
  els.saved.textContent = ok ? "Saved" : "Couldn't save — see the app log";
  els.saved.classList.toggle("err", !ok);
  if (ok) {
    els.saved.classList.remove("show");
    void els.saved.offsetWidth; // restart the fade on rapid applies
    els.saved.classList.add("show");
    clearTimeout(flashSaved._t);
    flashSaved._t = setTimeout(() => els.saved.classList.remove("show"), 1500);
  }
}

/** Run one plugin command for the current instance, patch state, persist. */
function apply(cmd, payload, patch) {
  if (!currentInstanceId || !currentState) return;
  invoke(`plugin:multiline-taskband|${cmd}`, { payload: { id: currentInstanceId, ...payload } })
    .then(() => {
      Object.assign(currentState, patch);
      persistState();
      flashSaved(true);
    })
    .catch((err) => {
      console.error(`${cmd} failed:`, err);
      flashSaved(false);
    });
}

const sizeVal = (el) => Number(el.value) || 11;
const padVal = (el) => {
  const v = parseInt(el.value, 10);
  return Number.isFinite(v) ? Math.min(Math.max(v, 0), 24) : 4;
};

const applyText = () =>
  apply(
    "set_text",
    { top: els.top.value, bottom: els.bottom.value },
    { top: els.top.value, bottom: els.bottom.value },
  );
const applySizes = () => {
  const top = sizeVal(els.topSize);
  const bottom = sizeVal(els.bottomSize);
  apply("set_font_sizes", { top, bottom }, { topSize: top, bottomSize: bottom });
};
const applyFamilies = () => {
  const top = els.topFamily.value.trim() || null;
  const bottom = els.bottomFamily.value.trim() || null;
  apply("set_font_family", { top, bottom }, { topFontFamily: top, bottomFontFamily: bottom });
};
const applyColors = () => {
  const top = colorMode.top === "solid" ? { type: "solid", value: resolveColor("top") } : { type: "default" };
  const bottom =
    colorMode.bottom === "solid" ? { type: "solid", value: resolveColor("bottom") } : { type: "default" };
  apply("set_colors", { top, bottom }, { topColor: top, bottomColor: bottom });
};
const applyBold = () => {
  const top = els.topBold.checked;
  const bottom = els.bottomBold.checked;
  apply("set_bold", { top, bottom }, { topBold: top, bottomBold: bottom });
};
const applyAlignment = () =>
  apply(
    "set_alignment",
    { top: alignSel.top, bottom: alignSel.bottom },
    { topAlign: alignSel.top, bottomAlign: alignSel.bottom },
  );
const applyPadding = () => {
  const left = padVal(els.padLeft);
  const right = padVal(els.padRight);
  apply("set_padding", { left, right }, { leftPadding: left, rightPadding: right });
};

// --- open-event fill -----------------------------------------------------------

function fill(p) {
  currentInstanceId = p.id;
  els.id.textContent = p.id;

  if (p.top !== undefined && p.top !== null) els.top.value = p.top;
  if (p.bottom !== undefined && p.bottom !== null) els.bottom.value = p.bottom;
  if (p.topSize !== undefined && p.topSize !== null) {
    els.topSize.value = p.topSize;
    els.topSizeVal.textContent = `${p.topSize} pt`;
  }
  if (p.bottomSize !== undefined && p.bottomSize !== null) {
    els.bottomSize.value = p.bottomSize;
    els.bottomSizeVal.textContent = `${p.bottomSize} pt`;
  }
  // Font family: null/absent = system font, shown as an empty input.
  els.topFamily.value = p.topFontFamily || "";
  els.bottomFamily.value = p.bottomFontFamily || "";

  if (p.leftPadding !== undefined && p.leftPadding !== null) els.padLeft.value = p.leftPadding;
  if (p.rightPadding !== undefined && p.rightPadding !== null) els.padRight.value = p.rightPadding;

  els.topBold.checked = !!p.topBold;
  els.bottomBold.checked = !!p.bottomBold;
  alignSel.top = Number(p.topAlign) || 0;
  alignSel.bottom = Number(p.bottomAlign) || 0;
  setPressed(segGroups["align-top"], alignSel.top, "align");
  setPressed(segGroups["align-bottom"], alignSel.bottom, "align");

  // Per-line color mode: a `solid` color selects "Custom" and pre-fills the
  // swatch/hex; anything else selects "System" and hides the row.
  for (const line of ["top", "bottom"]) {
    const color = line === "top" ? p.topColor : p.bottomColor;
    colorMode[line] = color && color.type === "solid" ? "solid" : "default";
    setPressed(segGroups[`cmode-${line}`], colorMode[line], "cmode");
    els[`${line}SolidRow`].hidden = colorMode[line] !== "solid";
    if (colorMode[line] === "solid") {
      if (/^#[0-9a-fA-F]{6}$/.test(color.value)) els[`${line}Color`].value = color.value;
      els[`${line}Hex`].value = color.value || "";
    }
  }

  if (p.side === "left" || p.side === "right") {
    setPressed(segGroups.side, p.side, "side");
    els.sideDot.dataset.side = p.side;
  }

  // Re-sync the store with the plugin's authoritative state so a failed apply
  // earlier can't leave stale settings behind.
  currentState = {
    side: p.side === "left" || p.side === "right" ? p.side : "right",
    top: p.top ?? null,
    bottom: p.bottom ?? null,
    topSize: p.topSize ?? 11,
    bottomSize: p.bottomSize ?? 11,
    topFontFamily: p.topFontFamily ?? null,
    bottomFontFamily: p.bottomFontFamily ?? null,
    leftPadding: p.leftPadding ?? 4,
    rightPadding: p.rightPadding ?? 4,
    topColor: p.topColor ?? { type: "default" },
    bottomColor: p.bottomColor ?? { type: "default" },
    topBold: !!p.topBold,
    bottomBold: !!p.bottomBold,
    topAlign: Number(p.topAlign) || 0,
    bottomAlign: Number(p.bottomAlign) || 0,
  };
  persistState();
  renderPreview();
}

// --- wiring ---------------------------------------------------------------------

window.addEventListener("DOMContentLoaded", () => {
  const lineNames = ["top", "bottom"];

  // Text: live preview while typing, apply on commit (Enter or blur).
  for (const line of lineNames) {
    const input = els[line];
    input.addEventListener("input", renderPreview);
    input.addEventListener("change", applyText);
  }

  // Font sizes: readout + preview while dragging, apply on release.
  for (const [line, size, val] of [
    ["top", els.topSize, els.topSizeVal],
    ["bottom", els.bottomSize, els.bottomSizeVal],
  ]) {
    size.addEventListener("input", () => {
      val.textContent = `${size.value} pt`;
      renderPreview();
    });
    size.addEventListener("change", applySizes);
  }

  // Font families: live preview while typing, apply on commit.
  for (const family of [els.topFamily, els.bottomFamily]) {
    family.addEventListener("input", renderPreview);
    family.addEventListener("change", applyFamilies);
  }

  // Color mode segmented buttons: switching reveals/hides the custom row and
  // immediately applies the line's mode.
  for (const line of lineNames) {
    segGroups[`cmode-${line}`].addEventListener("click", (e) => {
      const btn = e.target.closest("button[data-cmode]");
      if (!btn || btn.dataset.cmode === colorMode[line]) return;
      colorMode[line] = btn.dataset.cmode;
      setPressed(segGroups[`cmode-${line}`], colorMode[line], "cmode");
      els[`${line}SolidRow`].hidden = colorMode[line] !== "solid";
      applyColors();
      renderPreview();
    });

    // Swatch picker: mirrors into the hex field live; both apply on commit.
    els[`${line}Color`].addEventListener("input", () => {
      els[`${line}Hex`].value = els[`${line}Color`].value;
      renderPreview();
    });
    els[`${line}Color`].addEventListener("change", applyColors);

    // Hex field: normalise and apply valid values, revert invalid ones.
    els[`${line}Hex`].addEventListener("change", () => {
      const raw = els[`${line}Hex`].value.trim();
      const m = raw.match(HEX_RE);
      if (m) {
        els[`${line}Hex`].value = `#${m[1]}`;
        els[`${line}Color`].value = `#${m[1]}`;
        applyColors();
      } else {
        els[`${line}Hex`].value = els[`${line}Color`].value;
      }
      renderPreview();
    });
  }

  // Bold switches.
  for (const bold of [els.topBold, els.bottomBold]) {
    bold.addEventListener("change", () => {
      applyBold();
      renderPreview();
    });
  }

  // Alignment segmented buttons.
  for (const line of lineNames) {
    segGroups[`align-${line}`].addEventListener("click", (e) => {
      const btn = e.target.closest("button[data-align]");
      if (!btn) return;
      alignSel[line] = Number(btn.dataset.align);
      setPressed(segGroups[`align-${line}`], alignSel[line], "align");
      applyAlignment();
      renderPreview();
    });
  }

  // Side (left / right edge) — the header dot follows the tint.
  segGroups.side.addEventListener("click", (e) => {
    const btn = e.target.closest("button[data-side]");
    if (!btn) return;
    const side = btn.dataset.side;
    setPressed(segGroups.side, side, "side");
    els.sideDot.dataset.side = side;
    apply("set_side", { side }, { side });
  });

  // Padding.
  for (const pad of [els.padLeft, els.padRight]) {
    pad.addEventListener("change", () => {
      applyPadding();
      renderPreview();
    });
  }

  // Reset appearance to the plugin's defaults (text = instance id, 11 pt,
  // system font, system colors, no bold, left-aligned, 4 px padding). Side is
  // left alone — position is a separate concern.
  $("popup-reset").addEventListener("click", () => {
    if (!currentInstanceId || !currentState) return;
    const id = currentInstanceId;
    els.top.value = id;
    els.bottom.value = id;
    els.topSize.value = 11;
    els.bottomSize.value = 11;
    els.topSizeVal.textContent = "11 pt";
    els.bottomSizeVal.textContent = "11 pt";
    els.topFamily.value = "";
    els.bottomFamily.value = "";
    colorMode.top = "default";
    colorMode.bottom = "default";
    setPressed(segGroups["cmode-top"], "default", "cmode");
    setPressed(segGroups["cmode-bottom"], "default", "cmode");
    els.topSolidRow.hidden = true;
    els.bottomSolidRow.hidden = true;
    els.topBold.checked = false;
    els.bottomBold.checked = false;
    alignSel.top = 0;
    alignSel.bottom = 0;
    setPressed(segGroups["align-top"], 0, "align");
    setPressed(segGroups["align-bottom"], 0, "align");
    els.padLeft.value = 4;
    els.padRight.value = 4;
    renderPreview();

    Promise.all([
      invoke("plugin:multiline-taskband|set_text", { payload: { id, top: id, bottom: id } }),
      invoke("plugin:multiline-taskband|set_font_sizes", { payload: { id, top: 11, bottom: 11 } }),
      invoke("plugin:multiline-taskband|set_font_family", { payload: { id, top: null, bottom: null } }),
      invoke("plugin:multiline-taskband|set_colors", {
        payload: { id, top: { type: "default" }, bottom: { type: "default" } },
      }),
      invoke("plugin:multiline-taskband|set_bold", { payload: { id, top: false, bottom: false } }),
      invoke("plugin:multiline-taskband|set_alignment", { payload: { id, top: 0, bottom: 0 } }),
      invoke("plugin:multiline-taskband|set_padding", { payload: { id, left: 4, right: 4 } }),
    ])
      .then(() => {
        Object.assign(currentState, {
          top: id,
          bottom: id,
          topSize: 11,
          bottomSize: 11,
          topFontFamily: null,
          bottomFontFamily: null,
          leftPadding: 4,
          rightPadding: 4,
          topColor: { type: "default" },
          bottomColor: { type: "default" },
          topBold: false,
          bottomBold: false,
          topAlign: 0,
          bottomAlign: 0,
        });
        persistState();
        flashSaved(true);
      })
      .catch((err) => {
        console.error("Reset failed:", err);
        flashSaved(false);
      });
  });

  // Close: the header button, and Escape as a shortcut.
  const closePopup = () => {
    if (!currentInstanceId) return;
    invoke("plugin:multiline-taskband|close_popup", { payload: { id: currentInstanceId } }).catch(
      (err) => console.error("Failed to close popup:", err),
    );
  };
  $("popup-close").addEventListener("click", closePopup);
  window.addEventListener("keydown", (e) => {
    if (e.key === "Escape") closePopup();
  });

  // The plugin sends the instance id and its current state whenever the popup
  // opens. Re-render so each instance shows its own content.
  listen("multiline-taskband://popup//open", (event) => fill(event.payload)).catch((err) =>
    console.error("Failed to listen for popup open:", err),
  );

  renderPreview();
});
