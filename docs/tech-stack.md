# 技术栈 (Tech Stack)

> 本文档记录各层的技术选型。**已敲定的标 ✅，待定的标 ⏳**（待定项见 [`open-questions.md`](open-questions.md)，决策进 ADR）。

---

## 总览

| 层 | 技术 | 状态 | 备注 |
|----|------|------|------|
| 前端框架 | **React** | ✅ 已定 | 用户指定 |
| 前端构建 | Vite | ⏳ 建议 | React 生态主流、快 |
| 前端语言 | TypeScript | ✅ 已定 | 前端 + WASM 绑定层 |
| 状态管理 | **Redux Toolkit** | ✅ 已定 | [ADR-0004](decisions/0004-frontend-state-redux-toolkit.md) |
| 桌面壳 | **Tauri 2** | ✅ 已定 | 本机已装 (2.11)；用户指定 |
| 游戏核心 (engine) | **Rust → WASM** | ✅ 已定 | [ADR-0002](decisions/0002-engine-rust-wasm.md)；`wasm-pack`/`wasm-bindgen` |
| 后端语言 | **Rust** | ✅ 已定 | [ADR-0003](decisions/0003-backend-rust.md)；Stage 2 起 |
| 包管理 | npm | ⏳ 建议 | 本机已具备；是否升级 pnpm 见 Q4 |
| 测试 (TS) | Vitest | ⏳ 建议 | Vite 原生、快 |
| 测试 (Rust/engine) | `cargo test` | ✅ | 随 Rust；engine 单元测试主战场 |
| E2E | Playwright | ⏳ 建议 | 跨浏览器 |
| Lint/格式化 | ESLint + Prettier (TS) / rustfmt+clippy (Rust) | ⏳ 建议 | |

---

## ✅ 已敲定的关键技术决策

### engine：Rust → WASM（[ADR-0002](decisions/0002-engine-rust-wasm.md)）
- `packages/engine` 是 Rust crate，纯逻辑，参与 cargo workspace。
- 前端经 `wasm-pack`/`wasm-bindgen` 调用；后端与 Tauri 直接复用同一 crate。
- 状态以 JSON 序列化跨端传输。

### 后端：Rust（[ADR-0003](decisions/0003-backend-rust.md)）
- Stage 1 不需要；Stage 2 起接入，直接依赖 engine crate。
- Web 框架待 Stage 2 时补 ADR。

### 前端状态：Redux Toolkit（[ADR-0004](decisions/0004-frontend-state-redux-toolkit.md)）
- engine（Rust/WASM）持有权威游戏状态；RTK store 存 UI 状态 + engine 结果快照，不重复计算业务规则。
- 意图派发 → 调 engine → 用结果更新 store → 渲染。

---

## ⏳ 仍待定（影响技术栈的）

> 详见 [`open-questions.md`](open-questions.md)。

### Q4. 包管理 / monorepo 工具
- npm workspace（零额外工具）vs pnpm workspace（更快、磁盘省）。
- 倾向 npm 起步；但 **engine 是 Rust crate（cargo workspace）**，前端用 npm workspace，两者并存。

### 其余待定（Q3 许可证、Q6–Q10）见开放问题清单，不阻塞 Stage 1。

---

## 已敲定的依据

- **React**：用户明确指定。
- **Tauri 2**：用户明确指定；本机已安装 `tauri-cli 2.11.3`。
- **engine = Rust→WASM / 后端 = Rust / 状态 = RTK**：msuad 拍板（2026-06-28），见对应 ADR。

## 待定项的决策流程

每个 ⏳ 都将通过 ADR 敲定（[`decisions/`](decisions/)）。**未敲定前不擅自引入。**
