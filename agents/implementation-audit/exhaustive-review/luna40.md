# ADR-0000–0002 全文 EOF 复核（2026-10-03）

## 基线与方法

- 检查目标：实现 `08e4fc7`（与合并基线相同）；工作树 `.worktree/implementation-reaudit`。
- 已完整读取 `AGENTS.md`、`docs/principles.md` 及三份指定文档，EOF 行数：`0000-template.md` 37 行、`0001-record-architecture-decisions.md` 41 行、`0002-engine-rust-wasm.md` 48 行。按文档各段落逐章追 Rust crate、WASM 导出、JSON/JS 值边界、错误映射、依赖和测试入口；没有用片段搜索代替文档阅读。
- 对照总账 `implementation-audit-2026-10-02.md` 第 307–309 行确认三文档均已覆盖。既有总账只证明当时读过，不能代替当前调用链证据。本轮静态检查，不运行测试或编译。
- ADR 模板是治理工具；其结构要求不是缺失的产品功能，也不把模板字段逐项转成产品缺口。

## 逐章映射

| 文档章节 / 原文主张 | 当前实现与调用链证据 | 复核结论 |
|---|---|---|
| `0000-template.md` §模板说明、Status/Date/Deciders、Context/Decision/Alternatives/Consequences/Related（第 1–37 行） | 模板规定记录格式，不参与产品运行。ADR-0002 有状态、日期、决策者及各章节；技术栈在 `Cargo.toml`、`packages/engine/Cargo.toml`、`apps/web-wasm/Cargo.toml` 落地。 | 不构成产品功能要求。模板同时允许状态 `superseded by`，与 ADR-0001 的“accepted 只追加不修改”配套时，状态迁移应新增后继记录；属于文档治理约束。 |
| `0001` Context / Decision（第 1–19 行）：以 ADR 保存架构选择理由；递增编号；上下文→决策→备选→后果；accepted 只追加不修改，推翻时新建并链出 | ADR-0002 至 ADR-0027 延续相同格式。ADR-0002 当前状态行明确补充“核心决策保留；部署拓扑关系被 ADR-0005 细化”；`0005-unified-engine-three-deployments.md` 承接部署架构。 | 基本落实。需区分“被细化”与“被推翻”：0002 保留核心选择，并未被替代。状态行在 accepted ADR 上增加括注是否违反只追加规则，不能由工作树快照断定编辑历史；可作为治理一致性候选，不能报产品缺陷。 |
| `0001` Alternatives / Consequences / Related（第 21–41 行）：代码注释/wiki/不记录的替代方案；维护成本及后续事项 | ADR 集合持续记录选择理由；ADR-0002 的关联项链接 open question Q1、ADR-0003、架构和技术栈文档。核心实现与部署证据见下两行。 | 无实现偏离。ADR 维护成本是已陈述代价，不应转化成需新增产品能力。 |
| `0002` Context（第 1–18 行）：Stage 1 浏览器、可选后端、Tauri 复用同一纯逻辑引擎；状态可序列化 | workspace 成员包括 `packages/engine`、`apps/web-wasm`、`apps/server`、`apps/desktop/src-tauri`（根 `Cargo.toml:1–6`）。Web-WASM 通过 `engine` crate 调用 `ProtocolSession`；服务端和 Tauri 同样依赖 engine（各自 Cargo.toml）。`GameSession` 含 trait object，WASM 注册表持有 `ProtocolSession` 而不是序列化会话本体（`apps/web-wasm/src/lib.rs:4–7,43–59`）。 | 单一 Rust 实现与多宿主复用成立；不可序列化运行态由存档/协议 DTO 边界承接，符合“可序列化状态”作为跨界契约的意图，不意味着所有内部对象须 serde。 |
| `0002` Decision（第 20–29 行）：Rust crate；前端经 wasm-bindgen/wasm-pack；后端/Tauri 复用 crate；JSON 跨端状态 | Engine 当前为 `rlib`（`packages/engine/Cargo.toml:10–12`）；桥接 crate `web-wasm` 为 `cdylib` + `rlib`，依赖精确版本 `wasm-bindgen = 0.2.93`、`serde-wasm-bindgen`（`apps/web-wasm/Cargo.toml:10–26`）。脚本 `scripts/frontend-build-plan.mjs` 运行 wasm-pack；CI / distribution workflow 安装并检查固定版本。三宿主 API 各自走 engine/协议实现。 | Rust / WASM 路线落地。需精确描述：浏览器的主要桥接值是 `JsValue`，经 `serde-wasm-bindgen` 编解码为 JS 对象/Map，而不是在每个调用上先生成文本 JSON；文本 JSON 明确用于 `restore_json`，SaveSlot 等数据另有 serde JSON 契约。ADR 的 JSON 跨端表述适用于持久化/语言无关状态格式，但不应被误读为 WASM ABI 必须 JSON 字符串。 |
| `0002` Alternatives（第 31–36 行）：纯 TS 与 TS+Rust 双引擎被否决 | 全仓宿主结构仅 engine Rust 承担核心状态机；Web 展示/输入仍有 TS 逻辑，且 UI 调用 Worker/WASM；TS 的图表呈现或表单校验并非第二份撮合核心。 | 否决项与当前分层相符；未发现 TS 复制权威撮合或组合核心的证据。 |
| `0002` Consequences / follow-up（第 38–48 行）：桥接纯接口、JSON 边界、Rust 单测及少量 WASM 集测、工具链文档 | `web-wasm/src/lib.rs:331–515` 导出 `create_session`、`step`、快照、报告查询、指标、enqueue、save/restore 等 API；输入转换使用 `serde_wasm_bindgen::from_value`，输出走 `to_js`/`public_dto_to_js`，错误返回 `Result<_, JsValue>`。`HostFailure` 将 fatal 错误映射为带 code/message/where/context/recoveryActions 的 DTO（`:120–160,298–321`）；协议/错误测试在 `apps/web-wasm/src/protocol_tests.rs` 及 `lib.rs` 内。Engine 有 Cargo 测试、`export_bindings` 类型生成入口（根 `package.json`）。 | 分层与错误显式返回的方向成立；生产的 WASM 导出不等于少量集成测试“已充分覆盖”，本轮只确认调用者/测试文件存在，不报告测试通过。桥接接口会传送 engine DTO/事件，并非所有 API 都只是单一“输入状态+动作→新状态”接口；ADR 描述的是核心边界，不宜据此要求删掉查询/诊断等宿主方法。 |

## 旧结论复核与新候选

- 旧总账对 0000–0002 的处理是按主题全文阅读登记，没有提出具体结论；该覆盖记录成立，但不能推导出 WASM 运行时/JSON 边界已做调用链验证。以上补足实现层证据，没有发现已有“缺陷已修复/仍缺”结论需要撤销。
- 当前边界从源代码看是显式的：`create_session` / `enqueue` / `restore` 等输入解码失败直接以 `JsValue` 返回；找不到句柄返回带句柄号的错误；fatal step 错误序列化成结构化 `HostFailure`，序列化本身失败时保留原始失败说明（`lib.rs:298–321`）。未见此 ADR 路径静默吞掉错误。
- **待审候选 C40-1（低优先级、静态推断）：** WASM `SessionRegistry::register` 用 `AtomicU32::fetch_add` 分配句柄且直接 `HashMap::insert`（`apps/web-wasm/src/lib.rs:49–60`）；达到 `u32` 回绕且旧句柄仍存活时可能复用 ID 并覆盖会话。现有线程局部注册表与 `drop_session`（`:43–47,475–478`）正常释放；需极端长期/高频创建才触发，未复现，也未查找专门回绕测试。可作为防御性边界测试/溢出策略候选，不是 ADR-0002 决策违反，不应仅凭理论风险阻断功能。
- A 股语义：本次仅审查架构/宿主边界，没有改动交易规则或引入新的领域语义；不据此主张交易规则已复核。

## 范围

只新增本复核记录；未修改产品代码、正式 ADR、Git 状态或测试文件，未运行长测。
