# Haiku「Rayon / ComputeContext 残留终扫」二次核对

- 复核对象：`.tmp/haiku-rayon-residue-scan.md`（实习生产物）
- 复核方式：独立 `rg`（ripgrep 15.1.0）重跑 + 逐行抽样逐字比对 + 大小写/复合词/范围外补扫
- 复核日期：2026-10-17；工作区：`.worktree/std-thread-model`（只读分析）

## 一、计数复现（全量独立重跑，与报告口径一致）

| 指标 | 报告值 | 独立复现 | 结果 |
|---|---|---|---|
| 唯一命中行（union） | 284 | 284 | ✅ |
| `\brayon\b` | 57 | 57 | ✅ |
| `ComputeContext` | 215 | 215 | ✅ |
| `RAYON_NUM_THREADS` | 13 | 13 | ✅ |
| `wasm.bindgen.rayon`（点=通配符，未转义） | 8 | 8 | ✅ |
| `initThreadPool` | 0 | 0 | ✅ |

- 报告明细行数按节清点：生产 76 / 测试 135 / 注释文档 68 / 构建配置 5，合计 284，与各节标题声称值一致 ✅
- 多标签行实测 9 行；算术闭合：57+215+13+8+0 = 293 − 9（多标签重复计） = 284 ✅
- 佐证：转义点版 `wasm\.bindgen\.rayon` 命中 0，证明 8 处全部是 `-`/`_` 连接形式，与报告"点为通配符"的说明自洽。
- 根目录配置文件（Cargo.toml、Cargo.lock、package.json、rust-toolchain.toml 等）：关键词命中 0，报告范围声明无根目录缺口 ✅
- Cargo.lock 无 rayon 依赖条目：依赖层面 rayon 已彻底移除 ✅

## 二、抽样逐字核对（13 处，超出要求的 12 处）

| # | 位置 | 报告标签 | 核对结果 |
|---|---|---|---|
| 1 | apps/server/examples/server_max_speed_probe.rs:30 | RAYON_NUM_THREADS | ✅ 逐字一致 |
| 2 | apps/server/src/lib.rs:176 | RAYON_NUM_THREADS | ✅ 逐字一致（148/150 两处错误文案亦在） |
| 3 | apps/server/src/lib.rs:184 | ComputeContext | ✅ 逐字一致 |
| 4 | packages/engine/src/compute.rs:23 | ComputeContext | ✅ `pub struct ComputeContext {` |
| 5 | packages/engine/src/compute.rs:361 | ComputeContext | ✅ `impl ComputeContext {` |
| 6 | packages/engine/src/session.rs:1397 | ComputeContext | ✅ 逐字一致 |
| 7 | packages/engine/examples/production_entry_performance.rs:245 | rayon | ✅ 逐字一致（257 亦在） |
| 8 | packages/engine/src/verification_evidence/phase_timing.rs:550 | ComputeContext | ✅ 逐字一致 |
| 9 | apps/server/src/actor/fatal_tests.rs:5 | ComputeContext（测试） | ✅ 逐字一致 |
| 10 | packages/engine/src/session/pipeline/stock_stream.rs:953 | ComputeContext（测试） | ✅ 逐字一致；位于 865 `#[cfg(test)] mod tests` 之内 |
| 11 | packages/engine/src/session/pipeline/t3_boundary_tests.rs:24 | ComputeContext（测试） | ✅ 逐字一致 |
| 12 | packages/engine/src/session/deferred_drop.rs:3 | rayon（注释） | ✅ 逐字一致 |
| 13 | docs/decisions/0043-std-thread-ownership-runtime.md:41 | RAYON_NUM_THREADS（文档） | ✅ 逐字一致；另 packages/std-thread-proto/Cargo.toml:11 亦逐字一致 |

**13/13 全部通过，无行号偏移、无内容捏造。**

## 三、分类抽查（生产/测试边界）

- stock_stream.rs：268 行 `drive_stock_stream_with_feedback` 为生产 fn ✅；293 行在 291 `#[cfg(test)]` 门控的 `drive_stock_stream`（测试直通面）内 ✅；953 行在 865 `#[cfg(test)] mod tests` 内 ✅。该文件三类归档均正确。
- 库文件内测试项（compute.rs:272 等）与 `*_tests.rs` 归测试、examples 归生产的口径执行一致。

## 四、漏扫检查（大小写变体 + 复合词 + 范围外）

报告在其**声明的方法与范围内无漏扫**（union 独立复现 284 完全一致）。但作为"rayon 残留全景"存在以下系统性盲区（方法/范围设计所致，非执行错误）：

1. **大写 `Rayon`：82 行 / 90 处，净盲区 78 行**（不被任何给定关键词覆盖）。分布：docs/testing.md:111-113（仍把 Rayon thread 预算写为现行策略）、docs/superpowers/plans/2026-09-24-single-world-multithreading.md（30 行）、2026-09-25-production-entry-and-thread-pool.md（12 行）、docs/build-and-deployment.md、docs/naming-refactor-validation.md、docs/decisions/0018/0028，以及代码内注释 stock_stream.rs（7）、compute.rs（3）、server lib.rs/actor.rs 等。
2. **scripts/ 目录（声明范围外）：15 个文件 82 行**。实质影响最大：run-full-regression.mjs:60/69/364 仍主动导出 `RAYON_NUM_THREADS` 并按 `rayon_threads` 预算切分 CPU（退役后该 env 已无消费者）；run-full-regression.test.mjs 14 行断言该行为。
3. **下划线复合词被 `\brayon\b` 词边界排除：9 行**。`rayon_threads`（production_entry_performance.rs:229、escrow_performance_endpoint.rs:87/188）、`actual_rayon_threads`（escrow_verification_harness/runtime.rs:55/294/885、committed.rs:166）、测试名 `npc_completion_channel_does_not_deadlock_with_one_rayon_worker`（npc_decisions_tests.rs:44）。均为仍在使用的 rayon 命名标识符。
4. **裸 `RAYON` 1 行**：docs/superpowers/specs/2026-09-13-company-information-issues.md:1237（"RAYON=1/8 钉锚"）。
5. **命名残留非依赖残留**：`par_iter`/`into_par_iter`/`in_place_scope` 仍在 compute.rs 串行垫片（:1052 起）与若干调用点作为自有 API 存在；Cargo.lock 已无 rayon，属命名层残留，关键词扫描不覆盖（合理）。

## 五、可信度判定

- **口径内可信度：高。** 计数全复现、抽样 13/13 逐字命中、分类正确、无捏造。报告可作其声明口径（packages/apps/docs/.github + 根配置；`\brayon\b`/ComputeContext/RAYON_NUM_THREADS/wasm.bindgen.rayon/initThreadPool）的证据。
- **全景口径需补充：** 大写 Rayon 净 78 行 + scripts/ 82 行 + 复合词 9 行 + 裸 RAYON 1 行 ≈ 170 行未入账；全景残留约 454 行。若后续做"rayon 退役清理"，docs/testing.md 与 scripts/run-full-regression.* 是优先级最高的两处（一个写着现行策略、一个实际导出变量）。
