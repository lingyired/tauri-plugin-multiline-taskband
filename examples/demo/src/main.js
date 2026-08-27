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

// ---------------------------------------------------------------------------
// Plugin API helpers
// ---------------------------------------------------------------------------
const api = {
  create: (o) => invoke("plugin:multiline-taskband|create", { payload: o }),
  remove: (o) => invoke("plugin:multiline-taskband|remove", { payload: o }),
  setText: (o) => invoke("plugin:multiline-taskband|set_text", { payload: o }),
  setFontSizes: (o) => invoke("plugin:multiline-taskband|set_font_sizes", { payload: o }),
  setLayout: (o) => invoke("plugin:multiline-taskband|set_layout", { payload: o }),
  setColors: (o) => invoke("plugin:multiline-taskband|set_colors", { payload: o }),
  setBold: (o) => invoke("plugin:multiline-taskband|set_bold", { payload: o }),
  setAlignment: (o) => invoke("plugin:multiline-taskband|set_alignment", { payload: o }),
  setVisible: (o) => invoke("plugin:multiline-taskband|set_visible", { payload: o }),
  rect: (o) => invoke("plugin:multiline-taskband|rect", { payload: o }),
  isVisible: (o) => invoke("plugin:multiline-taskband|is_visible", { payload: o }),
};

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------
async function createInstance(id, side, top, bottom) {
  await api.create({ id, side, top, bottom }).catch((e) => console.error(`create ${id}:`, e));
  created.add(id);
  await listen(`multiline-taskband://${id}//ready`, () => {
    document.querySelector(`[data-id="${id}"] .ready-badge`).textContent = "ready";
  });
  renderList();
  updateStatus();
}

async function removeInstance(id) {
  await api.remove({ id }).catch((e) => console.error(`remove ${id}:`, e));
  created.delete(id);
  renderList();
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
  for (const id of created) {
    ul.appendChild(instanceCard(id));
  }
}

function instanceCard(id) {
  const preset = PRESETS.find((p) => p.id === id);
  const side = preset ? preset.side : "right";
  const li = document.createElement("li");
  li.className = "instance-card";
  li.dataset.id = id;

  const head = document.createElement("div");
  head.className = "instance-head";
  head.innerHTML = `
    <span class="instance-name">${id}</span>
    <span class="badge side-badge">${side}</span>
    <span class="badge ready-badge">pending</span>
    <label class="switch" title="显示/隐藏">
      <input type="checkbox" class="vis-toggle" checked />
      <span class="slider"></span>
    </label>
    <button type="button" class="remove-btn">删除</button>
  `;
  li.appendChild(head);

  // --- top/bottom text ---
  li.appendChild(textRow("文本", preset ? preset.top : "", preset ? preset.bottom : ""));

  // --- font sizes ---
  li.appendChild(fontRow());

  // --- layout ---
  li.appendChild(layoutRow());

  // --- colors (default / #rrggbb, per line) ---
  li.appendChild(colorRow("上行颜色", "top"));
  li.appendChild(colorRow("下行颜色", "bottom"));

  // --- bold & alignment ---
  li.appendChild(boldAlignRow());

  // --- rect / isVisible ---
  li.appendChild(rectRow());

  bindEvents(li, id, side);
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
    <input type="number" class="top-size" value="9" min="6" max="24" step="0.5" title="上行字号" />
    <input type="number" class="bottom-size" value="12" min="6" max="24" step="0.5" title="下行字号" />
  `;
  return row;
}

function layoutRow() {
  const row = document.createElement("div");
  row.className = "field-row";
  row.innerHTML = `
    <label>布局</label>
    <select class="layout">
      <option value="0">0 emphasis-bottom（上小下大）</option>
      <option value="1">1 emphasis-top（上大下小）</option>
      <option value="2">2 equal（等大居中）</option>
    </select>
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

// ---------------------------------------------------------------------------
// Event wiring (fire-and-forget; the plugin marshals to its UI thread)
// ---------------------------------------------------------------------------
function bindEvents(li, id, side) {
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

  $(".layout").addEventListener("change", (e) =>
    api.setLayout({ id, layout: Number(e.target.value) })
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

// Create the 5 presets on boot.
(async () => {
  for (const p of PRESETS) {
    await createInstance(p.id, p.side, p.top, p.bottom);
  }
})();
