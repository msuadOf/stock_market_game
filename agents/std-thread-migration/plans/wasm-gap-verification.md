# WASM 编译面缺口核实（2026-10-16）

> **状态回填（2026-10-18，Sol 终审 §89）**：下述"25 错、编译不过"为
> **2026-10-16 核实时的历史现状**，其后已由 D5 wasm 方向 A 修复——
> world 门控移除（runtime/mod.rs:58 注记：wasm 宿主无 OS 线程，spawn
> 运行期 Err 由 session 侧显式处理；step_runtime_enabled 在 wasm 恒
> false 走串行管线）。nightly engine/web-wasm 检查日志与编译级绿证
> 归档于 `.tmp/sol-d5-red-evidence/`（运行时验证由 CI 的 wasm 构建+
> Web 端冒烟承载）。A12 的 ADR-0005/0027 补记候选已登记（wasm 串行
> 旧路径依赖保留）。本报告保留原貌作历史证据。

核实人：独立分析 subagent（只读源码，产物仅写 .tmp/）。工作区：`.worktree/std-thread-model`（分支 `exp/std-thread-model`）。

## 结论（一句话，2026-10-16 时点）

**engine 在 wasm32 下编译不过：25 个 E0432/E0433 错误，全部源于 `runtime::world` 被 `#[cfg(not(target_arch = "wasm32"))]` 门控，而 `runtime::{bridge,drive,group,protocol}` 与 `session/failure.rs` 仍无条件引用其中的类型；web 端确定受影响**（`cargo check -p web-wasm --target wasm32` 复现同一批错误），缺口为本分支引入，main 无此问题。

## 环境与命令

- `rustup target list --installed`：`wasm32-unknown-unknown` 已装（1.96.1 与 nightly-2026-09-05 均有）。
- **坑**：直接 `cargo check -p engine --target wasm32-unknown-unknown`（默认 1.96.1）会在编译前失败——`.cargo/config.toml` 的 wasm32 RUSTFLAGS 含 nightly-only 的 `-Z llvm-target-feature=...` 且 `[unstable] build-std`，报 "the option Z is only accepted on the nightly compiler"。这是误导性工具链错误，不是真实缺口。
- **正确命令**（与 CI 一致，CI 固定 nightly-2026-09-05 + rust-src + build-std）：
  ```
  CARGO_TARGET_DIR=.tmp/wasm-check-target cargo +nightly-2026-09-05 check -p engine --target wasm32-unknown-unknown
  ```

## 错误清单（25 个，全部 E0432/E0433 "cannot find `world`"）

| 文件 | 数量 | 性质 |
|---|---|---|
| `packages/engine/src/runtime/group.rs` | 14 | `super::world::RootProjection`（结构体字段/函数签名，类型级）、`runtime::world`（:540/:615/:2234/:3450） |
| `packages/engine/src/runtime/drive.rs` | 7 | `use super::world::World;`（:18，E0432 import）+ 函数体/签名引用（:154/:176/:623/:1032/:1058/:1081） |
| `packages/engine/src/runtime/protocol.rs` | 2 | `super::world::OpenDerivationRecord`（:499/:519，枚举变体负载，类型级） |
| `packages/engine/src/runtime/bridge.rs` | 1 | `use super::world::...`（:18，E0432） |
| `packages/engine/src/session/failure.rs` | 1 | `crate::runtime::world::BeliefParticipantSnapshot`（:71，函数参数，类型级） |

涉及的 world 符号：`World`、`RootProjection`、`OpenDerivationRecord`、`BeliefParticipantSnapshot`（均为类型）。

## 根因定位（与此前怀疑对照）

- 怀疑成立：`packages/engine/src/runtime/mod.rs:58-59` 将 `pub mod world` 门控在 `#[cfg(not(target_arch = "wasm32"))]`，但 `pub mod bridge`（:35）/`pub mod drive`（:39）/`pub mod group`（:53）/`mod protocol`（:33）**未门控**且直接引用 world 类型。
- `session.rs:1405` 的字段 `runtime_driver: Option<std::sync::Mutex<crate::runtime::drive::RuntimeDriver>>` 也未门控；`session/failure.rs:276-345` 的采纳路径会把 driver 置为 `Some`。session 层对 RuntimeDriver 的引用已扩散到 `notices.rs`、`pipeline/day_end.rs`、`pipeline/npc_tick_preparation.rs` 等文件。
- 引入来源：commit `e395d535`（"runtime 线程拓扑骨架——协调者/组/交易部协议层（批2）"），**不在 main 上**；main 完全没有 `runtime/mod.rs`、session.rs 无 `runtime_driver`（grep=0）→ main 的 wasm 构建不受影响，缺口是本迁移分支新引入。

## web 端影响判定：**确定受影响**

- `apps/web-wasm/Cargo.toml` 以 path 依赖 `engine`（无 feature 可关掉 runtime 面；唯一 feature `simulation-diagnostics`）。
- `cargo check -p web-wasm --target wasm32-unknown-unknown`（nightly-2026-09-05）失败于**同一批 engine lib 25 错**——web-wasm 的 lib.rs 使用 `engine::session::protocol::ProtocolSession`，session 模块树编译必然拉入 `runtime::drive` → world 缺失。
- CI 面：`.github/workflows/ci.yml`（wasm 多核构建，固定 nightly-2026-09-05 + wasm-pack 0.13.1）、`distributions.yml`、`scripts/build-targets.mjs`（web 目标跑 wasm-pack）在本分支上都会红。

## 运行时语义补充（影响方案选择）

- `world.rs:475/512` 用 `std::thread::Builder::spawn` 起组/交易部线程。wasm32-unknown-unknown（+atomics + build-std）下 **编译可过、运行时 spawn 返回 Err**（无宿主线程入口；wasm-bindgen-rayon 只服务 rayon 池，与本 std::thread 模型不通用）。
- 5 个 runtime 文件中未发现其他 wasm 不兼容 API（无 std::process/net/fs/os；grep 扫描确认）。
- session 默认 `runtime_driver: None`（session.rs:1793/1943），置 Some 只发生在 failure.rs 采纳路径——即 wasm 端只要"编得过"，串行旧管线语义仍然完整。

## 三个选项的初步成本估计

| 选项 | 改动 | 成本 | 风险 |
|---|---|---|---|
| **A. 串行后端**：去掉 `world` 的 wasm 门控（或提供 wasm 单线程 World），wasm 上 session 保持 `runtime_driver=None` 走既有串行管线 | 门控删 1 行 + 编译验证 + session 对 spawn 失败的显式 fallback | **约 0.5~1 天**（扫描显示无阻塞 API；A 的第一步就是删门控重跑本报告命令验证） | spawn 失败若静默吞掉违反铁律 2，必须显式错误/fallback；"WASM 暂为串行"需登记 |
| **B. 暂不支持报错**：web-wasm/CI 显式声明 wasm 目标暂不可用 | 构建配置 + 文档登记 | 小时级 | 项目"Web 优先"，Stage 1 主产品即 wasm 前端——只能作临时登记/过渡，不可作为合并状态 |
| **C. cfg 门控降级**：把 bridge/drive/group/protocol + `session.runtime_driver` + failure.rs 采纳路径全部 `cfg(not(wasm32))` | 25 处错误多为**类型级**（import/枚举负载/结构体字段/签名），cfg 级联到 session 多个已引用 RuntimeDriver 的文件 | **约 2~3 天 + 长期维护税**（今后每处 native-only 代码都要重复门控，易 cfg 漂移） | 双实现分叉面大，审查成本高 |

## 建议

**A 方向**。与 std-thread 迁移方针一致（代码本体无线程数概念、并发能力交给宿主环境）：wasm 宿主无 OS 线程，`World::spawn` 失败 → session 显式保持串行管线是自然语义，改动最小。B 仅可作登记用；C 成本与维护税最高。落地前需先在实现 agent 的树上验证：删门控后 wasm32 全量 `cargo check`（含 web-wasm）确实变绿。
