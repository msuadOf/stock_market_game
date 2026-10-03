# 批次046：hosts来源复核

## 覆盖与方法

基线为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。按分配次序连续全文读取三篇来源至 EOF：`hosts-01.md` 67行，SHA-256 `3c547aacdad8ac3f52a1e3ad4b31b1e23c69fb7ed1136c8e4f2f966b6eae8fc7`；`hosts-02.md` 63行，SHA-256 `baa7fcdf5932a48c7627a31a62cf2646f95bc3c6c60a8fd1fb6d5ab0ef123805`；`hosts-03.md` 75行，SHA-256 `c60d43279a7085f21d6aa297fca2be9ce375eda7b58b31c4b76cd25f5797d104`。未变更产品代码、未运行测试/构建/回归。

已读仓库 `AGENTS.md`、`docs/principles.md`；参照当前 `docs/decisions/`（编号至 ADR-0028）、`docs/open-questions.md`、`agents/implementation-audit/implementation-audit-2026-10-02.md` 的 G/Q 主账，以及 `agents/oop-refactor-audit/completeness-2026-10-03/hosts/report.md` 与动作清单。此处只核对相关主账：G01–G05、G18–G20、G40、Q20；不能把对象抽取或封装完成写成行为缺口已修复。来源中关于 T+1、竞价、撮合和结算的判断是边界说明，本复核没有新增交易规则依据。

## 逐篇核对

### hosts-01：actor宿主

旧候选 `hosts-01-A01/A02` 已在当前代码中实现：桌面 `SessionHandles.cmd_tx` 私有（`apps/desktop/src-tauri/src/actor.rs:333-336`），服务端 `cmd_tx`、`event_tx` 私有并提供只返回 receiver 的 `subscribe_events()`（`apps/server/src/actor.rs:606-625`）。服务端 WS 初始和 Resync 都经句柄订阅（`apps/server/src/routes.rs:1269-1275`、`:1414-1420`）。actor仍分别独占各自 `ProtocolSession`，桌面 Tauri emit 与服务端 broadcast 资源没有被合并。对候选“缩小写口”的结论再证；若来源被当作待实施建议则已过时。

修订后的 `hosts-01` 已指出固定倍率16ms聚合字段可能是陈旧线索。现行桌面 `tick_and_emit` 调 `run_cycle(1)`（`apps/desktop/src-tauri/src/actor.rs:832-838`），而 `pending_fixed_events` / `last_fixed_publish` 字段仍保留（`:680-687`）；快速循环也在每个 cycle 调 `publish_protocol_cycle`（`:840-887`）。因此相关聚合行为不能从旧对象描述推定为已实现。此是 G19 的独立当前缺口（主账仍列固定倍率逐tick emit），不是新 OOP 候选，也不是本批修复。

`hosts-01` 对单写者、宿主差异、失败 rollback、generation/timeline 及 A 股语义边界的保留判断仍适用。速度规则不能因对象整理而统一：桌面无回执且非法内部值记录后忽略、服务端有数值上限与 oneshot 的差异仍由各宿主代码维持。未发现 ADR 明确撤销这些约束。

### hosts-02：服务端路由与部署

此篇主张的 `routes.rs` 私有模块拆分被明确标为可选 `organization_notes`，不是 OOP 动作；本轮未核实或宣称它已实施。它对“不新增 Routes/ApiService 大对象”、保留 Axum handler、`SessionHandles` 单写者、`ClientFrameBuffer` 连接局部所有权、先订阅再 baseline 的边界仍有参考价值。实际订阅顺序在当前 `routes.rs:1269-1275` 和 resync `:1414-1420` 可见。

旧注释“客户端消息仅作存活/pong”与实现处理 `Resync`、`GetFrame`、`SubmitIntent` 的不一致仍在 `apps/server/src/routes.rs:1256-1261`。这是真实注释缺陷，但模块拆分不能算修复，也不应靠把 WS 状态包装成类掩盖；当前 caller路径在 `run_ws`。部署边界仍由 `deployment.rs`/`lib.rs`组合；ADR-0027未发现取代依据。此篇无撮合或 A 股规则实现建议。

### hosts-03：适配层与集成测试

`hosts-03-A01` 的私有 WASM `SessionRegistry` 已实现：`thread_local REGISTRY` 存 `RefCell<SessionRegistry>`（`apps/web-wasm/src/lib.rs:43-45`），struct持有 handle→`ProtocolSession`（`:51-55`），`create`/`restore`/受检访问/删除/`step_update` 方法在 `:57-115`；公开 WASM导出仍通过 registry转交，例如创建/步进 `:331-350`、删除/恢复 `:473-515`。故“多个导出函数直接散操作 thread-local HashMap，建议加 registry”的旧说法已被现行实现反证。当前实现仍保持 worker-local adapter与引擎 authority分层。

但 `SessionRegistry::register` 仍用 `NEXT.fetch_add(1, Ordering::SeqCst)` 后直接 `HashMap::insert`（`apps/web-wasm/src/lib.rs:57-62`），没有耗尽检测。主账 Q20（WASM `u32` 句柄回绕可覆盖存活会话）仍未消除；引入 SessionRegistry 本身不修这个缺陷。恢复路径经同一注册函数，因此同样受影响。按旧候选自己声明的范围，句柄耗尽修复是独立行为修改，不得冒充 OOP 等价抽取。

hosts-03所审的 server WS测试与 WASM protocol测试属于支持性契约；不制造测试类。`hosts-03` 明确指出 `step_frame` 后 `tick_batch` 失败不因此原子，复核中不把适配层包装说成新 rollback 保证。无变化的调用契约仍须遵循 ADR-0010 与 ADR-0025。

## 结论

旧候选动作 hosts-01-A01、hosts-01-A02、hosts-03-A01 在基线已有生产实现，不应重复报为未实施；这是对当前状态的核验，不是本批新增实现。hosts-02模块整理仍只是可选组织说明。独立行为项 G19、Q20及 WS注释不一致保留为未解决线索；本批不改 G/Q 状态。没有发现新的 ADR 明确取代来源约束。未运行产品测试、构建或回归，不能据此声称行为已通过验收。
