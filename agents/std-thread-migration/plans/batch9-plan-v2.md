# 批 9 v2 —— 按视图终裁重切的执行计划

依据：
- 旧案 `agents/std-thread-migration/plans/batch9-execution-plan.md`（16 步，锚 d755dfb1）
- 残留全景 `plans/haiku-rayon-residue-scan.md` + `plans/haiku-rayon-doublecheck.md`（口径内 284 行 + 盲区 ≈454 行）
- 终裁 `reviews/sol-view-model-verdict.txt`（2026-10-17）
- 接轨前置 `plans/step-frame-integration-prerequisites.md`（2026-10-18）

行号锚定 **HEAD=21b1e88b**（本 worktree，2026-10-18）；实现 agent 在主 worktree 持续改动，
执行时以符号锚点重定位。本计划只做分析，未跑任何 cargo。

## 0. 相对旧案的三个事实修正

1. **旧案阶段 A 已完成**：web `initThreadPool` 断链修复已落地（终扫 0 命中；
   `app-startup-wiring.test.ts:9` 已锚定"参数已随 rayon 退役移除"）；web-wasm 头注释已
   改为串行管线现实（`apps/web-wasm/src/lib.rs:3-5`）。旧 A1/A2 注销。
2. **默认翻转已完成**：`runtime/mod.rs` `step_runtime_enabled()` 缺省=runtime（106 失败清零
   后落地）；`ENGINE_STEP_RUNTIME=0` 为逃生门。旧案 B/C 的"翻转完成"依赖已满足，
   剩余依赖收敛为 **step_frame 接轨完成**（server `actor.rs:1165/1636` 的
   `step_frame()` 仍进旧 candidate 管线，实现 agent 正在接轨）。
3. **"ComputeContext 全家/内部退役批"被终裁拆解**：wasm32 恒走旧串行管线
   （`step_runtime_enabled` wasm 恒 false）+ 逃生门回退目标=同一旧管线 + 验证/测试注入面
   消费 ComputeContext 核心 ⇒ **compute.rs 核心与 engine 内部旧管线不能在接轨后物理删除**。
   旧案 C2/C3/C5 的"内部退役批"解散：镜像/安装链/双写 → B 桶（视图战役后删）；
   wasm 串行/逃生门/测试注入 → C 桶（保留）。A 桶承接的是 ComputeContext 的
   **crate 外注入暴露面 + 454 行清单里的 rayon 残留生产面**（含终扫盲区 scripts/ 82 行）。

分拣判据（Sol 终裁三条 + 接轨前提）：
- **A 桶**：接轨完成后在 native 生产主路径零消费者，且不属于 wasm 串行必需面 / 会话镜像
  桥接 / 逃生门 / 测试注入面。
- **B 桶**：会话镜像/安装链/双写——当前生产读面仍依赖，留到视图内生战役（owner 原生
  读面+统一发布版本）替代后删。
- **C 桶**：wasm 串行路径、逃生门、测试/验证注入面——保留。

依赖标注口径：〔接轨〕= step_frame 接轨完成（server/desktop 生产入口不再进旧管线，
前提已由 server/desktop 换轨成立）；〔终验收〕= Sol 终验收（任务 #102 收官）绿。

---

## 1. A 桶 —— 接轨后即可删（10 步）

> A1→A5 同一编译依赖簇（`HostComputeInitError`/`with_compute` 在 lib.rs/actor.rs/测试间
> 相互消费），建议同一人连续做、最多两个提交、验证统一跑。

### A1. server actor.rs 去 compute 注入 〔依赖：接轨〕

| 项 | 内容 |
|---|---|
| 文件:行 | `apps/server/src/actor.rs`：:967 `NewSessionError::Compute` 变体；:981 `compute: Result<Arc<engine::ComputeContext>, _>` 字段；:993 `configured` 默认调 `host_compute_context()`；:996 `with_compute`；:1001-1002 `compute_workers`；:1007/:1012/:1021 `configured` 内 Result 传递；:1030 `ProtocolSession::new_with_compute`；:1546-1548 `restore_with_compute` + `compute_context()` 访问器消费 |
| 改什么 | `SessionManager` 删 `compute` 字段、`with_compute`、`compute_workers`、`NewSessionError::Compute`；:1030 改 `ProtocolSession::new`；:1546 改 `ProtocolSession::restore`；`configured` 直接构造（去 Result 包装） |
| 验证 | `cargo check -p server`（A1→A2 连续做，中间编译红属预期） |
| 依赖 | **step_frame 接轨完成**：接轨前 server 生产入口仍进旧管线并消费注入的 ComputeContext，先砍=生产面行为变化 |

### A2. server lib.rs 删 RAYON_NUM_THREADS 解析层 〔依赖：A1〕

| 项 | 内容 |
|---|---|
| 文件:行 | `apps/server/src/lib.rs`：:147-152 `HostComputeInitError`；:156 `resolve_compute_workers`；:169-189 `host_compute_context`（含 :176 `RAYON_NUM_THREADS` 读取）；:192-212 `compute_workers_tests` |
| 改什么 | 全删。std-thread 模型线程数由引擎内部决定，CPU 限制走 OS 级 cgroup/taskset（memory/project_std_thread_migration） |
| 验证 | `cargo check -p server` 绿 |

### A3. server 测试改造 〔依赖：A1+A2〕

| 项 | 内容 |
|---|---|
| 文件:行 | `apps/server/src/actor/fatal_tests.rs`：:5 `use engine::{ComputeContext, ComputeWork}`、:8 `with_workers`、:66 `SessionManager::with_compute`+`compute_workers` 断言（:34 注释同步）；`apps/server/tests/actor.rs`：:125 `ComputeContext::with_workers`、:200 附近注释 |
| 改什么 | 测试改走 `ProtocolSession::new`/无 compute 构造；worker 数区分断言（fatal_tests 用 1/2 区分）改为对 runtime 线程模型的等价断言或按语义裁剪——**逐条注明裁剪依据，禁止静默删断言（铁律一）** |
| 验证 | `cargo test -p server`（遵守 10s-case 与多核约束） |

### A4. server example 清理 〔依赖：A2〕

| 项 | 内容 |
|---|---|
| 文件:行 | `apps/server/examples/server_max_speed_probe.rs:30` `env::set_var("RAYON_NUM_THREADS", …)` |
| 改什么 | 删 set_var；probe 输出注明线程约束改由 OS 级（cgroup/taskset）承载 |
| 验证 | `cargo check -p server --examples` |

### A5. civil 协议层删 with_compute 三入口 〔依赖：A1〕

| 项 | 内容 |
|---|---|
| 文件:行 | `packages/engine/src/session/protocol/civil/session.rs`：:105 `restore_with_compute`、:157 `new_with_compute`、:180 `restore_verification_checkpoint_with_compute`（:101/:153/:172-174 的 `shared_default` 委托收敛进默认构造） |
| 改什么 | 删三个 `_with_compute`；保留 `new`/`restore`/checkpoint 无参版（wasm 串行仍经默认构造内部 `shared_default`——该内部路径属 C 桶保留）。先 `grep -rn "with_compute" packages apps --include="*.rs" \| grep -v "session.rs\|_tests"` 确认 crate 外消费者仅 server 且 A1 已收口 |
| 验证 | `cargo check -p engine -p server && cargo test -p engine --lib`（engine 全量走分片长验） |

### A6. engine 旧管线性能探针处置 〔依赖：A5+接轨〕

| 项 | 内容 |
|---|---|
| 删除 | `packages/engine/examples/max_speed_probe.rs`（旧管线吞吐探针，:17 喂 compute；被 runtime 探针取代）；`packages/engine/examples/escrow_performance_endpoint.rs`（旧管线 escrow 性能端点，:87-91/:100/:188 rayon_threads 契约） |
| 去参保留 | `packages/engine/examples/production_entry_performance.rs`（宿主生产入口 step_frame 性能证据面，终裁明确宿主入口验证重要——保留但剥 ComputeContext/rayon-threads 参数：:99/:229/:245/:257，环境契约字段同步）；`packages/engine/examples/driver_semantics_probe.rs`（runtime 语义探针——剥 :30 compute 参数与 :9 注释） |
| 验证 | `cargo check -p engine --examples`；`grep -rn "ComputeContext" packages/engine/examples` 归零（escrow_verification_harness 除外，属 C 桶） |

### A7. scripts/ 回归脚本退役 RAYON_NUM_THREADS 与 rayon_threads 预算 〔依赖：A2〕

终扫盲区（doublecheck §四.2：scripts/ 15 文件 82 行）中实质生效面：

| 文件:行 | 改什么 |
|---|---|
| `scripts/run-full-regression.mjs` :60,:69,:319,:327,:364,:387,:400,:430,:518,:635 | 删 `RAYON_NUM_THREADS` 导出；`rayon_threads` 预算分量从 worker 预算算术（`test_threads + rayon_threads === cpu_budget`）中退役，额度并回 test_threads |
| `scripts/run-full-regression.test.mjs` :191-208,:712,:720 | 断言同步改（预算恒等式、env 断言）——逐条对应实现改动，不许静默放宽 |
| `scripts/frontend-build-plan.mjs` :8、`scripts/frontend-build.test.mjs` :18 | buildEnv 删 RAYON 分量（保留 CARGO_BUILD_JOBS） |
| `scripts/simulation/baseline-run.mjs` :129,:301,:1090 | `rayon_threads_per_seed` 预算与导出退役；子进程 CPU 约束改 OS 级口径 |
| `scripts/simulation/verify-simulation-artifacts.mjs` :156,:172 | manifest `resource_policy` schema 去 `rayon_threads_per_seed` 键（无版本概念，破坏性可接受，但须与 baseline-run 同提交） |
| `scripts/simulation/run-escrow-verification-matrix.mjs` :94 | `rayon_registry_capacity_samples` 对照——批 7 后垫片已无全局注册表，此断言已名不副实；随 C 桶 escrow 验证面裁定同步退役或改为 workers 记账对照 |

验证：`node --test scripts/run-full-regression.test.mjs scripts/frontend-build.test.mjs`（及仓库测试入口对应范围）。

### A8. CI 注入清理 〔依赖：A2；wasm 注释项并入 A10〕

| 文件:行 | 改什么 |
|---|---|
| `.github/workflows/distributions.yml:60` | GITHUB_ENV 追加删 `RAYON_NUM_THREADS`（保留 `CARGO_BUILD_JOBS`） |
| `.github/workflows/ci.yml:81` | wasm-bindgen-rayon build-std 注释——并入 A10 随链接参数裁定 |

### A9. docs/examples 注释现行策略勘误 〔依赖：A1-A8，收口步〕

| 文件:行 | 改什么 |
|---|---|
| `docs/testing.md:111-113` | 仍把 Rayon thread 预算写为现行策略——改为引擎内部线程模型 + OS 级限制口径（终裁后事实勘误，非历史记录） |
| `apps/desktop/src-tauri/examples/production_actor_performance.rs:3` | 删 `Run with RAYON_NUM_THREADS=<workers>` 用法注释，改 OS 级说明 |
| `docs/architecture.md:162` | 已标"历史 rayon 形态"——核对措辞，历史记录保留不动 |

验证：`grep -rn "RAYON_NUM_THREADS" apps scripts .github docs` 仅剩历史归档类命中（superpowers/plans、decisions 历史、work-status 历史记录）。

### A10.（条件步）W 批收尾：wasm 面构建链与隔离门 〔依赖：wasm32 构建验证绿〕

wasm 缺口已裁方向 A（world 不门控已落地，`runtime/mod.rs` D5 注释）；rayon 退出 Cargo.lock
后 `+atomics/build-std` 链接参数与 COOP/COEP 隔离门的唯一存在理由（wasm-bindgen-rayon
多核）消失：

| 文件:行 | 改什么 |
|---|---|
| `apps/web/vite.config.ts:8-9` 起 | COOP/COEP 头插件删除（串行 wasm 不需 SharedArrayBuffer） |
| `apps/web/src/host/startup-policy.ts:10,:21-22,:29-30,:33` | crossOriginIsolated/SharedArrayBuffer 启动门禁与报错文案删除（与 vite 头同步删，否则启动必挂） |
| 仓库根 `.cargo/config.toml` + `apps/web-wasm/.cargo/config.toml:2` | wasm32 rustflags/build-std（nightly `-Z` 链）整体删除；`.github/workflows/ci.yml:81` 注释同步 |
| `apps/web-wasm/src/lib.rs:5` | "多核方案待缺口核实结论"占位改终稿（串行管线为本期 wasm 语义，登记为简化） |

验证（必须先跑再删）：`CARGO_TARGET_DIR=.tmp/wasm-check-target cargo +nightly-2026-09-05 check -p engine --target wasm32-unknown-unknown` 与 `-p web-wasm` 绿（无 build-std 后用稳定工具链再核一次）；`pnpm -C apps/web test`。若删参数后 wasm 构建红（如 atomics 被别处依赖），此步整体挂起上报，不得半删。

---

## 2. B 桶 —— 视图内生战役后删（登记，本批不执行）

终裁 §3 判定"双写不再必要"后可整体消失的链（依赖：owner 原生读面 + 统一发布版本落地，
即视图内生战役；战役本体=中大型跨层重构，另立排期）：

| 步 | 文件:行（锚） | 删除内容 |
|---|---|---|
| B1 | `runtime/protocol.rs:303-450`（EchoBelief/EchoPlans/EchoAttention(/Sparse)/EchoAccounts(/Sparse) 变体与回执）+ `runtime/drive.rs` 请求/收集段（:532 起）+ `runtime/group.rs` 上行侧 | 每拍 Echo 采集与安装（仅为会话镜像服务） |
| B2 | `runtime/drive.rs:39,:419,:579,:617,:714` | `account_dirty_prev` 两拍并集与"下拍整值覆盖+本拍公开投影修正"机制 |
| B3 | `runtime/drive.rs:723,:1413`（`apply_same_tick_fill_projection` 族） | 同拍成交投影、公开计划成交补写、公开个人记忆裁剪 |
| B4 | `runtime/drive.rs:1983,:2067,:2082` | `install_market_views`/`install_candles` 对会话容器的重复安装 |
| B5 | `session/failure.rs:477` 起 `runtime_step_with_install` 安装段、:844 baseline 族、:962 全名册 attention heap 重建；desk 挂单重建 `EnvelopeLedger`/`DeskBookMirror` 段 | 镜像安装失败的账户/个人态 undo、行情/K 线 baseline、旧调度 heap 重建 |

衔接警告：`step-frame-integration-prerequisites.md` §1 要求接轨批先把同拍投影改逐轮消费
（Continue 早轮成交恰一次落账）——B3 的删除以该修复后的形态为基线，两批交接时须对照。

---

## 3. C 桶 —— 保留（0 执行，登记归属）

| 项 | 范围 | 保留依据（终裁） |
|---|---|---|
| C1. wasm 串行路径 | `runtime/mod.rs` `step_runtime_enabled` wasm 恒 false 分支 + `wasm_serial_smoke` + world 不门控（D5 方向 A）+ `compute.rs` wasm 分支（:350-356 显式串行测试、:464/:470 wasm 构造）+ 旧管线 wasm 可达面（`protocol/commit.rs:62` `step_frame` → 旧 candidate 管线） | "WASM 仍使用的串行路径必须保留"；Web 优先（Stage 1）产品语义 |
| C2. 逃生门 | `ENGINE_STEP_RUNTIME=0`（`runtime/mod.rs` 开关 + failure.rs/session.rs/pipeline 分支消费点） | 保留至终验收后与 C1 归属一并裁定——逃生门回退目标=旧管线=wasm 串行路径本体，单独删逃生门无意义（旧案 C4"终验收后即删"作废，改联动裁定） |
| C3. 测试/验证注入面 | `compute.rs` 核心（ComputeContext 结构、with_workers/from_workers/install/spawn/parallel_grain、T1Probes、observer/dispatch_gate/capacity_wait_probe〔cfg(test, verification-harness)〕）；`session.rs:1881 new_with_compute`/:1900 `rebind_compute_for_verification`/:3430 `restore_with_compute`；`session/failure.rs:913 step_pipeline_contract`（test-only 入口）；454 清单"测试代码 135 行"的大部（dedicated_compute/per-ComputeContext 隔离 fixtures）；`examples/escrow_verification_harness/`（ADR-0042 验证证据复跑面，feature=verification-harness）；`indicators.rs:243` compute 参数（旧管线 wasm 可达生产消费） | 测试注入面保留；旧管线测试因 wasm 串行语义继续有效 |

C 桶处置时点：C1/C2 待 wasm 多核最终方案或旧管线归属裁定（可能长期保留）；
C3 中纯 native-only 的注入面可随 B 桶战役后旧管线 native 消费者清零再评估，本批不动。

---

## 4. 工时档位（更新）

| 范围 | 档位 |
|---|---|
| A1-A5（server+civil 收口簇） | ≈1 天（同簇连续做，无切换成本） |
| A6-A9（examples/scripts/CI/docs 残留面） | ≈0.5-1 天 |
| A10（W 收尾条件步，含 wasm 构建验证） | ≈0.5 天（挂起则 0） |
| **A 桶合计** | **中档，约 2-2.5 个工作日**（旧案 1.5-2.5d 的范围不变，重心从 web 断链/翻转依赖移到 scripts+examples 残留面） |
| B 桶 | 0（登记）；视图内生战役本体=中大型跨层重构，另立战役 |
| C 桶 | 0（保留登记） |
| 独立复核 | 每步完成后按 CLAUDE.md 安排未实施 subagent 复核（重点：删的是否真死代码、断言裁剪依据、A10 不得半删） |

时序：step_frame 接轨完成 → A1-A9 → A 桶复核 → 并入任务 #102 终验收收官；
A10 可与终验收并行（wasm 面独立），结论红则单独上报。

## 5. 收口判据

```bash
# crate 外暴露面归零
grep -rn "RAYON_NUM_THREADS\|HostComputeInitError\|with_compute" \
  apps/server apps/web apps/desktop packages/engine/src/session/protocol \
  scripts .github --include="*.rs" --include="*.ts" --include="*.mjs" --include="*.yml"
# 期望：0 命中（docs/ 历史归档除外）
# engine 内部命中 = C 桶登记面（compute.rs/session.rs 注入构造/旧管线/测试）
cargo check -p engine -p server -p web-wasm && cargo check -p engine --examples
node --test scripts/run-full-regression.test.mjs scripts/frontend-build.test.mjs
```
