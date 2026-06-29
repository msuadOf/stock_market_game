# ADR-0002: 游戏核心引擎用 Rust 编译为 WASM

- **状态 (Status):** accepted（核心决策保留；部署拓扑关系被 [ADR-0005](0005-unified-engine-three-deployments.md) 细化）
- **日期 (Date):** 2026-06-28
- **决策者 (Deciders):** msuad + Claude

## 上下文 (Context)

游戏需在多端运行：浏览器（Stage 1）、可选后端（Stage 2）、Tauri 桌面（Stage 3）。
核心逻辑（市场模拟、撮合、组合计算）是"后端可选"与"多端一致"的地基，
需要满足：纯逻辑（无副作用）、可序列化状态、跨端复用同一份实现。

关键张力：
- 纯 TS 路线 TDD 最顺、Stage 1 起步快，但多端需手动维护多份引擎。
- Rust→WASM 路线单一代码源，但 WASM 桥接与构建链更复杂、起步门槛高。

## 决策 (Decision)

**核心引擎用 Rust 实现，前端通过编译产物 WASM 调用。**

- `packages/engine` 是一个 Rust crate（纯逻辑，无 I/O），参与 cargo workspace。
- 前端通过 WASM 绑定（`wasm-bindgen` / `wasm-pack`）调用引擎。
- 后端（Rust）与 Tauri 桌面**直接复用同一个 crate**。
- 状态序列化为与语言无关的格式（JSON），跨端传输。

## 备选方案 (Alternatives Considered)

- **A. 纯 TypeScript** — TDD 顺、起步快、无 WASM 复杂度；但多端需维护多份引擎，一致性靠人工。
  不选：本项目把"多端一致性"作为一等目标，单一代码源价值更高。
- **B. TS engine + Rust 后端各自一份** — 否决：双份引擎的同步成本随玩法增长而失控。

## 后果 (Consequences)

- **正面：** 一份 Rust 引擎跑遍 web/server/desktop；Rust 类型系统天然支撑"防御式编程"与不变量断言；性能充裕。
- **负面：**
  - WASM 构建/桥接引入复杂度（`wasm-pack`、类型映射、序列化边界）。
  - TDD 在 Rust 侧（`cargo test`）顺，但在"前端调用 WASM"的集成层较绕——需分层测试策略。
  - Stage 1 起步门槛高于纯 TS（需先搭好 WASM 工具链）。
- **后续需要做的：**
  - 确立 WASM 桥接约定：引擎只暴露"输入状态+动作 → 新状态+事件"的纯接口，类型边界序列化清晰。
  - 测试分层：引擎单元测试用 `cargo test`（快、纯）；WASM 桥接层用少量集成测试覆盖。
  - 引入 `wasm-pack` / `wasm-bindgen` 等工具链，记录在 tech-stack.md。

## 关联 (Related)

- 关联开放问题：[`open-questions.md`](../open-questions.md) Q1（已解决）
- 关联 ADR：[ADR-0003](0003-backend-rust.md)（后端语言顺势选 Rust）
- 配套：[`architecture.md`](../architecture.md)、[`tech-stack.md`](../tech-stack.md)
