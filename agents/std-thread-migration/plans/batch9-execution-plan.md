# 批 9（翻转后 crate 外部清理）执行计划

基于 `batch9-external-cleanup-inventory.md`（盘点）产出。行号锚定 **HEAD=d755dfb1**（2026-10-16），
实现 agent 在持续改动，**执行时以符号锚点为准重新定位**，行号仅作参考。

## 0. 相对盘点的核实修正（2026-10-16 复核）

1. **Cargo.lock 已确认无 rayon**：server 的 `RAYON_NUM_THREADS` 环境变量现在喂的是 engine
   `ComputeContext` 的 worker 数（旧管线 scatter-gather 的 std::thread 池），变量名已名不副实。
   收口时连变量名一起删，不是保留改名。
2. **路径漂移**：盘点写 `failure.rs:281` → 实际 `packages/engine/src/session/failure.rs:498`
   （`step_pipeline_contract`）；盘点写 `protocol/civil/session.rs` → 实际
   `packages/engine/src/session/protocol/civil/session.rs`，且除 105/157 外还有
   `restore_verification_checkpoint_with_compute`（172-174）共 **3 个** with_compute 入口。
3. **行号漂移**：`worker-host.ts` threads 参数 139 → 328；`session.rs` 1800/3257 → 1803/1817/3270/3274。
4. **范围放大警告**：`ComputeContext` 在 engine **内部**有 50+ 文件引用
   （session 管线全部、views/self_views/account_book/settlement/indicators/verification_evidence/
   runtime/decision.rs 及大量测试）。compute.rs 家族删除不是"删公开 API"一步，是内部退役批的
   终点，本批只登记不执行。
5. `shared_ingress.rs:264,472,516` 的 `step_pipeline_contract` 调用点在 `#[cfg(test)]` mod 内
   （mod 起于 222 行），test-only 定性不变。

## 1. 依赖图总览

```
[A 可先行] A1 web 断链修复 ──┐
          A2 web-wasm 注释 ─┤（无依赖，随时可做）
                             │
[翻转完成]* ──> B1 server actor 去注入 ──> B2 server lib.rs 删解析
                  │                            ──> B3 server 测试改造 ──> B4 example 清理
                  └──> C1 civil/session.rs 删 with_compute×3
[翻转完成 + 终验收绿] ──> C4 runtime/mod.rs 删 step_runtime_enabled
[内部退役批（批 9 前置，另行排期）] ──> C2 session.rs 删 with_compute
                                 ──> C3 step_pipeline_contract 退役
                                 ──> C5 compute.rs 删 ComputeContext 家族
[wasm 缺口核实结论（另一任务）] ──> W1..W5
```

\* "翻转完成" = `step_runtime_enabled()`（`packages/engine/src/runtime/mod.rs:81`）缺省改 1
（即全局域迁移 + 106 个 session 侧测试修复 + 终验收通过）。当前缺省 0=旧管线。

## 2. 阶段 A —— 可先行（与 engine 翻转无关，随时可做）

### A1. web 断链修复（initThreadPool 已无导出，wasm worker 启动必挂，属修 bug）

| 项 | 内容 |
|---|---|
| 文件:行 | `apps/web/src/wasm-pkg.d.ts:35`（删 `initThreadPool` 声明） |
| | `apps/web/src/host/wasm-worker.ts:134,157-158,167`（删 `resolveThreadCount` + `initThreadPool` 调用；`ready` 消息不再带 `threads`；`initialize(message.threads)` 去参） |
| | `apps/web/src/host/worker-host.ts:328`（`init` 消息去掉 `threads` 字段；顺带 grep `threadCount` 来源，若仅此用途则删传递链） |
| | `apps/web/src/host/wasm-worker-ownership.test.ts:47,90,92`（mock 与断言同步去 `threads`） |
| 改什么 | init 消息协议两端（host→worker）整体去掉 threads；先 `grep -rn "threads" apps/web/src` 确认无 UI/诊断面板等其他消费者再动手 |
| 验证 | `pnpm -C apps/web lint && pnpm -C apps/web test`；类型检查 `pnpm -C apps/web exec tsc -b --dry`（或 build 的 tsc 段）。完整 `pnpm -C apps/web build` 含 wasm 产物检查，**受 wasm 缺口影响**，结论前不作为本步门禁 |
| 依赖 | 无（web-wasm 导出面已无 initThreadPool，断链是既成事实） |

### A2. web-wasm 头注释过时修正（纯注释，零行为）

| 项 | 内容 |
|---|---|
| 文件:行 | `apps/web-wasm/src/lib.rs:3-4` |
| 改什么 | 删/改"wasm-bindgen-rayon 初始化线程池后 rayon par_iter 自动用满核心"描述（Cargo.lock 已无 rayon，描述失实）；暂写"多核方案待 wasm 缺口结论（见 .tmp wasm 核实任务）"占位，结论后终稿 |
| 验证 | `cargo check -p web-wasm`（注释改动不应影响；若 wasm32 目标当前编译已断，native check 即可） |
| 依赖 | 无。若 wasm 缺口结论先到，可并入 W 批一起改 |

## 3. 阶段 B —— server 收口（依赖：默认翻转完成）

四步属同一编译依赖簇（`HostComputeInitError` 在 lib.rs 定义、actor.rs/测试消费），
建议 B1→B2→B3→B4 顺序一次做完、最多两个提交，验证统一跑。

### B1. `apps/server/src/actor.rs` 去 compute 注入

| 项 | 内容 |
|---|---|
| 文件:行（符号锚点优先） | :981 `compute: Result<Arc<engine::ComputeContext>, _>` 字段；:993 `configured` 默认调 `host_compute_context()`；:996 `with_compute`；:1001-1007 `compute_workers`；:1030 `ProtocolSession::new_with_compute`；:1546 `ProtocolSession::restore_with_compute` |
| 改什么 | `SessionManager` 删 `compute` 字段与 `with_compute`/`compute_workers`；:1030/:1546 改 `ProtocolSession::new`/`ProtocolSession::restore`；`configured` 去掉 Result 包装直接构造 |
| 验证 | `cargo check -p server`（此步后 B2 前编译会红，属预期，B1+B2 连续做） |
| 依赖 | **默认翻转完成**（翻转前生产路径仍消费 ComputeContext worker 配置，先砍=生产面行为变化无必要） |

### B2. `apps/server/src/lib.rs` 删 RAYON_NUM_THREADS 解析层

| 项 | 内容 |
|---|---|
| 文件:行 | :148-152 `HostComputeInitError`；:156 `resolve_compute_workers`；:169-189 `host_compute_context`（OnceLock 缓存）；:192-211 `compute_workers_tests` 内联测试 |
| 改什么 | 全删（环境变量名 RAYON_NUM_THREADS 一并退役；std-thread 模型线程数由引擎内部决定，CPU 限制走 OS 级 cgroup/taskset——见 memory/project_std_thread_migration） |
| 验证 | `cargo check -p server` 绿 |
| 依赖 | B1 |

### B3. server 测试改造

| 项 | 内容 |
|---|---|
| 文件:行 | `apps/server/src/actor/fatal_tests.rs:5,8,15,66,67`（`use engine::{ComputeContext, ComputeWork}` / `with_workers` / `new_with_compute` / `SessionManager::with_compute`+`compute_workers` 断言）；`apps/server/tests/actor.rs:125,200 附近`（`ComputeContext::with_workers`） |
| 改什么 | 测试改走 `ProtocolSession::new`/`SessionManager` 无 compute 构造；`ComputeWork` 相关断言（fatal_tests:8 用 workers=1/2 区分）改为对 runtime 线程模型的等价断言或按语义裁剪——**逐条注明裁剪依据**，不许静默删断言（铁律一） |
| 验证 | `cargo test -p server`（按项目多核/10s-case 约束跑） |
| 依赖 | B1+B2 |

### B4. `apps/server/examples/server_max_speed_probe.rs:30` 清理

| 项 | 内容 |
|---|---|
| 文件:行 | :30 `std::env::set_var("RAYON_NUM_THREADS", ...)` |
| 改什么 | 删；probe 若读该值做参照记录，改为读 OS 级配置说明（cgroup/taskset）并在输出里注明 |
| 验证 | `cargo check -p server --examples` |
| 依赖 | B2（变量解析层已删后此 set_var 成无效副作用） |

## 4. 阶段 C —— engine 暴露面删除（分级依赖）

### C1. civil 协议层删 with_compute 三入口（依赖 B1，翻转完成后即可）

| 项 | 内容 |
|---|---|
| 文件:行 | `packages/engine/src/session/protocol/civil/session.rs:105 restore_with_compute`、:157 `new_with_compute`、:172-174 `restore_verification_checkpoint_with_compute`（及其 :101/:153 的 shared_default 委托） |
| 改什么 | 删三个 `_with_compute`；保留 `new`/`restore`/checkpoint 无参版。先 `grep -rn "with_compute" packages apps --include="*.rs" | grep -v session.rs` 确认唯一 crate 外消费者（server）已在 B1 收口 |
| 验证 | `cargo check -p engine -p server && cargo test -p engine --lib`（engine 全量测试按长验/分片约束另行跑） |
| 依赖 | B1 + 默认翻转完成 |

### C2. GameSession 删 new_with_compute/restore_with_compute（**前置在批 9 之外**）

| 项 | 内容 |
|---|---|
| 文件:行 | `packages/engine/src/session.rs:1817 new_with_compute`、:3274 `restore_with_compute`（:1803/:3270 委托改直调内部路径） |
| 改什么 | 删两个 `_with_compute`；`compute_context()` 访问器（:1824 附近）随内部退役处理 |
| 阻塞前置 | engine **内部 50+ 文件**（session/pipeline 全线 + 大量 `*_tests.rs` 如 plan_chain_candidates_tests/plan_root_quota_wave_tests/ready_ingress_tests 等）仍调 `new_with_compute`——这是内部退役批（旧管线下线批）的工作量主体，**不属于批 9**。此处仅登记终点 |
| 验证 | 前置批完成后 `cargo check -p engine` |

### C3. `step_pipeline_contract` 退役（test-only，依赖内部测试迁移）

| 项 | 内容 |
|---|---|
| 文件:行 | `packages/engine/src/session/failure.rs:498 step_pipeline_contract`；调用点 `session/shared_ingress.rs:264,472,516`（cfg(test) 内）+ `session/pipeline/` 下 quote_expiry/executor_perturbation/commit_evidence 等 tests |
| 改什么 | 所有调用测试迁到 runtime 路径（`step()`）或随旧管线语义一起退役；然后删函数。注释已登记退役计划，此处执行 |
| 验证 | 前置批完成后 `cargo test -p engine`（分片） |
| 依赖 | C2 同批前置（内部退役批） |

### C4. 删翻转开关本体（依赖：缺省已翻 + 一轮终验收绿）

| 项 | 内容 |
|---|---|
| 文件:行 | `packages/engine/src/runtime/mod.rs:76-85 step_runtime_enabled` + 全部 `ENGINE_STEP_RUNTIME` 消费点（session.rs / session/failure.rs / pipeline/npc_tick_preparation.rs / runtime/drive_tests.rs，grep 确认） |
| 改什么 | 缺省改 1、终验收绿、再跑一轮生产探针后，删开关与分支，step() 走唯一 runtime 路径；CI/脚本若设过 `ENGINE_STEP_RUNTIME` 一并清（grep scripts/ 与 .github/） |
| 验证 | `cargo test -p engine`（分片）+ server 探针 smoke；全局 `grep -rn ENGINE_STEP_RUNTIME` 归零 |
| 依赖 | 默认翻转完成 + 终验收（=任务 #102 的收官动作之一，时序上可与批 9 收口同批） |

### C5. compute.rs 删 ComputeContext 家族（**依赖 C2/C3 + wasm 缺口结论**，登记不执行）

| 项 | 内容 |
|---|---|
| 文件:行 | `packages/engine/src/compute.rs`：`ComputeContext`:23、`ComputeInitError`:216、`ComputeWork`:234、`ComputeObservation`:255、`with_workers`:436、`from_workers`:458、`available`:463、`shared_default`:476、`active_width`:665(,676)、`workers`:870、`install`:876、`spawn`:993、`parallel_grain`:1042、`CpuBackend`:1127、`create_backend`:1185 及关联测试（:336 起 parallel_grain_tests 等） |
| 改什么 | 整文件随旧管线退役删除（或缩到 wasm 串行分支处置后删——compute.rs:350-356 有 wasm 显式串行路径，与 W 结论耦合） |
| 阻塞前置 | engine 内部 50+ 引用文件全部迁移（内部退役批）+ W 结论（wasm 面是否还需要串行 compute 入口）。**批 9 不执行** |
| 验证 | 前置齐后 `cargo check -p engine -p server && cargo check -p web-wasm` |

## 5. 阶段 W —— 留待 wasm 缺口核实结论（另一任务在跑，本批全部挂起）

| 步 | 文件:行 | 内容 | 决策依赖 |
|---|---|---|---|
| W1 | `apps/web/vite.config.ts:8-9` 起 COOP/COEP 头配置 | std-thread wasm 方案若仍需 SharedArrayBuffer 则保留，否则删 | 缺口结论 (a)/(b)/(c) |
| W2 | `apps/web/src/host/startup-policy.ts:10,21-22,29-30,33` | crossOriginIsolated/SharedArrayBuffer 启动门禁与报错文案；同上随方案走；若保留需把"多线程"措辞从 rayon 语义改为新方案语义 | 同上 |
| W3 | `apps/web-wasm/.cargo/config.toml`（注释）+ 仓库根 `.cargo/config.toml`（wasm-bindgen-rayon 1.2.1 专用 rustflags/build-std 链接参数） | 方案变则链接参数整体删除；方案 (a) 沿用则保留并改注释 | 同上 |
| W4 | `apps/web-wasm/src/lib.rs:3-4` 注释终稿 | 若 A2 已做占位，按结论改终稿；否则整步并入结论批 | 同上 |
| W5 | `packages/engine/src/runtime/drive.rs:24 RuntimeDriver`（未 wasm 门控）vs `runtime/mod.rs:58-59`（`pub mod world` 仅 cfg(not(wasm32))） | 疑似 wasm32 编译已断；核实命令 `cargo check -p web-wasm --target wasm32-unknown-unknown`；结论 (a) RuntimeDriver 出 wasm 串行后端（工作量中~大）/ (b) wasm 面显式不支持（违背 Stage 1，需用户拍板）/ (c) 其他 | **涉及产品目标，上报用户拍板**（盘点原文保留） |

## 6. 工时档位

- **批 9 可执行本体（A1+A2+B1-B4+C1）**：**中档偏小，约 1.5~2.5 个工作日**
  （A≈0.5d；B≈0.5-1d；C1≈0.5d 内，B/C1 间有编译耦合但同一人连续做无切换成本）。
- **C4**：≈0.5d，但时序绑定任务 #102 终验收收官。
- **C2/C3/C5**：工作量属"内部退役批"（50+ 文件迁移），**不计入批 9**；此处仅登记终点与前置链。
- **W1-W5**：待 wasm 缺口结论，其中 W5 方案 (a) 为中~大，(b) 小但需用户拍板。

## 7. 验证命令速查

```bash
# web（阶段 A）
pnpm -C apps/web lint && pnpm -C apps/web test
# server（阶段 B；测试遵守项目多核与 10s-case 约束）
cargo check -p server && cargo check -p server --examples && cargo test -p server
# engine（阶段 C；全量测试走长验/分片，不单命令裸跑）
cargo check -p engine -p server -p web-wasm
# wasm 缺口核实（W 批前置，由另一任务执行）
cargo check -p web-wasm --target wasm32-unknown-unknown
# 收口完成判据
grep -rn "ComputeContext\|RAYON_NUM_THREADS\|with_compute\|step_pipeline_contract\|ENGINE_STEP_RUNTIME" \
  apps/server apps/web packages/engine/src --include="*.rs" --include="*.ts" -l
# 期望：server/web 零命中；engine 命中仅剩内部退役批未完成的部分
```

## 8. 执行注意

- 实现中每步完成后按 CLAUDE.md 安排未实施 subagent 独立复核 diff（大 A 语义与本批无交易制度
  变化，复核重点是"删的是不是真死代码+断链修复无行为回退"）。
- 行号以 HEAD=d755dfb1 为锚，执行前每步先按符号重定位。
- B3 测试断言裁剪必须逐条注明依据，禁止静默弱化（铁律一）。
