# setLeadingIcon — instance 级列图标（leading icon column）

日期：2026-09-09 · 分支：`feat/icon-rendering` · 对齐：multiline-menubar v1.10.0 `setLeadingIcon`

## 需求

新增一种图标渲染方式：图标不跟在某一行的文字前面，而是独占左侧一列 —— 一个较大的图标在整块高度内垂直居中，上下两行文字从它右侧开始渲染。与 menubar 插件 v1.10.0 的列图标语义一一对齐（跨插件 API 一致性是当前主线）。

## API（对齐 menubar 命名）

```ts
await setLeadingIcon({ id: 'mb-1', icon: { data: svg, tint: true, size: 22 } })
// icon: null = 清除列图标 → 回到 inline 行为
// raw invoke: set_leading_icon
// Rust: set_leading_icon(id, icon: Option<IconSpec>)
```

- **不新增模式开关字段**：配置了有效的 leading 图标 → 进入列布局；清除 → 回 inline。
- **`IconSpec` 增加可选 `size: number`**（物理像素）：仅列图标使用，行内图标忽略。缺省 = 整块内容区高度（撑满）；显式值 clamp `8 … 块高`。
- **两层共存**：列图标占左列，行内 `setIcon` 图标在剩余文字区内照常 inline 渲染。
- **popup-open 快照**：payload 增加 `leadingIcon: IconSpec | null`。
- **权限**：`allow-set-leading-icon` 进 default；build.rs COMMANDS 增加 `set_leading_icon`。
- tint 语义沿用：跟随**首个可见行**的颜色（alpha 即 coverage）。

## 渲染（windows.rs）

- `measure`：先照旧算两行（行内 icon + gap + text），块高 `h = Σ行cell + LINE_GAP`；有有效 leading 图标时列宽 `= width_for_height(src, target_h) + ICON_GAP`（有非空文字时），总宽 `= max(行 content) + pads + col_w`。
- `paint_inst`：列图标 blit 在 `x = pad_left`、`y = (h - icon.h) / 2`；每行 group_x = `col_w + align_x(align, group_w, w - col_w, pad_left, pad_right)` —— 对齐在剩余文字区内照常工作（左/中/右语义不变）。
- 解码懒执行语义不变：`measure` 与 `paint` 共用 `icon_source`/`ICON_CACHE`，解不出 → col_w = 0，文字顶到最左，两处宽度一致不跳变。
- 去重：handler 对比新旧 spec（`IconSpec: PartialEq`，size 在 spec 内，size 变化自然触发 relayout）；同 spec 重发跳过 relayout。

## 边界情况

| 情况 | 行为 |
|---|---|
| spec 无效（path/data 双给或双无） | commands 层拒绝（InvalidArgument），与 set_icon 一致 |
| 资产缺失/解不出 | eprintln + 无列渲染（文字顶到最左），文件补上后下次 paint 恢复 |
| `size` ≤ 0 或超大 | clamp `8 … 块高` |
| 双行全隐 + 有可用列图标 | item 保留、仅渲染居中图标（水平垂直都居中）；`effective_visible` 扩展；清除图标或解码失败 → 回到隐藏离场 |
| 双行全隐 + 无 size | 图标兜底高度 32px（`ICON_ONLY_FALLBACK_SIZE`） |
| 右侧任务栏 / 对齐 | 列图标恒在最左，行对齐在剩余区内照常工作 |

## demo（examples/demo）

- popup 新增 **Leading icon** 区块：预设下拉（`iconPresets.js`）+ tint + size 数字输入（0 = 自动/整块高）+ 预览缩略图。
- 顶部任务条模拟区改为 `chip > [lead img] + lines 列` 结构，实时预览列布局。
- localStorage `settings-v1` 实例记录增加 `leadingIcon`；`popup-open` 回显；Reset appearance 一并清除。
- 版本 bump 1.0.3 → 1.0.4（tauri.conf.json + Cargo.toml + Cargo.lock）。

## 验证

`cargo check`（本机）→ `scripts/build-release-windows.mjs --arch arm64`（先 `export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8`，防 makensis Unicode 崩溃）→ VM/实机验证：列宽、垂直居中、两层图标共存、双行隐藏保留、与 Start/widgets 避让共存。
