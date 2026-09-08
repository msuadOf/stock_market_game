# 技术栈 (Tech Stack)

> 本文档记录各层的技术选型。**已敲定的标 ✅，待定的标 ⏳**（待定项见 [`open-questions.md`](open-questions.md)，决策进 ADR）。

---

## 总览

| 层 | 技术 | 状态 | 备注 |
|----|------|------|------|
| 前端框架 | **React** | ✅ 已定 | 用户指定 |
| 前端构建 | Vite 8 | ✅ 已定 | React 开发与生产构建 |
| 前端语言 | TypeScript | ✅ 已定 | 前端 + WASM 绑定层 |
| 状态管理 | **Redux Toolkit** | ✅ 已定 | [ADR-0004](decisions/0004-frontend-state-redux-toolkit.md) |
| 桌面壳 | **Tauri 2** | ✅ 已定 | 本机已装 (2.11)；用户指定 |
| 游戏核心 (engine) | **Rust → WASM** | ✅ 已定 | [ADR-0002](decisions/0002-engine-rust-wasm.md)；`wasm-pack`/`wasm-bindgen` |
| 后端语言 | **Rust** | ✅ 已定 | [ADR-0003](decisions/0003-backend-rust.md)；Stage 2 起 |
| 包管理 | pnpm 11.19 | ✅ 已定 | workspace 与冻结锁文件 |
| 测试 (TS) | Node test runner | ✅ 已定 | 直接运行 TypeScript 行为/契约测试 |
| 测试 (Rust/engine) | `cargo test` | ✅ | 随 Rust；engine 单元测试主战场 |
| Rust→TS 类型生成 | `ts-rs` 12 | ✅ 已定 | 保持 serde 线协议，CI 检查生成漂移 |
| E2E | Playwright 1.63 + Chromium | ✅ 已定 | production preview 下验证桌面与移动主链路 |
| Lint/格式化 | Oxlint（TS）/ rustfmt + clippy（Rust） | ✅ 已定 | CI 以 warning 为错误 |

---

## ✅ 已敲定的关键技术决策

### engine：Rust → WASM（[ADR-0002](decisions/0002-engine-rust-wasm.md)）
- `packages/engine` 是 Rust crate，纯逻辑，参与 cargo workspace。
- 前端经 `wasm-pack`/`wasm-bindgen` 调用；后端与 Tauri 直接复用同一 crate。
- 多线程 WASM 构建使用固定的 `nightly-2026-09-05` + `rust-src`；项目根目录 `.cargo/config.toml`
  启用 `atomics`/`bulk-memory`/`mutable-globals` 与 `build-std`。针对该 nightly，根配置还显式
  启用 shared/import memory，并导出 wasm-bindgen 0.2.93 线程转换所需的堆/TLS 符号；
  `scripts/check-wasm-threading.mjs` 会拒绝产出非共享内存的伪多线程绑定。执行
  `scripts/wasm-build.sh`（Windows 用 `.bat`），成员目录不重复声明 rustflags。
- 状态以 JSON 序列化跨端传输。

### 后端：Rust（[ADR-0003](decisions/0003-backend-rust.md)）
- Stage 1 不依赖后端；当前 Stage 2 服务直接依赖 engine crate。
- Web 框架已由 [ADR-0005](decisions/0005-unified-engine-three-deployments.md) 确定为 **Axum + tokio**，实现位于 `apps/server`。

### 前端状态：Redux Toolkit（[ADR-0004](decisions/0004-frontend-state-redux-toolkit.md)）
- engine（Rust/WASM）持有权威游戏状态；RTK store 存 UI 状态 + engine 结果快照，不重复计算业务规则。
- 意图派发 → 调 engine → 用结果更新 store → 渲染。

---

## ⏳ 仍待定

覆盖率门槛尚未形成 ADR。其他开放产品问题见
[`open-questions.md`](open-questions.md)。

---

## 已敲定的依据

- **React**：用户明确指定。
- **Tauri 2**：用户明确指定；本机已安装 `tauri-cli 2.11.3`。
- **engine = Rust→WASM / 后端 = Rust / 状态 = RTK**：msuad 拍板（2026-06-28），见对应 ADR。

## 待定项的决策流程

每个 ⏳ 都将通过 ADR 敲定（[`decisions/`](decisions/)）。**未敲定前不擅自引入。**
