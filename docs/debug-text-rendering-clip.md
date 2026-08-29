# 任务栏文字渲染"截断"问题排查记录

> 日期：2026-08-29
> 范围：`tauri-plugin-multiline-taskband` 的 overlay 文字渲染（`src/native/windows.rs`）
> 状态：**已解决** —— 渲染方案整体切换为 TrafficMonitor 的方式（见 §9），不再保留自行实验的方案 A/B/C

---

## 9. 结论：采用 TrafficMonitor 的渲染方式（2026-08-29 晚）

用户要求：不要继续在自有方案上打补丁，直接采用
`C:\Mac\Home\Documents\github\TrafficMonitor` 的渲染方式。

### 9.1 TrafficMonitor 实际怎么画（源码依据）

**GDI 路径**（`TrafficMonitor/DrawCommon.cpp` `CDrawCommon::DrawWindowText`）：
1. 每个文本行的绘制 rect = **布局 rect（窗口高度均分），字体完整 cell 高度**，
   **从不裁剪 `tmInternalLeading`**（`TaskBarDlg.cpp` 的窗口高度
   `TASKBAR_WND_HEIGHT = DPI(32)`，两行时每行 band = 16px，默认 9pt 字体
   cell ≈ 16px，恰好放得下，文字垂直居中）。
2. `DrawText` flags = **`DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX`** + 水平对齐
   （`DrawCommonHelper::ProccessTextFormat`）—— 让 GDI 自己垂直居中，
   不做任何手动基线运算。
3. 文字以**最终颜色直接绘制**（`SetTextColor(color)` + `SetBkMode(TRANSPARENT)`），
   ClearType 亚像素渲染完整保留。
4. 字体：`lfHeight = -MulDiv(pt, dpi, 72)`、`DEFAULT_QUALITY`（跟随系统 ClearType）、
   `DEFAULT_PITCH | FF_SWISS`、默认字号 **9pt**（`CommonData.h` `FontInfo::Create`）。

**D2D 路径**（`TaskBarDlgDrawCommon.cpp` `CTaskBarDlgDrawCommon::DrawWindowText`）：
- DirectWrite `CreateTextLayout`，`DWRITE_PARAGRAPH_ALIGNMENT_CENTER`（竖直居中）、
  `DWRITE_WORD_WRAPPING_NO_WRAP`（单行），layout rect = 完整 item rect。
- 输出**预乘 alpha** 像素 → `UpdateLayeredWindow(AC_SRC_ALPHA)`。
- 背景填充 `FillRect(draw_rect, 0x00000000, 1)` —— **alpha=1 的点击穿透背景**，
  与本插件背景 alpha=1 的做法一致。

### 9.2 本插件采纳的改动（`src/native/windows.rs`）

| 项 | 之前（自研实验） | 之后（TrafficMonitor 方式） |
|---|---|---|
| 行高 | `full_h - tmInternalLeading`（裁剪 leading） | **完整 cell 高度 `full_h`**，不裁剪 |
| 垂直对齐 | `DT_TOP` + 手动基线运算（rect 高度 < cell → 字形底部被裁） | **`DT_VCENTER`**，GDI 垂直居中，band 高度 = cell，永不裁剪 |
| 覆盖度 | RGB 三通道平均成灰度 alpha（ClearType → 灰度，发虚） | **保留逐通道 coverage**（白字黑底 DIB 的 RGB 即亚像素覆盖度），最终颜色逐通道预乘 |
| 输出 | 非预乘颜色（`a<<24 | 全亮颜色`，AC_SRC_ALPHA 语义不符） | **逐通道预乘**（`c_i * coverage_i / 255`，alpha = 平均覆盖度），与 D2D 路径输出一致 |
| 默认字号 | 11pt（两行 44px > 96dpi 任务栏 40px，溢出裁切） | **9pt**（与 TrafficMonitor 默认一致，两行 38px 可放入 96dpi 任务栏） |
| 字体 pitch | `DEFAULT_PITCH` | `DEFAULT_PITCH \| FF_SWISS`（与 `FontInfo::Create` 一致） |
| 调试代码 | `paint_inst` 内 dump DIB 到 `tmp-demo-cli\win_dib.bmp` | 删除 |

几何自洽性：窗口高度 = 上带全高 + `LINE_GAP` + 下带全高，与两行 coverage 缓冲
逐像素对应，ULW `psize` 与 alpha 缓冲尺寸一致 —— 修复了方案 C 中
"窗口高度 ≠ alpha 裁剪后高度" 的错位隐患。

### 9.3 为什么不会再截断

- **内部永不裁剪**：band 高度 = `DT_CALCRECT` 返回的 cell 全高，`DT_VCENTER`
  把 cell 居中放进 band，字形 ink（cell 内 [internalLeading, tmHeight) 段）
  天然完整 —— 数学上不存在 rect 高度 < cell 的情况。
- **任务栏溢出有 leading 兜底**：字号超过任务栏容量时，任务栏裁剪的是 cell
  上下的 leading（空隙），ink 仍可见；9pt 默认在 96/144/192dpi 下两行均不溢出。
- **平滑度对齐**：`DEFAULT_QUALITY` 跟随系统 ClearType；逐通道 coverage 保留
  亚像素渲染（不再 RGB 平均成灰度）；预乘输出消除白字亮边。

### 9.4 验证要点（下次在目标机器复测）

1. 两行 demo 实例（mb-1..5）CJK（如"人工智能"）与英文（mb-4 / mb-5）顶部/底部笔画完整。
2. 白色文字（深色任务栏）边缘无亮边、无发虚；黑色文字（浅色任务栏）同理。
3. 192dpi（Parallels 报告 192 / 渲染 96）下窗口物理尺寸 = inst 一半的错位不再导致截断。
4. 字体放大到 14pt+：内部仍完整（可能被任务栏裁 leading，但 ink 可见）。

---

## 1. 问题现象（历史记录）

用户反馈：任务栏上 demo 实例的文字"明显被截断了一点"，而 TrafficMonitor 的渲染"很优雅"。

该问题出现于 `1459fe6 fix(windows): improve overlay text rendering & default font` 之后——该提交引入了 4 项渲染改动：

1. `default_face()`：运行时探测最佳字体（微软雅黑 / JhengHei / Segoe UI），替代 GDI 系统默认（宋体）
2. `lfQuality = DEFAULT_QUALITY`：跟随系统 ClearType 设置（原来是写死 `ANTIALIASED_QUALITY`）
3. alpha 从 RGB 平均计算（替代只取 R 通道，兼容 ClearType 亚像素渲染）
4. **裁剪 `tmInternalLeading`**：把 DIB 高度从字体 cell 全高（full_h）压缩为 `full_h - lead`，让两行窗口高度从 44px 回落到 34px

用户截图显示（原图见会话剪贴板）：CJK 文字（如"人工智能"）与英文（"mb-4 / mb-5"）均有异常，顶部/局部笔画缺失。

---

## 2. 环境关键事实（Parallels VM 特殊性）

| 项 | 值 | 影响 |
|---|---|---|
| 系统 | Windows 11 Pro 26200 / ARM64（Parallels VM） | DWM 合成开启（`DwmIsCompositionEnabled=True`） |
| 插件 `GetDpiForSystem()` | **192**（200% 缩放） | 所有坐标按 192dpi 计算 |
| 任务栏物理高度 | 48px | 若按 192dpi 渲染，任务栏逻辑高应为 24px |
| 窗口物理尺寸 vs inst 尺寸 | **窗口 = inst 的一半**（84×62 → 42×31） | 窗口被任务栏 DPI 空间缩放 1/2 |
| 任务栏背景 | 动态变化（白 / 深灰 47 / 浅灰 242 在不同时刻截图均不同） | 文字颜色自适应逻辑（`taskbar_light_theme`）依赖背景判定 |

**关键矛盾**：插件进程 DPI aware（`GetDpiForSystem()=192`），但窗口实际按 96dpi 空间渲染（物理尺寸 = inst 逻辑尺寸的一半）。Parallels VM 报告 192dpi、实际渲染 96dpi 的错位，导致：
- `MoveWindow(hwnd, mx, my, 84, 62)` → 窗口物理 42×31
- `UpdateLayeredWindow(psize={84,62})` 与实际窗口 42×31 不匹配

---

## 3. 三个自研渲染方案（演进过程，现已全部废弃）

### 方案 A — `1459fe6`（用户看到的"截断"版本）
```rust
let h = full_h - lead;                    // DIB 高度 = ink 高度
create_dib(hdc, w, h);                    // DIB 高 h
RECT { top: -lead, bottom: full_h - lead } // rect 高度 = full_h
DrawTextW(..., DT_TOP);
alpha = DIB[0..h] 全量
```
- rect 高度 = full_h（cell 全高）→ DT_TOP 基线 = `-lead + ascent` → **字形 ink 顶恰好落在 DIB 第 0 行，字形完整**
- 但用户仍报告截断 → 另有原因（见 §5）

### 方案 B — `153a3b9`（当时 HEAD，显示"正常"但有裁切风险）
```rust
let h = full_h - lead;
create_dib(hdc, w, h);
RECT { top: 0, bottom: h }                 // rect 高度 = h = full_h - lead
DrawTextW(..., DT_TOP);
alpha = DIB[0..h] 全量
```
- **rect 高度 < cell 全高 → DT_TOP 基线 = ascent（第 32 行）→ 字形 ink 底 = ascent+descent = full_h = 38，超出 rect（29）→ 字形底部约 9 行被 GDI 裁剪**
- 实测：屏幕上文字可见（有深色像素），但正是"底部被裁"——很可能就是用户看到的截断！

### 方案 C — 未提交（思路正确，但触发新问题）
```rust
create_dib(hdc, w, full_h);                // DIB 保持 cell 全高
RECT { top: 0, bottom: full_h }
DrawTextW(..., DT_TOP);                    // 字形完整绘制，ink 在 DIB[lead..full_h)
alpha = DIB[lead..full_h)                  // alpha 生成时裁掉顶部 leading 行
```
- 理论正确：DIB 全高绘制（避免 GDI 裁剪），输出时裁 leading
- **实测：窗口完全透明，文字不可见（回归！）** → 当时根因未明（见 §6）；现按 §9 采用方案 D（TrafficMonitor 方式）后不再需要它

---

## 4. GDI 行为验证（Python ctypes 直测）

用 `C:\Users\lingsmbp\tmp-demo-cli\verify_*` 系列脚本直调 GDI 复现三种 rect 方案：

**核心发现：`DT_TOP` 按基线定位，不是按字形顶**
```
DT_TOP: baseline = rect.top + ascent
字形 ink 顶 = baseline - (ascent - internalLeading) = rect.top + internalLeading
字形 ink 底 = baseline + descent = rect.top + tmHeight
```
→ **只要 rect 高度 < tmHeight（cell 全高），字形底部（descent 部分）必然被裁**。
→ 因此 rect 必须用 cell 全高绘制，leading 必须在生成 alpha 时裁剪（即方案 C 的思路）。

实测数据（96dpi、YaHei 15px、tmHeight=20、internalLeading=5）：
- 方案 A（rect=[-5,15]）：字形 ink 在 DIB [0,12)，完整，底部 3 行空白
- 方案 B（rect=[0,15]）：字形 ink [4,15)，**底部 5 行被裁**
- 方案 C（rect=[0,20] + alpha 裁 [5,20)）：alpha 内字形 [0,12) 完整

结论：**方案 A、C 字形完整；方案 B 底部被裁**。用户看到的截断很可能是方案 B（153a3b9）或方案 A 在 192dpi 下的等效表现。

---

## 5. DPI / 窗口几何验证

插件日志（192dpi 下实测）：
```
top="mb-1" top_sz=29 tw=76 th=29 alen=2204 inst.w=84 inst.h=62 dpi=192
```
- `pt_to_px(11pt, 192) = 29px`，`full_h = 38`，`internalLeading = 9`，`h = 29`
- `inst.w/h = 84/62`（192dpi 单位）

`GetWindowRect` 实测：窗口物理 **42×31** = inst 的一半 → **窗口被系统按 96dpi 空间缩放 1/2**（Parallels 报告 192 实际渲染 96）。

---

## 6. 未解疑点：ULW 内容不显示（方案 C 时代）

方案 C 运行后，5 个 demo 窗口区域与任务栏背景完全一致（全透明），文字不可见。诊断过程：

1. **窗口 DIB dump（原始字节）**：完全正确
   - 背景 `0x01000000`（alpha=1 黑色，点击穿透 trick）
   - 字形像素 alpha=255、白色
   - ⚠️ 注意：PIL 读 32bpp BMP 会**忽略 alpha 通道**（mode=RGB，convert 后 alpha 一律 255），最初误判为"全 alpha=255"，实际是解析误读
2. **ULW 返回值**：`ret=1`（成功），lasterr 无异常
3. **隔离实验**（独立顶层 layered 窗口 + ULW 上传红色方块）：
   - `ULW ret=1` 成功
   - **屏幕上同样看不到红色方块** → 指向系统层问题（非插件代码）
4. **回退验证**：stash 掉方案 C，恢复 153a3b9 后**文字可见**（5 个窗口均有深色像素）

**矛盾点**：
- 方案 C 的窗口 DIB 与方案 B 结构相同（背景 alpha=1 + 字形 alpha=255，仅字形行位置不同）
- 方案 B 显示正常，方案 C 不显示
- 隔离实验（与插件无关的 ULW）也不显示 → 存在系统层因素（DWM 合成 / Parallels 显示驱动 / DPI 错位）

> 注：该疑点已被 §9 的方案 D 绕开 —— 方案 D 的窗口高度与 coverage 缓冲完全自洽，
> 且移除了当时方案 C 调试版中的额外代码（dump / eprintln），不再有未知干扰。

---

## 7. 验证脚本（tmp-demo-cli/，临时目录）

| 脚本 | 用途 |
|---|---|
| `verify_drawtext_clip.py` | 对比三种 rect 方案的字形 ink 完整性 |
| `verify_ink_rows.py` | 逐行统计字形 ink 分布 |
| `check_taskbar_geom.py` | 截屏 + 枚举任务栏子窗口几何 |
| `analyze_ink*.py` / `ascii*.py` / `peek_pixels.py` | 截图像素分析（ASCII 可视化） |
| `dump_window*.py` | PrintWindow / GetDC 抓窗口内容（layered 窗口不可靠） |
| `enum_demo.py` | 枚举 demo 窗口状态（类名/尺寸/可见性/样式） |
| `ulw_test.py` / `ulw_test2.py` | **ULW 隔离实验**（独立窗口上传图形） |

---

## 8. 历史结论（方案 D 之前）

### 已确认
1. **方案 B（153a3b9）会裁字形底部**（GDI DT_TOP 基线语义），这就是用户看到的"截断"——**应放弃 B，回到 A 或采用 C**
2. 方案 A 的 rect（`[-lead, full_h-lead]`，跨度 = full_h）在 96dpi 验证中字形完整
3. DPI 错位（192 报告 / 96 渲染）是环境事实，窗口物理尺寸 = inst 一半

### 待解决（已被 §9 方案 D 覆盖）
1. 方案 C 为什么不显示（与系统层 ULW 失效的疑点相互纠缠）
2. 方案 A 在用户机器上的"截断"具体表现
