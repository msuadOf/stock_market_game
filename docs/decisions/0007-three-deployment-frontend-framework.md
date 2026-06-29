# ADR-0007: 三端前端框架（host-adapter 统一 + Blueprint + AG Grid + Lightweight Charts）

- **状态 (Status):** accepted
- **日期 (Date):** 2026-06-29
- **决策者 (Deciders):** msuad + Claude
- **关联 (Related):** [ADR-0005](0005-unified-engine-three-deployments.md)（三端部署、host-agnostic 应用层）、[ADR-0004](0004-frontend-state-redux-toolkit.md)（RTK）。解决 [open-questions.md](../open-questions.md) Q3（许可证）、Q4（包管理器）。

## 上下文 (Context)

engine 已完成（7 模块 + 初始持仓，144 测试绿、市场转活），但无前端/宿主，玩家无法玩。需一次性搭建 ADR-0005 的三端部署框架，让**同一份 engine + 同一份前端代码**以三种方式跑起来：① 纯前端（engine→WASM→Web Worker）② 前后端分离（Axum 后端，WS+REST）③ Tauri 桌面。msuad 2026-06-29 敲定了全部前端细节选择（见 Decision）。

## 决策 (Decision)

### §1 一份前端 + EngineHost 适配器（核心）
- 前端代码**完全相同**，只换「如何连 engine」的适配器：`WasmWorkerHost` / `RemoteHost`(WS+REST) / `TauriHost`(invoke/listen)。
- 统一 `EngineHost` TS 接口：`start/stop/setSpeed/submitIntent/snapshot`。
- 三端共用 `packages/engine`（rlib，权威逻辑）。

### §2 前端技术栈
- **React + Vite + TypeScript + Redux Toolkit**（ADR-0004）。
- **UI 库**：**Blueprint.js**（数据组件/交互：Table/Tree/表单/Overlay），**视觉样式自定义为贴 ref 的亮色券商风**（白底 `#f4f4f4`、红涨 `#d81e06`/绿跌 `#009944`/平 `#b8b8b8`、微软雅黑、`tabular-nums`）——覆盖 Blueprint 默认暗色。
- **主题**：默认亮色 + **亮/暗主题开关**（CSS 变量驱动）。
- **界面语言**：**全中文、无西文**（Blueprint 英文标签本地化覆盖）。
- **行情表格**：**AG Grid**（Community，免授权）。
- **K 线/分时图**：**TradingView Lightweight Charts**——价格 + 量能 pane（默认）+ **MACD、KDJ 可选指标开关**（默认关）。
- **桌面布局**：**React Grid Layout**（可拖拽缩放工作台）。

### §3 布局（桌面/移动统一组件、仅布局不同）
- **桌面**：顶栏（设置+账户）｜左自选/行情（AG Grid）｜右上主图（日K/分时）｜右下操作面板（买/卖 + 自动单/条件单）+ 盘口/成交/持仓面板。
- **移动**：仿 `ref/模拟股市（2）.html`——底部 tab + 详情页 + 交易底页。
- **切换判据**：横屏/竖屏（`width >= height` → 桌面；否则移动）。

### §4 WASM 边界
- `GameSession` 不可序列化（含 `Box<dyn Strategy>`）→ **handle API**（u32 句柄 + JSON 边界），仅 `Snapshot`/`Event`/`Intent`/`SessionSetup` 跨界。
- WASM 跑在 **Web Worker**（不卡 UI）；720x 高频按 rAF 节流（PriceTick 合并、Trade 全保留）。

### §5 类型同步
- 从 Rust serde 类型用 **typeshare** 生成 TS（`apps/web/src/types/engine.ts`）—— Rust↔TS 单一真源，不手写漂移。

### §6 多线程 + GPU（架构预留）
- **多线程**：engine 加 `Send + Sync`（`GameSession: Send`）；宿主 **actor-per-session**（无锁、无共享可变状态，契合 ADR-0005）。详见 [WS-1 + plan](../../C:/Users/msuad/.claude/plans/sharded-skipping-liskov.md)。
- **GPU offload（可选，默认关）**：`ComputeBackend` trait + `CpuBackend`(rayon) + `engine-gpu`(wgpu) crate + `ComputeMode` 配置；wgpu 跨 native/WebGPU 三端复用；**确定性用 GPU 整数/定点计算解决**（i32/i64 bit-精确，契合 Money=i64）；为大规模 NPC 预备，当前规模默认关。

### §7 包管理器与许可证
- **包管理器 = pnpm**（pnpm-workspace.yaml）——解决 Q4。
- **许可证 = MIT**（维持）——解决 Q3。

## 备选方案 (Alternatives Considered)

- **Ant Design**：生态大，但密集金融数据界面不如 Blueprint（Palantir 为金融/情报分析设计）顺手。不选。
- **antd-mobile（移动优先）**：贴 ref，但桌面（Tauri）体验弱；改用「统一组件 + 按横竖屏切布局」。不选。
- **Mutex<GameSession> 并发**：可行但锁风险（死锁/长持锁）；改用 actor-per-session（命令 channel + 单线程独占）。不选。
- **GPU 用 f32**：非确定（跨厂商 FP 差异）；改用整数/定点（契合 Money=i64）。不选。
- **类型手写 TS**：会漂移；改用 typeshare 生成。不选。

## 后果 (Consequences)

- **正面：** 一份前端三端跑（最大复用）；engine Send+Sync 解锁多线程宿主（后端并发、Tauri、未来联机）；GPU seam 为规模化预备且保确定性；typeshare 杜绝类型漂移；handle API 让 WASM 干净。
- **负面 / 代价：**
  - 体量大（WS-4 前端做全是主要工作量）；Blueprint+AG Grid+Lightweight Charts 三库集成有学习曲线。
  - Blueprint 主题/中文本地化是确定的样式工作。
  - `Send+Sync` 是 pervasive 小改（trait bound），须跑全量回归。
  - wgpu 重依赖；本轮只搭 seam（默认关），真实 GPU 内核待规模化。
- **后续需要做的：**
  - 落地 WS-0..WS-6（见 plan 文件）。
  - 解决 open-questions Q3（MIT）、Q4（pnpm）——本 ADR 已定。

## 关联 (Related)

- [ADR-0005](0005-unified-engine-three-deployments.md)（三端部署、应用层契约）。
- 解决：[open-questions.md](../open-questions.md) Q3（MIT）、Q4（pnpm）。
- 实现计划：`docs/superpowers/plans/` + 会话 plan 文件。
