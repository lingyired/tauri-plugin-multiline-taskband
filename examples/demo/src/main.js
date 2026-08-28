// Multiline Taskband demo frontend — drives every plugin API through
// window.__TAURI__ (withGlobalTauri: true), no bundler needed.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

// ---------------------------------------------------------------------------
// 5 preset instances: 2 on the left edge, 3 on the right edge.
// ---------------------------------------------------------------------------
const PRESETS = [
  { id: "left-1", side: "left", top: "总收益", bottom: "+5.67%" },
  { id: "left-2", side: "left", top: "成本", bottom: "¥12,340" },
  { id: "right-1", side: "right", top: "A股", bottom: "+1.23%" },
  { id: "right-2", side: "right", top: "QDII", bottom: "-0.40%" },
  { id: "right-3", side: "right", top: "黄金", bottom: "+0.81%" },
];

const created = new Set(); // ids whose overlay window exists on the taskbar
const readyIds = new Set(); // ids that have emitted the plugin `ready` event

// Frontend mirror of each instance's layout state: { side, order }. The Rust
// side owns the truth; this map only drives the demo controls (management
// list: drag re-order, side switch, visibility) so the UI can reflect and
// mutate them.
const instState = new Map(); // id -> { side, order }

// ---------------------------------------------------------------------------
// Plugin API helpers
// ---------------------------------------------------------------------------
const api = {
  create: (o) => invoke("plugin:multiline-taskband|create", { payload: o }),
  remove: (o) => invoke("plugin:multiline-taskband|remove", { payload: o }),
  setText: (o) => invoke("plugin:multiline-taskband|set_text", { payload: o }),
  setFontSizes: (o) => invoke("plugin:multiline-taskband|set_font_sizes", { payload: o }),
  setPadding: (o) => invoke("plugin:multiline-taskband|set_padding", { payload: o }),
  setSide: (o) => invoke("plugin:multiline-taskband|set_side", { payload: o }),
  setOrder: (o) => invoke("plugin:multiline-taskband|set_order", { payload: o }),
  setMargin: (o) => invoke("plugin:multiline-taskband|set_margin", { payload: o }),
  setColors: (o) => invoke("plugin:multiline-taskband|set_colors", { payload: o }),
  setBold: (o) => invoke("plugin:multiline-taskband|set_bold", { payload: o }),
  setAlignment: (o) => invoke("plugin:multiline-taskband|set_alignment", { payload: o }),
  setVisible: (o) => invoke("plugin:multiline-taskband|set_visible", { payload: o }),
  rect: (o) => invoke("plugin:multiline-taskband|rect", { payload: o }),
  isVisible: (o) => invoke("plugin:multiline-taskband|is_visible", { payload: o }),
  setPopupWindow: (o) => invoke("plugin:multiline-taskband|set_popup_window", { payload: o }),
  setAutoPopup: (o) => invoke("plugin:multiline-taskband|set_auto_popup", { payload: o }),
  openPopup: (o) => invoke("plugin:multiline-taskband|open_popup", { payload: o }),
  closePopup: (o) => invoke("plugin:multiline-taskband|close_popup", { payload: o }),
  togglePopup: (o) => invoke("plugin:multiline-taskband|toggle_popup", { payload: o }),
  setMenu: (o) => invoke("plugin:multiline-taskband|set_menu", { payload: o }),
};

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------
async function createInstance(id, side, top, bottom) {
  // Register every event listener BEFORE creating the instance. The plugin's
  // UI thread emits `ready` right after the overlay window is created, which
  // can beat this JS call if `listen` is registered after `create` — the
  // event would be dropped and the badge stuck on "pending".
  await listen(`multiline-taskband://${id}//ready`, () => {
    readyIds.add(id);
    updateBadges();
  });
  await listen(`multiline-taskband://${id}//click`, (e) => {
    const el = document.querySelector(`[data-id="${id}"] .click-out`);
    if (el) {
      const p = e.payload.position;
      el.textContent = `点击: ${e.payload.button} @ ${p.x},${p.y}`;
    }
  });
  await listen(`multiline-taskband://${id}//popup-open`, (e) => {
    const el = document.querySelector(`[data-id="${id}"] .popup-out`);
    if (el) el.textContent = "popup 打开";
  });
  await listen(`multiline-taskband://${id}//popup-close`, (e) => {
    const el = document.querySelector(`[data-id="${id}"] .popup-out`);
    if (el) el.textContent = "popup 关闭";
  });
  // Right-click context menu: "打开设置界面" re-shows the main window and
  // "退出 App" exits the process. The actions are handled on the Rust side
  // (works even while the main window is hidden); here we only echo the
  // selection on the card.
  await listen(`multiline-taskband://${id}//menu`, (e) => {
    const el = document.querySelector(`[data-id="${id}"] .popup-out`);
    if (el) el.textContent = `菜单: ${e.payload.itemId}`;
  });

  await api.create({ id, side, top, bottom }).catch((e) => console.error(`create ${id}:`, e));
  created.add(id);
  if (!instState.has(id)) {
    // Creation order is the initial sort key; the management list drag
    // re-assigns 0..n-1 afterwards.
    instState.set(id, { side, order: instState.size });
  }
  await api.setMenu({
    id,
    items: [
      { type: "item", id: "open-settings", text: "打开设置界面" },
      { type: "separator" },
      { type: "item", id: "quit", text: "退出 App" },
    ],
  }).catch((e) => console.error(`setMenu ${id}:`, e));
  renderList();
  renderManageList();
  updateStatus();
}

async function removeInstance(id) {
  await api.remove({ id }).catch((e) => console.error(`remove ${id}:`, e));
  created.delete(id);
  readyIds.delete(id);
  instState.delete(id);
  renderList();
  renderManageList();
  updateStatus();
}

async function setAllVisible(visible) {
  for (const id of created) {
    await api.setVisible({ id, visible }).catch(() => {});
  }
}

// ---------------------------------------------------------------------------
// UI
// ---------------------------------------------------------------------------
function renderList() {
  const ul = document.querySelector("#instance-list");
  ul.innerHTML = "";
  // Cards follow the same order as the management list (ascending `order`).
  const ids = [...created].sort(
    (a, b) => (instState.get(a)?.order ?? 0) - (instState.get(b)?.order ?? 0)
  );
  for (const id of ids) {
    ul.appendChild(instanceCard(id));
  }
}

/** Reflect `readyIds` on every rendered card's badge (idempotent). */
function updateBadges() {
  for (const el of document.querySelectorAll(".ready-badge")) {
    const id = el.closest(".instance-card")?.dataset.id;
    const ready = readyIds.has(id);
    el.textContent = ready ? "ready" : "pending";
    el.classList.toggle("ready", ready);
  }
}

function instanceCard(id) {
  const preset = PRESETS.find((p) => p.id === id);
  const state = instState.get(id) || {
    side: preset ? preset.side : "right",
    order: 0,
  };
  const ready = readyIds.has(id);
  const li = document.createElement("li");
  li.className = "instance-card";
  li.dataset.id = id;

  const head = document.createElement("div");
  head.className = "instance-head";
  head.innerHTML = `
    <span class="instance-name">${id}</span>
    <span class="badge ready-badge ${ready ? "ready" : ""}">${ready ? "ready" : "pending"}</span>
    <label class="switch" title="显示/隐藏">
      <input type="checkbox" class="vis-toggle" checked />
      <span class="slider"></span>
    </label>
    <button type="button" class="remove-btn">删除</button>
  `;
  li.appendChild(head);

  // --- top/bottom text ---
  li.appendChild(textRow("文本", preset ? preset.top : "", preset ? preset.bottom : ""));

  // --- font sizes (both lines default to the same size) ---
  li.appendChild(fontRow());

  // --- per-instance horizontal padding ---
  li.appendChild(paddingRow());

  // --- colors (default / #rrggbb, per line) ---
  li.appendChild(colorRow("上行颜色", "top"));
  li.appendChild(colorRow("下行颜色", "bottom"));

  // --- bold & alignment ---
  li.appendChild(boldAlignRow());

  // --- rect / isVisible ---
  li.appendChild(rectRow());

  // --- click / popup feedback ---
  li.appendChild(feedbackRow());

  bindEvents(li, id);
  return li;
}

function textRow(label, topVal, bottomVal) {
  const row = document.createElement("div");
  row.className = "field-row";
  row.innerHTML = `
    <label>${label}</label>
    <input class="top-text" value="${topVal}" placeholder="上行" />
    <input class="bottom-text" value="${bottomVal}" placeholder="下行" />
  `;
  return row;
}

function fontRow() {
  const row = document.createElement("div");
  row.className = "field-row";
  row.innerHTML = `
    <label>字号(pt)</label>
    <input type="number" class="top-size" value="11" min="6" max="24" step="0.5" title="上行字号" />
    <input type="number" class="bottom-size" value="11" min="6" max="24" step="0.5" title="下行字号" />
  `;
  return row;
}

function paddingRow() {
  const row = document.createElement("div");
  row.className = "field-row";
  row.innerHTML = `
    <label>左右边距(px)</label>
    <input type="number" class="pad-left" value="4" min="0" max="24" step="1" title="左边距（物理像素）" />
    <input type="number" class="pad-right" value="4" min="0" max="24" step="1" title="右边距（物理像素）" />
  `;
  return row;
}

function colorRow(label, line) {
  const row = document.createElement("div");
  row.className = "field-row";
  row.dataset.line = line;
  row.innerHTML = `
    <label>${label}</label>
    <select class="color-type">
      <option value="default">default（跟随系统）</option>
      <option value="solid">solid（固定色）</option>
    </select>
    <input type="color" class="color-value" value="#FF4F44" disabled />
    <input class="color-hex" value="#FF4F44" placeholder="#rrggbb" disabled />
  `;
  return row;
}

function boldAlignRow() {
  const row = document.createElement("div");
  row.className = "field-row";
  row.innerHTML = `
    <label>加粗</label>
    <label class="mini">上<input type="checkbox" class="top-bold" /></label>
    <label class="mini">下<input type="checkbox" class="bottom-bold" /></label>
    <label class="mini align-lbl">对齐</label>
    <select class="top-align">
      <option value="0">上·左</option><option value="1">上·中</option><option value="2">上·右</option>
    </select>
    <select class="bottom-align">
      <option value="0">下·左</option><option value="1">下·中</option><option value="2">下·右</option>
    </select>
  `;
  return row;
}

function rectRow() {
  const row = document.createElement("div");
  row.className = "field-row";
  row.innerHTML = `
    <button type="button" class="rect-btn">rect()</button>
    <span class="rect-out muted"></span>
  `;
  return row;
}

function feedbackRow() {
  const row = document.createElement("div");
  row.className = "field-row";
  row.innerHTML = `
    <span class="click-out muted"></span>
    <span class="popup-out muted"></span>
  `;
  return row;
}

// ---------------------------------------------------------------------------
// Event wiring (fire-and-forget; the plugin marshals to its UI thread)
// ---------------------------------------------------------------------------
function bindEvents(li, id) {
  const $ = (sel) => li.querySelector(sel);

  $(".vis-toggle").addEventListener("change", (e) =>
    api.setVisible({ id, visible: e.target.checked }).catch(() => {})
  );
  $(".remove-btn").addEventListener("click", () => removeInstance(id));

  const onInput = (sel, fn) => {
    const el = $(sel);
    el.addEventListener(el.type === "checkbox" ? "change" : "input", () => fn(el));
  };

  const pushText = () =>
    api.setText({ id, top: $(".top-text").value, bottom: $(".bottom-text").value });
  onInput(".top-text", () => pushText());
  onInput(".bottom-text", () => pushText());

  onInput(".top-size", (el) =>
    api.setFontSizes({ id, top: Number(el.value), bottom: Number($(".bottom-size").value) })
  );
  onInput(".bottom-size", (el) =>
    api.setFontSizes({ id, top: Number($(".top-size").value), bottom: Number(el.value) })
  );

  onInput(".pad-left", (el) =>
    api.setPadding({ id, left: Number(el.value), right: Number($(".pad-right").value) })
  );
  onInput(".pad-right", (el) =>
    api.setPadding({ id, left: Number($(".pad-left").value), right: Number(el.value) })
  );

  // Wire both color rows: read the current value of the other row so
  // setColors always receives a complete { top, bottom } pair.
  const colorState = () => {
    const read = (line) => {
      const row = li.querySelector(`.field-row[data-line="${line}"]`);
      if (row.querySelector(".color-type").value === "default") {
        return { type: "default" };
      }
      let v = row.querySelector(".color-hex").value.trim();
      if (!v.startsWith("#")) v = `#${v}`;
      return { type: "solid", value: v };
    };
    return { top: read("top"), bottom: read("bottom") };
  };
  for (const line of ["top", "bottom"]) {
    const row = li.querySelector(`.field-row[data-line="${line}"]`);
    const typeSel = row.querySelector(".color-type");
    const colorVal = row.querySelector(".color-value");
    const hexIn = row.querySelector(".color-hex");
    const push = () => api.setColors({ id, ...colorState() }).catch(() => {});
    typeSel.addEventListener("change", () => {
      const solid = typeSel.value === "solid";
      colorVal.disabled = !solid;
      hexIn.disabled = !solid;
      push();
    });
    colorVal.addEventListener("input", () => {
      hexIn.value = colorVal.value;
      push();
    });
    hexIn.addEventListener("input", () => push());
  }

  const boldPush = () =>
    api.setBold({ id, top: $(".top-bold").checked, bottom: $(".bottom-bold").checked });
  $(".top-bold").addEventListener("change", boldPush);
  $(".bottom-bold").addEventListener("change", boldPush);

  $(".top-align").addEventListener("change", (e) => pushAlign());
  $(".bottom-align").addEventListener("change", (e) => pushAlign());
  const pushAlign = () =>
    api.setAlignment({ id, top: Number($(".top-align").value), bottom: Number($(".bottom-align").value) });

  $(".rect-btn").addEventListener("click", async () => {
    try {
      const r = await api.rect({ id });
      $(".rect-out").textContent = `x=${r.x} y=${r.y} ${r.width}×${r.height}`;
    } catch (e) {
      $(".rect-out").textContent = String(e);
    }
  });
}

// ---------------------------------------------------------------------------
// Instance management list (drag to re-order, switch side, toggle visibility)
// ---------------------------------------------------------------------------
let dragId = null;

function renderManageList() {
  const ul = document.querySelector("#instance-manage-list");
  ul.innerHTML = "";
  const ids = [...created].sort(
    (a, b) => (instState.get(a)?.order ?? 0) - (instState.get(b)?.order ?? 0)
  );
  for (const id of ids) {
    ul.appendChild(manageRow(id));
  }
}

function manageRow(id) {
  const state = instState.get(id) || { side: "right", order: 0 };
  const li = document.createElement("li");
  li.className = "manage-row";
  li.dataset.id = id;
  li.draggable = true;
  li.innerHTML = `
    <span class="drag-handle" title="拖拽调整顺序">⠿</span>
    <span class="manage-name">${id}</span>
    <select class="manage-side" title="靠任务栏左/右侧">
      <option value="left" ${state.side === "left" ? "selected" : ""}>left</option>
      <option value="right" ${state.side === "right" ? "selected" : ""}>right</option>
    </select>
    <label class="switch" title="显示/隐藏">
      <input type="checkbox" class="manage-vis" checked />
      <span class="slider"></span>
    </label>
  `;

  li.addEventListener("dragstart", (e) => {
    dragId = id;
    li.classList.add("dragging");
    e.dataTransfer.effectAllowed = "move";
    // Chromium/WebView2 refuse to fire dragover/drop on the target unless the
    // source registers at least one data item during dragstart.
    e.dataTransfer.setData("text/plain", id);
  });
  li.addEventListener("dragend", () => {
    dragId = null;
    for (const el of document.querySelectorAll("#instance-manage-list li")) {
      el.classList.remove("dragging", "drag-over");
    }
  });
  li.addEventListener("dragover", (e) => {
    e.preventDefault(); // allow drop
    e.dataTransfer.dropEffect = "move";
    li.classList.add("drag-over");
  });
  li.addEventListener("dragleave", () => {
    li.classList.remove("drag-over");
  });
  li.addEventListener("drop", (e) => {
    e.preventDefault();
    li.classList.remove("drag-over");
    if (!dragId || dragId === id) return;
    const ulEl = document.querySelector("#instance-manage-list");
    const rows = [...ulEl.children];
    const fromEl = rows.find((r) => r.dataset.id === dragId);
    const toEl = rows.find((r) => r.dataset.id === id);
    if (!fromEl || !toEl) return;
    // Move the dragged row before/after the target, matching mouse intent.
    const fromIdx = rows.indexOf(fromEl);
    const toIdx = rows.indexOf(toEl);
    if (fromIdx < toIdx) toEl.after(fromEl);
    else toEl.before(fromEl);
    // Re-assign order by the new list position: list order == global order.
    [...ulEl.children].forEach((row, i) => {
      const rid = row.dataset.id;
      const st = instState.get(rid);
      if (st) st.order = i;
      api.setOrder({ id: rid, order: i }).catch(() => {});
    });
    renderList(); // keep the cards in the same order as the management list
  });

  li.querySelector(".manage-side").addEventListener("change", (e) => {
    const st = instState.get(id);
    if (st) st.side = e.target.value;
    api.setSide({ id, side: e.target.value }).catch(() => {});
  });
  li.querySelector(".manage-vis").addEventListener("change", (e) => {
    li.classList.toggle("row-hidden", !e.target.checked);
    api.setVisible({ id, visible: e.target.checked }).catch(() => {});
  });
  return li;
}

// ---------------------------------------------------------------------------
// Boot
// ---------------------------------------------------------------------------
function updateStatus() {
  document.querySelector("#instance-status").textContent = `${created.size} 个实例`;
}

document.querySelector("#create-btn").addEventListener("click", async () => {
  const id = document.querySelector("#new-id").value.trim();
  if (!id) return;
  const side = document.querySelector("#new-side").value;
  const top = document.querySelector("#new-top").value.trim();
  const bottom = document.querySelector("#new-bottom").value.trim();
  await createInstance(id, side, top, bottom);
  document.querySelector("#new-id").value = "";
});

document.querySelector("#show-all-btn").addEventListener("click", () => setAllVisible(true));
document.querySelector("#hide-all-btn").addEventListener("click", () => setAllVisible(false));

document.querySelector("#margin-btn").addEventListener("click", () => {
  const v = parseInt(document.querySelector("#global-margin").value, 10);
  if (Number.isFinite(v)) {
    api.setMargin({ margin: v }).catch((e) => console.error("setMargin:", e));
  }
});

// Create the 5 presets on boot.
(async () => {
  // The settings popup window is declared in tauri.conf.json (label "popup").
  // Register it with the plugin and keep auto-popup on left click enabled —
  // hosts must call setPopupWindow before the first click.
  await api.setPopupWindow({ label: "popup" }).catch((e) => console.error("setPopupWindow:", e));
  await api.setAutoPopup({ enabled: true }).catch(() => {});
  for (const p of PRESETS) {
    await createInstance(p.id, p.side, p.top, p.bottom);
  }
})();
