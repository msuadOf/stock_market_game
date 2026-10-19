# WASM panic（WASM_WORKER_PROTOCOL unreachable）排查台账

- 批次：S1/S2 公司系统集成 UI 实测遗留问题，N4 排查（worktree `wf_0e6af0a3-0bb-1`，基线 main@5bdf0953 一族）
- 日期：2026-10-07/08
- 结论速览：**报障的「step 即触发 panic」在本机复现为「首个 step（休市日日终 settlement）在 ≥64 wasm 线程时冻结 85–125 秒」**；伴随一个确定性主线程 bug（实测倍速轮询 Illegal invocation，已修复）。字面 `unreachable` 陷阱在隔离复现中未再现，证据与假设见下。

## 1. 现象（原始报告）

- S1/S2 集成批 UI 实测：默认新局在浏览器内 step 即触发 WASM panic，失败码 `WASM_WORKER_PROTOCOL`，message `unreachable`。
- main 检出（10-06 构建 wasm）与本树生产 preview 同流程同样崩溃 → 非集成引入。
- 局内场景（公司面板、「从文件读取」导入）被阻断。

## 2. 复现环境与步骤

- 本 worktree（当前引擎代码）；`vite dev`（端口 5199，COOP/COEP 由项目插件注入）+ Playwright headless Chromium。
- wasm 均为本树现构建，覆盖三种变体：`--dev`（debug+unwind）、`--release`（unwind）、`--release -Z build-std=panic_abort,std`（与生产 frontend-build 一致）。
- 驱动脚本 `.tmp/company-system/wasm-panic/repro.mjs`：打开页面 → 选「本地」→「启动游戏」→ 默认新局自动 1x 运行；镜像 Worker 双向 postMessage、轮询 `data-game-tick`、`.app-error`、wasm 内存（ingress 消息中的 `memory.buffer.byteLength`）。
- 线程数注入：拦截 `init` 消息补 `threads` 字段（`REPRO_THREADS`），避免改动生产代码。

## 3. 复现结果矩阵

| wasm 线程 | 首 step（休市日 2030-01-01 日终） | 之后 |
|---|---|---|
| 1 | 立即完成 | 1 tick/s 正常 |
| 8 | 立即完成 | 正常；公司面板可开 |
| 32 | 立即完成 | 正常 |
| 64 | **冻结 85–92s**（2/3 次；1 次在外部负载≈114 时瞬时完成） | 恢复后 1 tick/s 正常 |
| 128（=本机 hardwareConcurrency，默认值） | **冻结 85–125s**（5/5 次） | 恢复后 1 tick/s 正常 |

- 冻结期间：Worker 无任何消息（含公司面板 publicReports 请求无响应 → 与「局内场景被阻断」吻合）；wasm 内存以 ≈0.7MB/s 缓慢爬升（说明在推进而非死锁）；结束时 +265MB 序列化突发，终态 561–691MB（距 4GB 上限远，非 OOM）。
- 三种 wasm 构建变体表现一致；**均未 post 出字面 `unreachable` 失败**（最长观察 ~6 分钟，含冻结后的正常 1x 流与追帧）。
- 每次运行都出现确定性主线程错误：`TypeError: Illegal invocation at poll (useSpeedMetricsPolling.ts:18:35)`（详见 §5.1）。

## 4. 根因分析

### 4.1 主线程 Illegal invocation（已修复）

- Vite 转换后 18:35 即 `finally { timer = ports.schedule(() => void poll(), 1e3) }`。
- `useSpeedMetricsPolling` 生产接线传 `schedule: setTimeout`，以 `ports.schedule(...)` 方法形式调用，receiver 为普通对象；Chrome 严格模式模块下实测抛 `TypeError: Illegal invocation`（浏览器实测：`{schedule:setTimeout}.schedule()` 抛、裸调用不抛）。
- 后果：首次采样 1 秒后的重试链在 finally 中断，实测倍速轮询停止（不影响引擎推进，但污染控制台并使 UI「实测倍速」停摆）。
- 引入时间：8cf34a1c（2026-10-03，早于 10-06 基线 → 与「非集成引入」一致）。

### 4.2 首 step 冻结：wasm 多线程日终 settlement 的分配/争用塌缩（已登记，未修）

CDP `Profiler`（10ms 采样）对主 Worker 与 24 个 rayon helper Worker 采样：

- 主 Worker 101.3s 中 **82.8% 在 `std Condvar::wait`**（阻塞等 pool 完成 scope）；另有 dlmalloc::malloc 3.2%。
- 26 个 helper 聚合 2901 CPU-秒：
  - `String::clone` **29.7%**、`__rdl_dealloc` **22.9%**、`__rdl_alloc/realloc` ≈7%、`RawVec finish_grow` 1.5% → **分配器占比 ≈60%**；
  - rayon/crossbeam 协调（`Stealer::steal`、`Injector::steal`、`wake_any_threads`、wasm_sync Condvar）≈20%；
  - 真正业务（`deliver_public_information::{closure#0}` 3.2%、`observed_at_of` 3.5%、`BeliefBook::*` ≈1%）≤10%。
- 热点业务路径：`GameSession::deliver_public_information`（`session/notices.rs`）对 2 万 belief participants `into_par_iter`，每个 NPC 克隆 `information()`/`belief()`（String 密集）再丢弃；wasm dlmalloc 全局锁 + crossbeam 窃取队列在 64–128 个 SAB 原子线程下争用塌缩。
- 线程数非单调致病：64 与 128 均塌缩；外部高负载（≈114）时 64 反而瞬时完成（OS 换出争用线程）→ 空闲开发机（报障者环境）必然触发。
- 原生对照（排除引擎逻辑缺陷）：同一 setup/seed，`RAYON_NUM_THREADS=128` 原生 40 步 6.8s 干净；6 个随机 seed × 40 步干净；release 整日推进至 ~5000 步（集合竞价+连续竞价早期）无 panic（5 分钟 deadline 截断）。引擎默认规模逻辑路径未发现原生 panic。

### 4.3 字面 `unreachable` 为何未在隔离复现中出现（假设，未证实）

- 生产链路 `panic_abort` 下，Rust panic → abort → wasm trap `RuntimeError: unreachable` → `wasm-tick-loop.stepOnce` 捕获 → `postFailure` → 正是报障的 `WASM_WORKER_PROTOCOL` + `unreachable`（无位置无消息，与 panic hook 未接 console 一致）。
- 本机（共享、高负载）三种构建 5–6 分钟内未触发该 panic。合理假设：同一高线程数病症在报障机器的空闲时序下表现为竞态或分配失败（panic 落在参与 scope 的主 Worker 线程 → 报 failure；落在 helper 线程 → scope 永久卡死，静默挂起）。需空闲机器复测确认（见 §6-d）。

## 5. 处置

### 5.1 已修复并提交（本 worktree）

1. `fix(web)`：`useSpeedMetricsPolling` 生产接线改箭头包装 `setTimeout/clearTimeout`，消除 Chrome `Illegal invocation`；`speed-metrics-polling.test.ts` 在既有行为测试上新增结构回归（红→绿验证：裸 `schedule: setTimeout` 时红）。
2. `test(engine)`：`packages/engine/tests/web_default_setup.rs` —— 逐字段镜像 `apps/web/src/config/defaults.ts DEFAULT_SETUP`（此前唯一未被任何原生测试覆盖的默认规模路径）+ `wasm-worker step_update` 同语义冒烟（30 步 ≈6.6s，覆盖休市日结+次日集合竞价开局）；另附 `#[ignore]` 整日长验收（命令见文件头注释）。

### 5.2 验证

- 浏览器（threads=8）：无 pageerror；1x 正常推进；公司面板公开报告列表可见（截图 `.tmp/company-system/wasm-panic/repro-final.png`）。
- `cargo test -p web-wasm --lib`：18 passed。
- `cargo test -p engine --test web_default_setup`：1 passed / 1 ignored（长验收另册）。
- web 套件 `scripts/run-web-tests.mjs`：122/123 pass；唯一失败 `save/file-target-permissions.test.ts` 为环境性（读取 `apps/desktop/src-tauri/gen/schemas/acl-manifests.json`，本 worktree 未做 desktop codegen；主检出存在该产物）——与本次改动无关。

## 6. 建议处置（deferred，不在本批落地）

- a. **wasm 线程池上限/整定**：`resolveThreadCount` 现原样透传 `navigator.hardwareConcurrency`；引擎性能主线（memory：16 核调优包络 ~220x→1000x）之外的大机器（≥64）恰是塌缩区。建议在性能主线内确定阈值（需空闲机器二分 32–128）与显式策略（上限值、是否对用户可见），不在本批静默加帽。
- b. **引擎侧分配削减**：`deliver_public_information` 的 per-NPC `information/belief` 整体克隆（String 密集）是塌缩燃料；改共享/Arc 化或分块（`with_min_len`）可同时惠及原生与 wasm。属性能主线（`.worktree/engine-max-performance` 台账 current-state.md）范围。
- c. **panic 可观测性**：生产 wasm panic 当前只剩裸 `unreachable`（无消息/位置），排查成本极高。建议评估在 wasm 构建接入 panic hook（如 console_error_panic_hook，注意体积/许可）或至少在 HostFailure message 中区分「引擎结构化错误」与「wasm trap」。
- d. **空闲机器复测字面 trap**：单用户会话、threads=hardwareConcurrency、默认新局，观察是否 post `WASM_WORKER_PROTOCOL unreachable`；若再现，用 `--dev`（unwind）构建取 panic 消息定位。

## 7. 证据索引（均在 `.tmp/company-system/wasm-panic/`，gitignored）

- `repro.mjs`：浏览器复现/验证驱动（含线程注入、消息镜像、内存采样、公司面板检查）。
- `profile.mjs`：自启 Chromium + raw CDP 对主/rayon helper Worker 的 CPU 采样剖析。
- `console.log` / `profile-console.log`：完整消息时间线（含 128 线程冻结窗口）。
- `main-profile-*.json` / `helper-profile-*.json`：CDP Profile 原始数据（§4.2 数字来源）。
- `repro-final.png`：修复后验证截图（threads=8，公司面板打开）。
- `vite.log`、`web-tests.log`：服务端与套件日志。
