// Taskband settings popup — edits whichever taskbar instance opened it.
// The plugin emits `multiline-taskband://popup//open` (to this window only)
// with the instance's id and current state every time the popup opens, so
// every instance gets its own prefilled form.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

let currentInstanceId = null;

window.addEventListener("DOMContentLoaded", () => {
  const headerEl = document.querySelector("#popup-header");
  const topEl = document.querySelector("#popup-top");
  const bottomEl = document.querySelector("#popup-bottom");
  const layoutEl = document.querySelector("#popup-layout");
  const topSizeEl = document.querySelector("#popup-top-size");
  const bottomSizeEl = document.querySelector("#popup-bottom-size");
  const topColorTypeEl = document.querySelector("#popup-top-color-type");
  const topColorEl = document.querySelector("#popup-top-color");
  const topHexEl = document.querySelector("#popup-top-hex");
  const bottomColorTypeEl = document.querySelector("#popup-bottom-color-type");
  const bottomColorEl = document.querySelector("#popup-bottom-color");
  const bottomHexEl = document.querySelector("#popup-bottom-hex");
  const topBoldEl = document.querySelector("#popup-top-bold");
  const bottomBoldEl = document.querySelector("#popup-bottom-bold");
  const topAlignEl = document.querySelector("#popup-top-align");
  const bottomAlignEl = document.querySelector("#popup-bottom-align");

  // Color picker auto-fills the hex field; the hex field is what gets applied.
  topColorEl.addEventListener("input", () => {
    topHexEl.value = topColorEl.value;
  });
  bottomColorEl.addEventListener("input", () => {
    bottomHexEl.value = bottomColorEl.value;
  });

  // A "solid" color type enables the color/hex inputs; "default" disables them.
  const wireColorType = (typeEl, colorEl, hexEl) => {
    typeEl.addEventListener("change", () => {
      const solid = typeEl.value === "solid";
      colorEl.disabled = !solid;
      hexEl.disabled = !solid;
    });
  };
  wireColorType(topColorTypeEl, topColorEl, topHexEl);
  wireColorType(bottomColorTypeEl, bottomColorEl, bottomHexEl);

  // Prefill the whole form when an instance opens the popup.
  listen("multiline-taskband://popup//open", (event) => {
    const {
      id,
      top,
      bottom,
      topSize,
      bottomSize,
      layout,
      topColor,
      bottomColor,
      topBold,
      bottomBold,
      topAlign,
      bottomAlign,
    } = event.payload;
    currentInstanceId = id;
    if (headerEl) headerEl.textContent = `实例设置 — ${id}`;
    if (top !== undefined && top !== null) topEl.value = top;
    if (bottom !== undefined && bottom !== null) bottomEl.value = bottom;
    if (topSize !== undefined && topSize !== null) topSizeEl.value = topSize;
    if (bottomSize !== undefined && bottomSize !== null)
      bottomSizeEl.value = bottomSize;
    if (layout !== undefined && layout !== null) layoutEl.value = String(layout);
    if (topBold !== undefined && topBold !== null) topBoldEl.checked = !!topBold;
    if (bottomBold !== undefined && bottomBold !== null)
      bottomBoldEl.checked = !!bottomBold;
    if (topAlign !== undefined && topAlign !== null)
      topAlignEl.value = String(topAlign);
    if (bottomAlign !== undefined && bottomAlign !== null)
      bottomAlignEl.value = String(bottomAlign);

    const applyColor = (color, typeEl, colorEl, hexEl) => {
      if (color && color.type === "solid" && /^#[0-9a-fA-F]{6}$/.test(color.value)) {
        typeEl.value = "solid";
        colorEl.value = color.value;
        hexEl.value = color.value;
        colorEl.disabled = false;
        hexEl.disabled = false;
      } else {
        typeEl.value = "default";
        colorEl.disabled = true;
        hexEl.disabled = true;
      }
    };
    applyColor(topColor, topColorTypeEl, topColorEl, topHexEl);
    applyColor(bottomColor, bottomColorTypeEl, bottomColorEl, bottomHexEl);
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
    invoke("plugin:multiline-taskband|set_text", {
      payload: { id: currentInstanceId, top: topEl.value, bottom: bottomEl.value },
    }).catch((err) => console.error("Failed to set text:", err));
  });
  document.querySelector("#popup-text-reset").addEventListener("click", () => {
    if (!requireInstance()) return;
    topEl.value = currentInstanceId;
    bottomEl.value = currentInstanceId;
    invoke("plugin:multiline-taskband|set_text", {
      payload: { id: currentInstanceId, top: currentInstanceId, bottom: currentInstanceId },
    }).catch((err) => console.error("Failed to reset text:", err));
  });

  // --- sizes & layout ---
  document.querySelector("#popup-sizes").addEventListener("click", () => {
    if (!requireInstance()) return;
    invoke("plugin:multiline-taskband|set_font_sizes", {
      payload: {
        id: currentInstanceId,
        top: Number(topSizeEl.value) || 9,
        bottom: Number(bottomSizeEl.value) || 12,
      },
    }).catch((err) => console.error("Failed to set font sizes:", err));
  });
  layoutEl.addEventListener("change", () => {
    if (!requireInstance()) return;
    invoke("plugin:multiline-taskband|set_layout", {
      payload: { id: currentInstanceId, layout: Number(layoutEl.value) },
    }).catch((err) => console.error("Failed to set layout:", err));
  });

  // --- colors ---
  const colorStyle = (typeEl, hexEl) => {
    if (typeEl.value === "default") return { type: "default" };
    let v = hexEl.value.trim();
    if (!v.startsWith("#")) v = `#${v}`;
    return { type: "solid", value: v };
  };
  document.querySelector("#popup-colors").addEventListener("click", () => {
    if (!requireInstance()) return;
    invoke("plugin:multiline-taskband|set_colors", {
      payload: {
        id: currentInstanceId,
        top: colorStyle(topColorTypeEl, topHexEl),
        bottom: colorStyle(bottomColorTypeEl, bottomHexEl),
      },
    }).catch((err) => console.error("Failed to set colors:", err));
  });
  document.querySelector("#popup-reset-colors").addEventListener("click", () => {
    if (!requireInstance()) return;
    topColorTypeEl.value = "default";
    bottomColorTypeEl.value = "default";
    topColorEl.disabled = true;
    topHexEl.disabled = true;
    bottomColorEl.disabled = true;
    bottomHexEl.disabled = true;
    invoke("plugin:multiline-taskband|set_colors", {
      payload: {
        id: currentInstanceId,
        top: { type: "default" },
        bottom: { type: "default" },
      },
    }).catch((err) => console.error("Failed to reset colors:", err));
  });

  // --- bold ---
  document.querySelector("#popup-bold").addEventListener("click", () => {
    if (!requireInstance()) return;
    invoke("plugin:multiline-taskband|set_bold", {
      payload: {
        id: currentInstanceId,
        top: topBoldEl.checked,
        bottom: bottomBoldEl.checked,
      },
    }).catch((err) => console.error("Failed to set bold:", err));
  });

  // --- alignment ---
  document.querySelector("#popup-alignment").addEventListener("click", () => {
    if (!requireInstance()) return;
    invoke("plugin:multiline-taskband|set_alignment", {
      payload: {
        id: currentInstanceId,
        top: parseInt(topAlignEl.value, 10) || 0,
        bottom: parseInt(bottomAlignEl.value, 10) || 0,
      },
    }).catch((err) => console.error("Failed to set alignment:", err));
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
