// Taskband settings popup — edits whichever taskbar instance opened it.
// The plugin emits `multiline-taskband://popup//open` (to this window only)
// with the instance's id and current state every time the popup opens, so
// every instance gets its own prefilled form.
//
// Persistence: every applied change is written straight to the shared
// localStorage store (see settings.js), so the main window — and the next
// launch — see it. Opening the popup also re-syncs the store with the
// plugin's actual state, in case a previous apply failed.
import { saveInstanceState } from "./settings.js";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

// Which taskbar instance opened this popup. Set when the plugin emits the
// "open" event; used so Apply/Close target the right instance.
let currentInstanceId = null;
// Mirror of the instance's persisted state; every apply patches this and
// saves it back to the store.
let currentState = null;

function persistState() {
  if (!currentInstanceId || !currentState) return;
  saveInstanceState(currentInstanceId, currentState);
}

window.addEventListener("DOMContentLoaded", () => {
  const headerEl = document.querySelector("#popup-header");
  const topEl = document.querySelector("#popup-top");
  const bottomEl = document.querySelector("#popup-bottom");
  const topSizeEl = document.querySelector("#popup-top-size");
  const topSizeValueEl = document.querySelector("#popup-top-size-value");
  const bottomSizeEl = document.querySelector("#popup-bottom-size");
  const bottomSizeValueEl = document.querySelector("#popup-bottom-size-value");
  const topFamilyEl = document.querySelector("#popup-top-family");
  const bottomFamilyEl = document.querySelector("#popup-bottom-family");
  const padLeftEl = document.querySelector("#popup-pad-left");
  const padRightEl = document.querySelector("#popup-pad-right");
  const sideEl = document.querySelector("#popup-side");
  const topColorEl = document.querySelector("#popup-top-color");
  const topHexEl = document.querySelector("#popup-top-hex");
  const bottomColorEl = document.querySelector("#popup-bottom-color");
  const bottomHexEl = document.querySelector("#popup-bottom-hex");
  const topColorModeEl = document.querySelector("#popup-top-color-mode");
  const bottomColorModeEl = document.querySelector("#popup-bottom-color-mode");
  const topSolidRowEl = document.querySelector("#popup-top-solid-row");
  const bottomSolidRowEl = document.querySelector("#popup-bottom-solid-row");
  const topBoldEl = document.querySelector("#popup-top-bold");
  const bottomBoldEl = document.querySelector("#popup-bottom-bold");
  const topAlignEl = document.querySelector("#popup-top-align");
  const bottomAlignEl = document.querySelector("#popup-bottom-align");

  // Picking a color in the native picker auto-fills the hex field (which the
  // user may still edit by hand); the hex field is what gets applied.
  topColorEl.addEventListener("input", () => {
    topHexEl.value = topColorEl.value;
  });
  bottomColorEl.addEventListener("input", () => {
    bottomHexEl.value = bottomColorEl.value;
  });

  // "System default" hides the picker/hex row — applying then sends
  // `{ type: "default" }` and the plugin paints the line black on a light
  // taskbar / white on a dark one, following theme switches automatically.
  // Switching back to "Custom color" reveals the previously picked color.
  const syncSolidRow = (modeEl, rowEl) => {
    rowEl.hidden = modeEl.value !== "solid";
  };
  topColorModeEl.addEventListener("change", () => syncSolidRow(topColorModeEl, topSolidRowEl));
  bottomColorModeEl.addEventListener("change", () =>
    syncSolidRow(bottomColorModeEl, bottomSolidRowEl),
  );

  // Live-update the font size readouts as the sliders move.
  topSizeEl.addEventListener("input", () => {
    topSizeValueEl.textContent = topSizeEl.value;
  });
  bottomSizeEl.addEventListener("input", () => {
    bottomSizeValueEl.textContent = bottomSizeEl.value;
  });

  // The plugin sends the instance id and its current state whenever the popup
  // opens. Re-render so each instance shows its own content.
  listen("multiline-taskband://popup//open", (event) => {
    const {
      id,
      top,
      bottom,
      topSize,
      bottomSize,
      topFontFamily,
      bottomFontFamily,
      leftPadding,
      rightPadding,
      topColor,
      bottomColor,
      topBold,
      bottomBold,
      topAlign,
      bottomAlign,
      side,
    } = event.payload;
    currentInstanceId = id;
    if (headerEl) headerEl.textContent = `Instance settings — ${id}`;
    if (top !== undefined && top !== null) topEl.value = top;
    if (bottom !== undefined && bottom !== null) bottomEl.value = bottom;
    if (topSize !== undefined && topSize !== null) {
      topSizeEl.value = topSize;
      topSizeValueEl.textContent = topSize;
    }
    if (bottomSize !== undefined && bottomSize !== null) {
      bottomSizeEl.value = bottomSize;
      bottomSizeValueEl.textContent = bottomSize;
    }
    // Font family: null/absent = system font, shown as an empty input.
    topFamilyEl.value = topFontFamily || "";
    bottomFamilyEl.value = bottomFontFamily || "";
    if (leftPadding !== undefined && leftPadding !== null) padLeftEl.value = leftPadding;
    if (rightPadding !== undefined && rightPadding !== null) padRightEl.value = rightPadding;
    if (side === "left" || side === "right") sideEl.value = side;
    if (topBold !== undefined && topBold !== null) topBoldEl.checked = !!topBold;
    if (bottomBold !== undefined && bottomBold !== null) bottomBoldEl.checked = !!bottomBold;
    if (topAlign !== undefined && topAlign !== null) topAlignEl.value = String(topAlign);
    if (bottomAlign !== undefined && bottomAlign !== null) bottomAlignEl.value = String(bottomAlign);
    // Per-line color mode: a `solid` color selects "Custom color" and
    // pre-fills the picker/hex; anything else (`default`) selects "System
    // default" and hides the picker row.
    const syncColorMode = (color, modeEl, rowEl, colorEl, hexEl) => {
      const solid = !!color && color.type === "solid";
      modeEl.value = solid ? "solid" : "default";
      rowEl.hidden = !solid;
      if (solid) {
        if (/^#[0-9a-fA-F]{6}$/.test(color.value)) colorEl.value = color.value;
        hexEl.value = color.value || "";
      }
    };
    syncColorMode(topColor, topColorModeEl, topSolidRowEl, topColorEl, topHexEl);
    syncColorMode(bottomColor, bottomColorModeEl, bottomSolidRowEl, bottomColorEl, bottomHexEl);

    // Re-sync the store with the plugin's authoritative state so a failed
    // apply earlier can't leave stale settings behind. Only when the payload
    // carries the full snapshot (it always does with the current plugin).
    if (side === "left" || side === "right") {
      currentState = {
        side,
        top: top ?? null,
        bottom: bottom ?? null,
        topSize: topSize ?? 11,
        bottomSize: bottomSize ?? 11,
        topFontFamily: topFontFamily ?? null,
        bottomFontFamily: bottomFontFamily ?? null,
        leftPadding: leftPadding ?? 4,
        rightPadding: rightPadding ?? 4,
        topColor: topColor ?? { type: "default" },
        bottomColor: bottomColor ?? { type: "default" },
        topBold: !!topBold,
        bottomBold: !!bottomBold,
        topAlign: topAlign ?? 0,
        bottomAlign: bottomAlign ?? 0,
      };
      persistState();
    } else {
      currentState = null;
    }
  }).catch((err) => console.error("Failed to listen for popup open:", err));

  const requireInstance = () => {
    if (!currentInstanceId) {
      console.warn("Popup action ignored: no instance is targeted.");
      return false;
    }
    return true;
  };

  // --- text ---
  document.querySelector("#popup-text-update").addEventListener("click", () => {
    if (!requireInstance()) return;
    const top = topEl.value;
    const bottom = bottomEl.value;
    invoke("plugin:multiline-taskband|set_text", {
      payload: { id: currentInstanceId, top, bottom },
    })
      .then(() => {
        currentState.top = top;
        currentState.bottom = bottom;
        persistState();
      })
      .catch((err) => console.error("Failed to set text:", err));
  });

  // Reset the text to the instance's own id on both lines — the demo default
  // that makes instances easy to identify.
  document.querySelector("#popup-text-reset").addEventListener("click", () => {
    if (!requireInstance()) return;
    const id = currentInstanceId;
    topEl.value = id;
    bottomEl.value = id;
    invoke("plugin:multiline-taskband|set_text", {
      payload: { id, top: id, bottom: id },
    })
      .then(() => {
        currentState.top = id;
        currentState.bottom = id;
        persistState();
      })
      .catch((err) => console.error("Failed to reset text:", err));
  });

  // --- font sizes (per line, in pt) ---
  document.querySelector("#popup-sizes").addEventListener("click", () => {
    if (!requireInstance()) return;
    const top = Number(topSizeEl.value) || 11;
    const bottom = Number(bottomSizeEl.value) || 11;
    invoke("plugin:multiline-taskband|set_font_sizes", {
      payload: { id: currentInstanceId, top, bottom },
    })
      .then(() => {
        currentState.topSize = top;
        currentState.bottomSize = bottom;
        persistState();
      })
      .catch((err) => console.error("Failed to set font sizes:", err));
  });

  // --- font family (per line; blank = system font) ---
  // `set_font_family` treats `null` as "system font", so an empty input is
  // normalised to `null` before invoking.
  const applyFontFamilies = (top, bottom) => {
    invoke("plugin:multiline-taskband|set_font_family", {
      payload: { id: currentInstanceId, top, bottom },
    })
      .then(() => {
        currentState.topFontFamily = top;
        currentState.bottomFontFamily = bottom;
        persistState();
      })
      .catch((err) => console.error("Failed to set font family:", err));
  };
  document.querySelector("#popup-families").addEventListener("click", () => {
    if (!requireInstance()) return;
    const top = topFamilyEl.value.trim() || null;
    const bottom = bottomFamilyEl.value.trim() || null;
    applyFontFamilies(top, bottom);
  });
  document.querySelector("#popup-families-reset").addEventListener("click", () => {
    if (!requireInstance()) return;
    topFamilyEl.value = "";
    bottomFamilyEl.value = "";
    applyFontFamilies(null, null);
  });

  // --- per-instance horizontal padding (physical px) ---
  document.querySelector("#popup-padding").addEventListener("click", () => {
    if (!requireInstance()) return;
    const left = parseInt(padLeftEl.value, 10) || 4;
    const right = parseInt(padRightEl.value, 10) || 4;
    invoke("plugin:multiline-taskband|set_padding", {
      payload: { id: currentInstanceId, left, right },
    })
      .then(() => {
        currentState.leftPadding = left;
        currentState.rightPadding = right;
        persistState();
      })
      .catch((err) => console.error("Failed to set padding:", err));
  });

  // --- side (left / right) — taskband-specific ---
  document.querySelector("#popup-side-apply").addEventListener("click", () => {
    if (!requireInstance()) return;
    const side = sideEl.value;
    invoke("plugin:multiline-taskband|set_side", {
      payload: { id: currentInstanceId, side },
    })
      .then(() => {
        currentState.side = side;
        persistState();
      })
      .catch((err) => console.error("Failed to set side:", err));
  });
  document.querySelector("#popup-side-reset").addEventListener("click", () => {
    if (!requireInstance()) return;
    sideEl.value = "right";
    invoke("plugin:multiline-taskband|set_side", {
      payload: { id: currentInstanceId, side: "right" },
    })
      .then(() => {
        currentState.side = "right";
        persistState();
      })
      .catch((err) => console.error("Failed to reset side:", err));
  });

  // --- colors ---
  // Resolve the effective color for a line: prefer the hex text field (works
  // even when the native <input type=color> picker can't open in this window),
  // fall back to the color input value.
  const resolveColor = (line) => {
    const hex = document.querySelector(`#popup-${line}-hex`).value.trim();
    if (hex) return hex;
    return document.querySelector(`#popup-${line}-color`).value;
  };

  // Apply the chosen colors to whichever instance opened this popup. Each
  // line is sent according to its own mode: "System default" sends
  // `{ type: "default" }`, "Custom color" sends the picked hex.
  document.querySelector("#popup-colors").addEventListener("click", () => {
    if (!requireInstance()) return;
    const top =
      topColorModeEl.value === "solid"
        ? { type: "solid", value: resolveColor("top") }
        : { type: "default" };
    const bottom =
      bottomColorModeEl.value === "solid"
        ? { type: "solid", value: resolveColor("bottom") }
        : { type: "default" };
    invoke("plugin:multiline-taskband|set_colors", {
      payload: { id: currentInstanceId, top, bottom },
    })
      .then(() => {
        currentState.topColor = top;
        currentState.bottomColor = bottom;
        persistState();
      })
      .catch((err) => console.error("Failed to set colors:", err));
  });

  // Revert both lines to the system default text color and reflect that in
  // the mode selects (the custom pickers keep their last value so switching
  // back to "Custom color" restores the previous choice).
  document.querySelector("#popup-reset-colors").addEventListener("click", () => {
    if (!requireInstance()) return;
    topColorModeEl.value = "default";
    bottomColorModeEl.value = "default";
    topSolidRowEl.hidden = true;
    bottomSolidRowEl.hidden = true;
    const top = { type: "default" };
    const bottom = { type: "default" };
    invoke("plugin:multiline-taskband|set_colors", {
      payload: { id: currentInstanceId, top, bottom },
    })
      .then(() => {
        currentState.topColor = top;
        currentState.bottomColor = bottom;
        persistState();
      })
      .catch((err) => console.error("Failed to reset colors:", err));
  });

  // --- bold ---
  document.querySelector("#popup-bold").addEventListener("click", () => {
    if (!requireInstance()) return;
    const top = topBoldEl.checked;
    const bottom = bottomBoldEl.checked;
    invoke("plugin:multiline-taskband|set_bold", {
      payload: { id: currentInstanceId, top, bottom },
    })
      .then(() => {
        currentState.topBold = top;
        currentState.bottomBold = bottom;
        persistState();
      })
      .catch((err) => console.error("Failed to set bold:", err));
  });

  // --- alignment (per line: 0 = left, 1 = center, 2 = right) ---
  document.querySelector("#popup-alignment").addEventListener("click", () => {
    if (!requireInstance()) return;
    const top = parseInt(topAlignEl.value, 10) || 0;
    const bottom = parseInt(bottomAlignEl.value, 10) || 0;
    invoke("plugin:multiline-taskband|set_alignment", {
      payload: { id: currentInstanceId, top, bottom },
    })
      .then(() => {
        currentState.topAlign = top;
        currentState.bottomAlign = bottom;
        persistState();
      })
      .catch((err) => console.error("Failed to set alignment:", err));
  });

  // --- close ---
  document.querySelector("#popup-close").addEventListener("click", () => {
    if (!currentInstanceId) {
      console.warn("Popup close ignored: no instance is targeted.");
      return;
    }
    invoke("plugin:multiline-taskband|close_popup", {
      payload: { id: currentInstanceId },
    }).catch((err) => console.error("Failed to close popup:", err));
  });
});
