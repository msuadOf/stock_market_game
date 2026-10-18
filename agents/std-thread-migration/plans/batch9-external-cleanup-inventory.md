# 批 9 外部清理盘点（2026-10-15，只读盘点 agent 产出，翻转后执行）

## 1. 旧管线外部调用点

**server**：
- `apps/server/src/lib.rs:148-189`：RAYON_NUM_THREADS 解析 + host_compute_context()（→ ComputeContext::with_workers/shared_default），OnceLock 缓存
- `apps/server/src/actor.rs:981,996,1007,1030,1546`：SessionManager 持 Arc<ComputeContext>，经 ProtocolSession::new_with_compute/restore_with_compute 注入
- `apps/server/src/actor/fatal_tests.rs:5,8,34,66`、`tests/actor.rs:125,200`：测试直接构造 ComputeContext::with_workers
- `apps/server/examples/server_max_speed_probe.rs:30`：set_var("RAYON_NUM_THREADS")

**web / web-wasm**（wasm-bindgen-rayon 遗留，Cargo.lock 已无 rayon）：
- `apps/web/src/wasm-pkg.d.ts:35` 声明、`host/wasm-worker.ts:157` 调用 initThreadPool——web-wasm/src/lib.rs 已无此导出，**属断链**
- `host/worker-host.ts:139`（threads 参数）、`wasm-worker-ownership.test.ts:47` 同步遗留
- `apps/web/vite.config.ts:8`、`host/startup-policy.ts:22,30`：SharedArrayBuffer/COOP-COEP 门禁
- `apps/web-wasm/src/lib.rs:3-4`、`.cargo/config.toml:2`：仅注释提及 rayon

engine-gpu、desktop：无 ComputeContext/rayon 引用（desktop 走 ProtocolSession::new）。
**无任何包引用 step_frame_and_commit / step_on_context / ENGINE_STEP_RUNTIME。**

## 2. engine 暴露面删除候选（翻转后）

- `compute.rs`：ComputeContext(23)/ComputeInitError(216)/ComputeWork(234)/ComputeObservation(255)
  及 with_workers(436)/from_workers(458)/available(463)/shared_default(476)/
  active_width(665,676)/workers(870)/install(876)/spawn(993)、parallel_grain(1042)、
  CpuBackend/create_backend(1127,1185)
- `session.rs:1800,3257` + `protocol/civil/session.rs:105,157`：new_with_compute/restore_with_compute
- `failure.rs:281` step_pipeline_contract（test-only，注释已登记退役）
- 翻转后 `runtime/mod.rs:81` step_runtime_enabled 开关本体

## 3. wasm 编译面疑似缺口（**翻转前必须核实**）

- `runtime/mod.rs:58-59`：`pub mod world` 仅 cfg(not(wasm32))；但 `drive.rs:24`
  RuntimeDriver **未门控**且 `use world::World`，`session.rs:1388` runtime_driver
  字段未门控——wasm32 编译可能已断，需 `cargo check --target wasm32-unknown-unknown` 核实
- 旧管线 wasm 处理参照：compute.rs:350-356 wasm 显式串行（with_workers 报
  ThreadsUnavailable）；npc_decisions.rs:402 wasm 串行循环；deferred_drop.rs:7
  wasm 无 std::thread
- **若确认断**：std::thread runtime 无法在纯 wasm32 跑——需要决策：
  (a) RuntimeDriver 出 wasm 串行后端（工作量中~大）；(b) wasm 面显式"暂不支持"
  （违背 Web 优先 Stage 1）；(c) 其他。**涉及产品目标，需上报用户拍板**

## 4. 工作量档位

小~中（约 12-15 文件）：server 收口 ~6、web 断链清理 ~5、web-wasm 2、
engine 标注 ~4；无 step API 外部迁移。
