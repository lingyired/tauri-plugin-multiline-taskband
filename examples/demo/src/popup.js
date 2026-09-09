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
import { ICON_PRESETS } from "./iconPresets.js";

// Presets keyed by id for the icon dropdown; the dropdown itself is filled in
// from ICON_PRESETS at startup (see popup.html for the static "No icon" entry).
const presetById = new Map(ICON_PRESETS.map((p) => [p.id, p]));

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
              topVisible: true,
              bottomVisible: true,
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

// Alignment maps straight onto flex justify-content for the preview rows
// (each row is an icon + text group that moves as one unit).
const ALIGN_JUSTIFY = ["flex-start", "center", "flex-end"];
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
  simTopIcon: $("sim-top-icon"),
  simTopText: $("sim-top-text"),
  simBottomIcon: $("sim-bottom-icon"),
  simBottomText: $("sim-bottom-text"),
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
  topIconPreset: $("popup-top-icon-preset"),
  topIconPreview: $("popup-top-icon-preview"),
  topIconTint: $("popup-top-icon-tint"),
  bottomIconPreset: $("popup-bottom-icon-preset"),
  bottomIconPreview: $("popup-bottom-icon-preview"),
  bottomIconTint: $("popup-bottom-icon-tint"),
  leadingPreset: $("popup-leading-preset"),
  leadingPreview: $("popup-leading-preview"),
  leadingTint: $("popup-leading-tint"),
  leadingSize: $("popup-leading-size"),
  simLeading: $("sim-leading-icon"),
  topSolidRow: $("popup-top-solid-row"),
  bottomSolidRow: $("popup-bottom-solid-row"),
  topColor: $("popup-top-color"),
  topHex: $("popup-top-hex"),
  bottomColor: $("popup-bottom-color"),
  bottomHex: $("popup-bottom-hex"),
  topBold: $("popup-top-bold"),
  bottomBold: $("popup-bottom-bold"),
  topShown: $("popup-top-shown"),
  bottomShown: $("popup-bottom-shown"),
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
  // Resolve the taskbar ink once per paint — the CSS variable flips with the
  // light/dark theme and tinted icons/`default` text must track it.
  const tbInk =
    getComputedStyle(document.documentElement).getPropertyValue("--tb-ink").trim() || "#1a1a1a";
  const lines = [
    {
      el: els.simTop,
      textEl: els.simTopText,
      iconEl: els.simTopIcon,
      dot: els.topDot,
      shown: els.topShown.checked,
      text: els.top.value,
      size: Number(els.topSize.value) || 11,
      bold: els.topBold.checked,
      family: els.topFamily.value.trim(),
      solid: colorMode.top === "solid",
      color: resolveColor("top"),
      align: alignSel.top,
      preset: presetById.get(els.topIconPreset.value) ?? null,
      tint: els.topIconTint.checked,
    },
    {
      el: els.simBottom,
      textEl: els.simBottomText,
      iconEl: els.simBottomIcon,
      dot: els.bottomDot,
      shown: els.bottomShown.checked,
      text: els.bottom.value,
      size: Number(els.bottomSize.value) || 11,
      bold: els.bottomBold.checked,
      family: els.bottomFamily.value.trim(),
      solid: colorMode.bottom === "solid",
      color: resolveColor("bottom"),
      align: alignSel.bottom,
      preset: presetById.get(els.bottomIconPreset.value) ?? null,
      tint: els.bottomIconTint.checked,
    },
  ];
  for (const l of lines) {
    // A hidden line vanishes from the strip — on the taskbar the instance
    // shrinks to the remaining line, which the strip shows naturally.
    l.el.style.display = l.shown ? "" : "none";
    // pt -> px at ~1.05, clamped so the preview strip keeps its shape
    const sizePx = Math.min(Math.max(Math.round(l.size * 1.05), 8), 17);
    // Line colour: `solid` wins, otherwise the taskbar ink (system colour).
    const ink = l.solid ? l.color : tbInk;
    // Leading icon (one group with the text): monochrome when Tint is on —
    // every paint is repainted in the line colour, mirroring the plugin
    // (alpha becomes coverage, pixels take the line colour).
    if (l.shown && l.preset) {
      l.iconEl.hidden = false;
      l.iconEl.style.height = `${Math.round(sizePx * 1.2)}px`;
      l.iconEl.src = iconPreviewUrl(l.preset, l.tint ? ink : null);
    } else {
      l.iconEl.hidden = true;
      l.iconEl.removeAttribute("src");
    }
    l.textEl.textContent = l.text || "\u00a0";
    l.textEl.style.fontSize = `${sizePx}px`;
    l.textEl.style.fontWeight = l.bold ? "700" : "400";
    l.textEl.style.fontFamily = l.family ? `'${l.family}'` : "";
    l.textEl.style.color = ink;
    l.el.style.justifyContent = ALIGN_JUSTIFY[l.align] || "flex-start";
    l.dot.style.background = ink;
    l.dot.style.opacity = l.shown ? "" : "0.3";
  }
  // Leading column icon: spans the whole block (or an explicit clamped
  // height), vertically centred — mirrors the plugin's column layout. Tint
  // follows the first visible line's ink.
  const leadPreset = presetById.get(els.leadingPreset.value) ?? null;
  if (leadPreset) {
    const shownLines = lines.filter((l) => l.shown);
    const blockH = shownLines.length
      ? shownLines.reduce(
          (acc, l) => acc + Math.min(Math.max(Math.round(l.size * 1.05), 8), 17),
          0,
        ) + (shownLines.length - 1) * 3
      : 17;
    const leadSize = parseInt(els.leadingSize.value, 10) || 0;
    const h = leadSize > 0 ? Math.min(Math.max(leadSize, 8), blockH) : blockH;
    const first = shownLines[0] ?? lines[0];
    const leadInk = first.solid ? first.color : tbInk;
    els.simLeading.hidden = false;
    els.simLeading.style.height = `${h}px`;
    els.simLeading.src = iconDataUrl(leadPreset.svg, els.leadingTint.checked ? leadInk : null);
  } else {
    els.simLeading.hidden = true;
    els.simLeading.removeAttribute("src");
  }
  const left = Math.min(Math.max(parseInt(els.padLeft.value, 10) || 0, 0), 24);
  const right = Math.min(Math.max(parseInt(els.padRight.value, 10) || 0, 0), 24);
  els.simChip.style.padding = `3px ${right}px 3px ${left}px`;
}

/**
 * Build a data: URL for a preset, optionally repainting it in `paint` — the
 * browser-side stand-in for the plugin's tint.
 *
 *   - SVG preset: rewrite every opaque `fill`/`stroke` to `paint` (alpha is
 *     preserved by the original `fill` semantics; `none`/`transparent`
 *     attributes are left alone).
 *   - PNG preset with no tint: pass the data URL through unchanged.
 *   - PNG preset with tint: wrap the data URL in an SVG `<mask>` so the
 *     alpha channel becomes coverage and a coloured `<rect>` fills the
 *     silhouette — the same visual effect the plugin paints on the taskbar.
 */
function iconPreviewUrl(preset, paint) {
  if (preset.kind === "png") {
    if (!paint) return preset.png;
    const svg =
      `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 128 128">` +
      `<mask id="m">` +
      `<image href="${preset.png}" width="128" height="128" preserveAspectRatio="xMidYMid meet"/>` +
      `</mask>` +
      `<rect width="100%" height="100%" fill="${paint}" mask="url(#m)"/>` +
      `</svg>`;
    return "data:image/svg+xml;charset=utf-8," + encodeURIComponent(svg);
  }
  const body = paint
    ? preset.svg.replace(/((?:fill|stroke)=")([^"]*)(")/gi, (m, pre, value, post) =>
        /^(none|transparent)$/i.test(value.trim()) ? m : `${pre}${paint}${post}`,
      )
    : preset.svg;
  return "data:image/svg+xml;charset=utf-8," + encodeURIComponent(body);
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
const applyLineVisible = () => {
  const top = els.topShown.checked;
  const bottom = els.bottomShown.checked;
  apply("set_line_visible", { top, bottom }, { topShown: top, bottomShown: bottom });
};

/**
 * Build one line's `IconSpec` from the UI, or `null` for "no icon".
 *
 * The UI only exposes the built-in presets, so a selection always maps to a
 * preset whose content (SVG source or PNG data URL) goes out on the `data`
 * channel — no file path involved, which keeps the demo working on any
 * machine.
 */
function readIcon(line) {
  const preset = presetById.get(els[`${line}IconPreset`].value);
  if (!preset) return null;
  const tint = els[`${line}IconTint`].checked;
  // PNG presets are already a data URL; SVG presets hand the source through
  // and let the plugin pick `data:image/svg+xml;utf8,...` at apply time.
  return { data: preset.png ?? preset.svg, tint };
}

/**
 * Map a stored `IconSpec` back to a preset, or `null` when it does not match
 * one. Handles both shapes a saved spec can have: `{ data }` (equal to a
 * preset's SVG or PNG data URL) and `{ path }` (legacy — file name matching
 * the preset id).
 */
function presetForIcon(icon) {
  if (!icon) return null;
  if (icon.data) {
    const data = icon.data.trim();
    return ICON_PRESETS.find((p) => p.svg === data || p.png === data) ?? null;
  }
  if (icon.path) {
    const stem = icon.path.split(/[\\/]/).pop().replace(/\.(svg|png|ico|bmp)$/i, "").toLowerCase();
    return presetById.get(stem) ?? null;
  }
  return null;
}

/** The colour a line paints right now: solid custom hex, else taskbar ink. */
function effectiveLineColor(line) {
  if (colorMode[line] === "solid") {
    const c = resolveColor(line);
    if (/^#[0-9a-fA-F]{6}$/.test(c)) return c;
  }
  return (
    getComputedStyle(document.documentElement).getPropertyValue("--tb-ink").trim() || "#1a1a1a"
  );
}

/** Show the selected preset next to the dropdown — its raw colours, or when
 *  Tint is on the single-colour version the taskbar will actually paint. */
function updateIconPreview(line) {
  const img = els[`${line}IconPreview`];
  const preset = presetById.get(els[`${line}IconPreset`].value);
  if (preset) {
    img.src = iconPreviewUrl(preset, els[`${line}IconTint`].checked ? effectiveLineColor(line) : null);
    img.hidden = false;
  } else {
    img.removeAttribute("src");
    img.hidden = true;
  }
}

/** Same thumbnail for the leading (column) icon; tint uses the first
 *  visible line's colour, exactly like the plugin paints it. */
function updateLeadingPreview() {
  const img = els.leadingPreview;
  const preset = presetById.get(els.leadingPreset.value);
  if (preset) {
    const line = els.topShown.checked ? "top" : "bottom";
    img.src = iconDataUrl(preset.svg, els.leadingTint.checked ? effectiveLineColor(line) : null);
    img.hidden = false;
  } else {
    img.removeAttribute("src");
    img.hidden = true;
  }
}

const applyIcons = () => {
  const top = readIcon("top");
  const bottom = readIcon("bottom");
  apply("set_icon", { top, bottom }, { topIcon: top, bottomIcon: bottom });
};

/**
 * Build the leading (column) `IconSpec` from the UI, or `null` for "no icon".
 * Same preset-only rule as the per-line icons; `size` in px, `0`/blank =
 * full block height (omitted from the spec so the plugin default applies).
 */
function readLeadingIcon() {
  const preset = presetById.get(els.leadingPreset.value);
  if (!preset) return null;
  const size = parseInt(els.leadingSize.value, 10) || 0;
  return {
    data: preset.svg,
    tint: els.leadingTint.checked,
    ...(size > 0 ? { size } : {}),
  };
}

const applyLeadingIcon = () => {
  const icon = readLeadingIcon();
  apply("set_leading_icon", { icon }, { leadingIcon: icon });
};

// Dim a line section's edit controls while its "Show" switch is off.
function syncLineDim() {
  for (const [line, shown] of [["top", els.topShown], ["bottom", els.bottomShown]]) {
    shown.closest(".line-sec").classList.toggle("line-off", !shown.checked);
  }
}
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
  // Per-line visibility: absent (older plugin builds) means shown.
  els.topShown.checked = p.topVisible !== false;
  els.bottomShown.checked = p.bottomVisible !== false;
  syncLineDim();
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

  // Per-line icons: absent/null = none, otherwise the dropdown is set to the
  // preset that matches the stored spec (by SVG content, or by file name for
  // legacy `path` specs). A spec that matches nothing falls back to "No icon".
  for (const line of ["top", "bottom"]) {
    const icon = line === "top" ? p.topIcon : p.bottomIcon;
    const preset = presetForIcon(icon);
    els[`${line}IconPreset`].value = preset ? preset.id : "";
    els[`${line}IconTint`].checked = !!icon?.tint;
    updateIconPreview(line);
  }

  // Leading (column) icon: absent/null = none; size absent/0 = full block.
  {
    const preset = presetForIcon(p.leadingIcon);
    els.leadingPreset.value = preset ? preset.id : "";
    els.leadingTint.checked = !!p.leadingIcon?.tint;
    els.leadingSize.value = p.leadingIcon?.size ?? 0;
    updateLeadingPreview();
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
    topShown: els.topShown.checked,
    bottomShown: els.bottomShown.checked,
    topIcon: p.topIcon ?? null,
    bottomIcon: p.bottomIcon ?? null,
    leadingIcon: p.leadingIcon ?? null,
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

  // Per-line "Show" switches: hide that line on the taskbar (the other one
  // re-centres; both off = the whole item disappears).
  for (const shown of [els.topShown, els.bottomShown]) {
    shown.addEventListener("change", () => {
      applyLineVisible();
      syncLineDim();
      renderPreview();
    });
  }

  // Icon presets: populate the three dropdowns once at startup (static HTML
  // only carries the leading "No icon" entry).
  for (const select of [els.topIconPreset, els.bottomIconPreset, els.leadingPreset]) {
    for (const p of ICON_PRESETS) {
      const opt = document.createElement("option");
      opt.value = p.id;
      opt.textContent = p.name;
      select.appendChild(opt);
    }
  }

  // Icons: picking a preset is a commit (no typing involved), so apply right
  // away, refresh the preview thumbnail, and sync the live taskbar strip.
  // Tint changes the icon's paint in both the thumbnail and the strip.
  for (const line of lineNames) {
    const preset = els[`${line}IconPreset`];
    const tint = els[`${line}IconTint`];
    preset.addEventListener("change", () => {
      // Presets marked `tintRecommended: false` (multi-colour brand assets
      // such as a logo PNG) look wrong in tint mode — the colour information
      // is discarded, leaving a solid silhouette of the icon's outer shape.
      // Default those to tint=off whenever they're picked; users can still
      // toggle it back on manually.
      const p = presetById.get(preset.value);
      if (p && p.tintRecommended === false && tint.checked) {
        tint.checked = false;
      }
      applyIcons();
      updateIconPreview(line);
      renderPreview();
    });
    tint.addEventListener("change", () => {
      applyIcons();
      updateIconPreview(line);
      renderPreview();
    });
  }

  // Leading (column) icon: any commit (preset pick, tint toggle, size) applies
  // the whole spec at once and refreshes thumbnail + strip.
  for (const el of [els.leadingPreset, els.leadingTint, els.leadingSize]) {
    el.addEventListener("change", () => {
      applyLeadingIcon();
      updateLeadingPreview();
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
    els.topShown.checked = true;
    els.bottomShown.checked = true;
    syncLineDim();
    for (const line of ["top", "bottom"]) {
      els[`${line}IconPreset`].value = "";
      els[`${line}IconTint`].checked = false;
      updateIconPreview(line);
    }
    els.leadingPreset.value = "";
    els.leadingTint.checked = false;
    els.leadingSize.value = 0;
    updateLeadingPreview();
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
      invoke("plugin:multiline-taskband|set_line_visible", {
        payload: { id, top: true, bottom: true },
      }),
      invoke("plugin:multiline-taskband|set_icon", {
        payload: { id, top: null, bottom: null },
      }),
      invoke("plugin:multiline-taskband|set_leading_icon", {
        payload: { id, icon: null },
      }),
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
          topShown: true,
          bottomShown: true,
          topIcon: null,
          bottomIcon: null,
          leadingIcon: null,
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
