# S40：ADR 模板、决策记录制度与 Rust/WASM 引擎方向全文复核

## 阅读与基线

2026-10-03。连续全文读取 `docs/decisions/0000-template.md` 37 行、`0001-record-architecture-decisions.md` 41 行、`0002-engine-rust-wasm.md` 48 行，共 126 行。已遵守根 AGENTS/principles；本轮只新增审计工作文件，未改产品、未执行 Git 写操作、未运行测试或构建。产品源码对应 `b76ece3`，worktree 的 merge HEAD `4ad5a2e` 只另有审计文档。

另外追踪 ADR-0005:19–45、architecture:27–35 与 tech-stack:29–37 对旧部署措辞的细化，现行 ProtocolSession、三个宿主入口、WASM exports、Worker caller、serde normalizer、构建工具和代表性测试正文。

## 全文条款状态

| 来源与章节 | 当前映射/证据 | 判定 |
|---|---|---|
| 模板:1–12，文件命名/状态/日期/决策者 | `docs/decisions/0001`、`0002`及后续 ADR 文件 | 文档作者模板，不是运行时 schema，不要求给游戏添加 ADR parser/validator。 |
| 模板:14–32，上下文/决策/备选/后果 | 每份 ADR 的写作结构 | 过程约定；占位符及“后续需要做的”不是一项具体产品承诺。 |
| 模板:34–37，关联 | 已指向 open-questions | 导航，不是游戏 API。 |
| ADR0001:7–13，上下文 | 持续保存决策、引用原决定与替代关系 | 架构历史背景，不新增自动审计功能。 |
| ADR0001:17–20，编号和结构 | `docs/decisions/` 持续递增至0026；已读0001/0002采用固定章节 | 过程实现存在，不把日期编号或措辞差异当产品缺口。 |
| ADR0001:21，accepted只追加/另建推翻决策 | ADR0002:3 指向0005细化；ADR0023–26独立登记后来决策；0016:91之后追加计划契约 | 旧ADR现行文本有修订说明，历史制度是否严格逐字只追加属于文档治理；本轮未审整个Git历史，不宣称已证明全仓遵守。 |
| ADR0001:22/35–36，开放问题/检查ADR | `AGENTS.md:53–60`、`docs/open-questions.md` 的已解决/ADR关联 | 协作过程已有入口，不要求新增游戏提示或软件门禁。 |
| ADR0001:24–33/38–41，备选/后果/关联 | 解释为何不用注释/wiki或不记录 | 不包含额外运行时代办。 |
| ADR0002:7–15，多端纯逻辑/可序列化/同实现 | `packages/engine/src/lib.rs:19–111` 导出规则、账户、撮合、session/会计等同一crate；三个宿主依赖该crate | 方向已实现；多端相同源码不等于完整跨端验收。 |
| ADR0002:19–21，Rust crate/cargo/无I/O | `Cargo.toml:3–9` workspace；`packages/engine/Cargo.toml:10–11,50–55`；engine内部状态/动作与typed errors | 已实现。对核心src搜索fs/net/SystemTime/web_sys/js_sys/tokio/reqwest未找到相关调用，搜索仅辅助依赖/owner追踪，不冒充纯度行为证明。 |
| ADR0002:22，WASM绑定 | `apps/web-wasm/Cargo.toml:12–26`；`lib.rs:331/344/465/510` exports；`wasm-worker.ts:141/193/230`实际call | 产品 caller 已接，不是仅库级导出。 |
| ADR0002:23，Server/Tauri直接复用 | Server Cargo:26 → `actor.rs:939` ProtocolSession::new；Desktop Cargo:31 → `actor.rs:624`，restore在1145 | 实际actor拥有同一ProtocolSession；不把三个Rust原语函数当真实三宿主测试。 |
| ADR0002:24，JSON跨端 | WASM `restore_json`（lib:510）→ serde_json→ProtocolSession::restore；Worker slot:58 JSON.stringify；SaveSlot严格serde与边界验证 | 已实现。内部WASM出站JsValue/Map不表示存档没有JSON协议；下游统一normalize。 |
| ADR0002:26–38，备选/优缺点/TDD复杂度 | Rust/TS分层测试入口与现行技术栈 | 不要求另建TS引擎；性能“充裕”只是当时预期，不可冒充测量通过。 |
| ADR0002:40，输入状态+动作到状态/事件、清晰类型边界 | `lib.rs:64–116` SessionRegistry拥有ProtocolSession；enqueue:465传player intent、step:344返回协议update；Worker包装→EngineHost→UI | 语义已接。ADR0005:45、architecture:28–35明确有会话owner及命令/update适配；不要求每次经JS复制整GameState或把所有宿主改成无状态RPC。 |
| ADR0002:41，引擎单测/少量WASM集成 | `apps/web-wasm/src/protocol_tests.rs:23/49/109`；`apps/web/src/host/serde-normalize.test.ts`、worker/restore tests；真实browser `e2e/company-information.spec.ts:135` | 分层测试代码存在。本轮未执行；native registry测试不证明wasm32 ABI，真实WASM E2E另有实际查询错误路径断言。 |
| ADR0002:42，工具链/技术栈登记 | `scripts/frontend-build-plan.mjs:16–24` wasm-pack→threading/release检查→copy/stage→tsc/vite；`.cargo/config.toml:1–16`；tech-stack:29–37 | 工具实际接构建，版本/线程符号登记；没有新缺口。 |
| ADR0002:44–48，Q1/ADR3/架构关联 | open-questions的Q1已解决，architecture/tech-stack与代码依赖一致 | 引用方向已兑现，不把Stage2/3未来语态原样视为今日未实现。 |

## 错误与序列化 caller 链

- 初始化：`createWorkerHost`（worker-host.ts:125）创建专属 Worker，初始化收到ready后:183发create → worker.ts:142 → WasmSessionSlot:49 → Rust `create_session:331`，先serde解析再构造ProtocolSession。初始化/Worker错误经 worker.ts:83–103/328–337、worker-host.ts:151–175显式返回失败并销毁该Worker；没有用空catch或默认新局掩盖输入错误。
- 玩家动作：worker.ts:193–197 → Rust enqueue:465–470 → ProtocolSession::enqueue_player_intent；运行 step_update:89–116 → EngineUpdate，typed致命错误经 HostFailure（lib.rs:124）保留code/where/context/recovery actions。错误对象无法序列化时lib.rs:301–308返回包含原始失败与序列化失败的显式字符串，不是静默fallback。
- 存档：Worker parseSaveSlot → WasmSessionSlot.restore:53–67 → JSON.stringify → Rust restore_json:510–515 → ProtocolSession严格恢复；替换句柄/释放旧句柄由 restoreWasmSession控制。公开持久档为日终候选，lib.rs:481–497不再兑现旧日内save要求，符合ADR0025。
- 公共报告：worker.ts:261 → lib.rs:406 → 引擎公共query → normalizePublicReportPage（serde-normalize.ts:19） → 当前generation请求协调与UI。`serde-normalize.ts:31–55` 对Map递归转换，对bigint超安全范围显式报错；公司i128金额本来用十进制字符串，不以Number改写。没有把JsValue内部表示差异登记成“缺JSON存档”。

## 候选反证与已登记边界

1. **“engine没有直接cdylib，因此WASM未实现”核销。** 核心rlib供三宿主复用，`apps/web-wasm`才是cdylib/wasm-bindgen owner，实际Worker有call；这符合ADR0002:21–23及后续拓扑细化。engine Cargo:9“feature启用cdylib”的注释已过时，仅是文档漂移。
2. **“WASM导出持有thread_local registry，违反核心无全局状态”核销。** registry在适配crate（lib.rs:43），核心状态由ProtocolSession独占；宿主句柄管理不是core公司/账户共享状态。原生jemallocator进程配置（engine lib:15–17）不拥有游戏状态，不能据global_allocator字面判定业务全局状态。
3. **“WASM本身没用JSON对象，所以不支持跨端存档”核销。** 存档持久化及实际restore走JSON.stringify/serde_json；Map是桥内表示，normalize与严格parse已经接入caller。
4. **“裸字符串错误都被静默吞错”核销。** serde或合法operation错误以字符串出站，但Worker统一包装operationError/failure，主线程显式reject/故障通知；structural致命错误另保留HostFailure。不能把不同错误种类都强制升级为不可恢复fatal。
5. **重复create保留旧句柄的注释**（WasmSessionSlot:48）是已知内部边界；生产createWorkerHost:131每次新建Worker、:183只在ready流程创建，dispose:269–275销毁Worker。未定位正常用户新局在同Worker重复create的caller，不能仅凭注释确认用户新局资源泄漏，也不是这三篇ADR的独立具体需求。
6. 原始错误脱敏不自动等于缺“为什么”：protocol_tests:136起校验真实来源链且不泄露私有数值；未知类型标明详情未公开。是否某个错误分类仍漏上下文需具体caller证据，本轮未确认额外实例。
7. G01–G05宿主连接/拉帧/恢复边界、G37真实订单诊断关联、完整三宿主与发布验收债保持总账分类；同一engine依赖和native协议测试不能核销。技术指标展示Q08是更细领域/展示契约，不能从ADR0002的泛化架构句强行扩大为所有UI算术都要搬Rust。

本轮未确认这三篇新增产品漏实现；全部126行已经映射为文档模板/过程制度、已实现方向、后续细化契约或既有验收边界。未运行测试，因此结论不代表本基线构建/浏览器/三宿主验收通过。
