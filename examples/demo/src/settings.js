// Shared settings persistence for the demo.
//
// The host (this demo frontend) owns persistence — the plugin itself is
// stateless. The main window and the settings popup share the same origin,
// so both read/write this one localStorage key. Whenever a setting is
// applied it is saved immediately, and on the next launch the main window
// re-applies everything to the plugin.
//
// Storage shape (`multiline-taskband-demo:settings-v1`):
//   {
//     margin: 4,                       // global instance gap (physical px)
//     leftEdgeMargin: 0,               // extra gap after the taskbar's left
//                                     // edge for the left group (physical px)
//     rightEdgeMargin: 0,              // extra gap before the notification
//                                     // area for the right group (physical px)
//     instances: { [id]: Instance },   // per-instance appearance + shown
//     order: [id, ...]                 // unified list order — also the layout
//                                      // order per side on the taskbar (the
//                                      // demo derives each side's set_order
//                                      // keys from this sequence)
//   }
// Instance: { shown, topShown, bottomShown, side, top, bottom, topSize,
//             bottomSize, topFontFamily, bottomFontFamily, leftPadding,
//             rightPadding, topColor, bottomColor, topBold, bottomBold,
//             topAlign, bottomAlign }
// `shown` hides the whole instance; `topShown`/`bottomShown` hide individual
// lines (both false = the instance renders nothing, like shown === false).
//
// The pre-settings-v1 key (which only stored shown/hidden) is read as a
// migration fallback when the new key does not exist yet. An older shape that
// kept customs in a separate `customOrder` array is migrated too: order then
// becomes presets in canonical order followed by the customs.

const STORAGE_KEY = "multiline-taskband-demo:settings-v1";
const LEGACY_SHOWN_KEY = "multiline-taskband-demo:shown-v2";

export const DEFAULT_MARGIN = 4;
export const DEFAULT_EDGE_MARGIN = 0;
export const DEFAULT_FONT_SIZE = 11;
export const DEFAULT_PADDING = 4;

/**
 * The demo's 5 canonical instances (2 left / 3 right). Defined here so both
 * the main window and the popup share the same canonical list — the popup's
 * persistence path must know them too, otherwise loadSettings() would treat
 * them as orphaned records and drop them.
 */
export const PRESETS = [
  { id: "mb-1", side: "left", top: "mb-1", bottom: "mb-1" },
  { id: "mb-2", side: "left", top: "mb-2", bottom: "mb-2" },
  { id: "mb-3", side: "right", top: "mb-3", bottom: "mb-3" },
  { id: "mb-4", side: "right", top: "mb-4", bottom: "mb-4" },
  { id: "mb-5", side: "right", top: "mb-5", bottom: "mb-5" },
];

/** Fresh per-instance defaults. `side` defaults to `right`. */
export function instanceDefaults(id, side = "right") {
  return {
    shown: true,
    topShown: true,
    bottomShown: true,
    side,
    top: id,
    bottom: id,
    topSize: DEFAULT_FONT_SIZE,
    bottomSize: DEFAULT_FONT_SIZE,
    topFontFamily: null,
    bottomFontFamily: null,
    leftPadding: DEFAULT_PADDING,
    rightPadding: DEFAULT_PADDING,
    topColor: { type: "default" },
    bottomColor: { type: "default" },
    topBold: false,
    bottomBold: false,
    topAlign: 0,
    bottomAlign: 0,
  };
}

function normalizeInstance(id, raw) {
  return { ...instanceDefaults(id), ...(raw || {}) };
}

/**
 * Load the persisted settings, seeding defaults for the preset list and
 * migrating the legacy shown-only key. Tolerant of missing fields / older
 * shapes: stored values win over defaults. Instance records that are neither
 * a preset nor referenced by `order` are dropped.
 */
export function loadSettings(presets = PRESETS) {
  const settings = {
    margin: DEFAULT_MARGIN,
    leftEdgeMargin: DEFAULT_EDGE_MARGIN,
    rightEdgeMargin: DEFAULT_EDGE_MARGIN,
    instances: {},
    order: presets.map((p) => p.id),
  };
  const presetIds = new Set(presets.map((p) => p.id));
  for (const p of presets) {
    settings.instances[p.id] = instanceDefaults(p.id, p.side);
  }

  let raw = null;
  try {
    raw = JSON.parse(window.localStorage.getItem(STORAGE_KEY));
  } catch (_) {}
  if (raw && typeof raw === "object") {
    if (typeof raw.margin === "number" && Number.isFinite(raw.margin)) {
      settings.margin = raw.margin;
    }
    if (typeof raw.leftEdgeMargin === "number" && Number.isFinite(raw.leftEdgeMargin)) {
      settings.leftEdgeMargin = raw.leftEdgeMargin;
    }
    if (typeof raw.rightEdgeMargin === "number" && Number.isFinite(raw.rightEdgeMargin)) {
      settings.rightEdgeMargin = raw.rightEdgeMargin;
    }
    if (raw.instances && typeof raw.instances === "object") {
      for (const [id, inst] of Object.entries(raw.instances)) {
        settings.instances[id] = normalizeInstance(id, inst);
      }
    }
    // Unified list order (also the per-side taskbar layout order). Falls back
    // to the older presets-then-customOrder shape when `order` is absent.
    if (Array.isArray(raw.order)) {
      settings.order = raw.order.filter((id) => settings.instances[id]);
    } else if (Array.isArray(raw.customOrder)) {
      settings.order = [
        ...presets.map((p) => p.id),
        ...raw.customOrder.filter((id) => id && !presetIds.has(id) && settings.instances[id]),
      ];
    }
  } else {
    // Migration: the old key only recorded which presets were shown/hidden.
    try {
      const legacy = JSON.parse(window.localStorage.getItem(LEGACY_SHOWN_KEY));
      if (legacy && typeof legacy === "object") {
        for (const [id, shown] of Object.entries(legacy)) {
          if (typeof shown === "boolean" && settings.instances[id]) {
            settings.instances[id].shown = shown;
          }
        }
      }
    } catch (_) {}
  }

  // Drop orphaned instance records (order is authoritative for customs) so
  // the store can't accumulate stale entries.
  for (const id of Object.keys(settings.instances)) {
    if (!presetIds.has(id) && !settings.order.includes(id)) {
      delete settings.instances[id];
    }
  }
  // Keep the order complete and duplicate-free (e.g. presets a stored order
  // predates), then prune ids whose record is gone.
  settings.order = [...new Set(settings.order)].filter((id) => settings.instances[id]);
  for (const id of Object.keys(settings.instances)) {
    if (!settings.order.includes(id)) settings.order.push(id);
  }
  return settings;
}

export function saveSettings(settings) {
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
  } catch (_) {}
}

/**
 * Merge field updates into one instance's stored record and save. Used by
 * the popup window, which knows a single instance id but not the presets:
 * it patches that instance's own entry and preserves host-owned fields
 * (shown, order, margin, edge margins). Returns the merged instance.
 */
export function saveInstanceState(id, updates) {
  const settings = loadSettings();
  settings.instances[id] = normalizeInstance(id, {
    ...settings.instances[id],
    ...updates,
  });
  saveSettings(settings);
  return settings.instances[id];
}
