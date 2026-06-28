# ADR-0003: 后端使用 Rust

- **状态 (Status):** accepted
- **日期 (Date):** 2026-06-28
- **决策者 (Deciders):** msuad + Claude

## 上下文 (Context)

Stage 2 需要"可选后端"以支持联机、远程持久化、权威状态。后端语言候选为 Rust 与 Go。
约束：
- engine 已选 Rust（[ADR-0002](0002-engine-rust-wasm.md)）。
- 桌面壳为 Tauri（Rust）。
- 本机已装 Rust 1.96、未装 Go。
- 项目推崇防御式编程与强类型。

## 决策 (Decision)

**后端使用 Rust。** 与 engine、Tauri 同语言栈，复用 engine crate。

## 备选方案 (Alternatives Considered)

- **A. Go** — 并发简单、单二进制部署轻；但本机未安装、与 engine/Tauri 不同栈，且需跨语言共享引擎逻辑（成本高）。
  不选：既然 engine 已是 Rust，跨语言复用的代价使 Go 的并发便利性得不偿失。
- **B. Node.js / TS 后端** — 否决：与 engine（Rust）仍跨语言，且偏离用户给定的"Rust 或 Go"候选。

## 后果 (Consequences)

- **正面：** engine crate 可被后端直接依赖，无需 WASM 中转；类型与错误模型（`Result`）天然契合防御式编程；与 Tauri 同栈，技能与工具链复用。
- **负面：** Rust 后端生态在"开箱即用的 Web 框架便利性"上不如 Go/Node，部分功能需更多手写。
- **后续需要做的：**
  - Stage 1 不需要后端；Stage 2 起步时选定 Web 框架（如 Axum）并补 ADR。
  - 后端复用 engine crate 的方式（直接依赖 vs 子 crate 拆分）待 engine 成形后定。

## 关联 (Related)

- 关联开放问题：[`open-questions.md`](../open-questions.md) Q2（已解决）
- 关联 ADR：[ADR-0002](0002-engine-rust-wasm.md)（engine = Rust）
- 配套：[`tech-stack.md`](../tech-stack.md)
