# Luna44：Open Questions、Principles、Roadmap 全文复核

## 范围与方法

- 基线：指定 worktree 当前源码，产品基线为 `08e4fc7`（与 merge 提交相同）；本复核只读取 `docs/open-questions.md`（157行）、`docs/principles.md`（92行）、`docs/roadmap.md`（18行），并读取 `AGENTS.md`。三文均从首行连续读至 EOF；另以最新总账 `agents/implementation-audit/implementation-audit-2026-10-02.md`、`candidate-checks.md`、`coverage/s05.md` 回查旧结论，并根据总账所列当前生产调用链核对来源。没有运行测试、构建、浏览器、联网或改产品/Git。
- 结果分类：已定决策不等于完整生产验收；生产实现不等于跨宿主/发布验收；开放问题、未来产品范围、实现缺口、证据债分别记录。凡复用总账的源码判定，均指出其现行生产入口和消费边界，不将旧计划勾选或历史测试结果当作当前运行证明。

## Open Questions 全项矩阵

| 原文/行 | 状态与最新决定 | 当前调用/生产边界及复核旧总账 | 反证、候选与归类 |
|---|---|---|---|
| Q1「Rust 编译为 WASM」（L17–19） | 已决定，未见冲突。 | `packages/engine` 为核心，Web 的 `wasm-pkg.d.ts` 暴露 `save/restore`，Web Worker/host 调用引擎；服务端与 Tauri 分别依赖同一个 Rust engine。对应总账 S02/R19“架构主干已有”。 | 三宿主实际链路仍有 G01–G05/G18–G20；不可从语言落地或 WASM 工件推出 Stage 2/3 整体验收完成。没有新的语言选择候选。 |
| Q2「后端语言…Rust」（L23–25） | 已决定。 | `apps/server/src/actor.rs` 每 session actor 持有 engine `GameSession`；HTTP/WS 层通过 actor 适配。总账 S02/R19结论仍成立。 | Stage 2公网身份、TLS、数据库及运维另属 roadmap 明列的未完成条件；不是后端语言问题。 |
| Q3「维持 MIT」（L29–34） | 已决定，文档同时留有“暂用 MIT/备选”历史措辞。 | `LICENSE` 与 ADR-0007 §7 为规范来源；对运行时没有 caller。总账 S02未列许可证实现缺口。 | 建议后续把“当前暂用/备选”改成历史背景或删除，以免与“已解决”并列造成仍未决定的印象；不属于产品/交易实现缺口。 |
| Q4「pnpm workspace…11.19.0」（L38–44） | 已决定。 | workspace 根 `package.json` 的 `packageManager` 与 `pnpm-workspace.yaml` 消费此选择；Web/server/desktop packages 由 pnpm workspace 管理。总账 S02确认现状。 | 当前旧选项仍列 npm/pnpm利弊，但已明确解决，属于可读性问题；未发现方案漂移候选。 |
| Q5「Redux Toolkit」（L48–50） | 已决定。 | `apps/web/src/render-app.tsx` 的 Redux `Provider`，`apps/web/src` 的 store/slices承载UI状态；引擎状态仍通过 host 投影，不把 RTK 误作引擎状态真源。总账 S02确认。 | 不能据 Redux 存在推论 UI 整体或移动交互验收完成；G22–G34是各自独立缺口/视觉债，不是状态库选择未落地。 |
| Q6「全中文…未来第二语言仍开放」（L56–60） | 首发决策已定，第二语言是未来产品决定。 | Web文案及设计是现行中文UI；无 i18n 框架。总账 S02、R17把中文UI与页面/UX部分缺口分开。 | 不应把“未来开放”混成待决定的当前首发阻塞项，也不应称已有国际化。没有现行实现缺口候选。 |
| Q7「一个快速槽 + JSON导入导出…Stage 2持久化仍待产品决定」（L62–67） | Stage 1契约已落地；多槽、完整历史、数据库、云同步属 Stage 2未来决策。 | Web save 层 `apps/web/src/save/day-end-persistence.ts` 管公共快速槽/日终保存；Rust `SaveSlot`负责深校验、原子恢复；桌面通过 Tauri actor save/restore。总账 R01/S05确认日终最小存档及恢复边界。 | “本地/文件存档”在 Roadmap 与 Q7一致；持久化数据库并非 roadmap 当前已承诺，而是公网部署前条件。公共日内存档不做，内部候选回滚不等于公共日内存档。 |
| Q8「SplitMix64…NPC下单决策…seed不含并发到达轨迹」（L69–78） | 已解决的确定性契约有明确边界。 | `GameSession`/Session RNG与存档包含可重放状态；NPC决定调用引擎策略随机流；总账 R12/R19/R02和 Q10→G39区分固定输入、已受理事实重放与自由worker调度。 | 新局宿主仍用固定 `DEFAULT_SEED=42n`，总账 G20：这不是 Q8决策撤回，而是 Q9/G20所列“普通新局熵取种”局部未接；不能宣称任意并发同 seed 整局字节一致。 |
| Q9「tick…全订单簿…T+1…统一账户、撮合驱动价格」（L80–84） | 核心玩法/对外交易语义已定。 | `GameSession`经 pipeline推进、订单簿撮合、结算；Web/Server/Tauri host驱动会话。总账 R07–R10/R15/R19认为主干生产路径存在。 | `docs/trading-rules.md` 的简化、A股类别差异及日历覆盖仍按具体缺口核对；总账 G15有官方覆盖非假日回退边界，不能把“核心竞价规则”写成完整复刻交易所全制度。没有新交易规则实现候选。 |
| Q10「视觉设计、Blueprint/AG Grid/Lightweight Charts、响应式」（L86–90） | 视觉方向/技术选择已定。 | `apps/web/src/App.tsx`、`MobileStockDetail.tsx`、chart runtime呈现真实页面；总账 R17确认页面存在。 | 页面存在不覆盖 G10–G14/G22–G25/G30–G34；也不代表移动、键盘、reduce-motion、实际尺寸像素矩阵已验收。未来资讯/更多周期等在总账 §6另列，不是已定设计项自动授权。 |
| Q11「NPC AI…独立策略、个人参数」（L94–120） | 旧 ADR-0006策略摘要有历史共同隐藏V描述；L106明确由 ADR-0016及公司计划取代；L117–120 ADR-0026已补机构真实经历/暂停买入、不强制卖出方向。 | 当前策略 caller在 `packages/engine/src/session/pipeline/npc_decisions.rs`；工厂 `strategy/factory.rs`；个人信念/计划链在 `session/decision_chain/`。总账 R04/R14、G06–G09/G38核出逐条生产断点。 | 必须以 L106–120及 ADR-0016/0026为准，不能复活共同隐藏 V、要求人人估值、或把历史“可插拔”直接视为外部动态扩展能力。总账 §3的“Q11 Correction/CreditDefault分发”与此文档Q11不是同一个问题，构成编号碰撞；建议候选重命名为 R? 或 `Q11-Correction` 并在总账标注来源，防止误读为 NPC 策略方向仍待定。 |
| Q12「费用可出金、现金池可缩小、不补钱/不造对手盘」（L122–138） | ADR-0024最新决定已解决；公司经营现金和投资者现金严格分隔，股东分配不在本发布实现。 | 交易 `FillTransition`生成费用，`settlement.rs`汇总receipt再进入账户；公司经营/结账另在公司账本/Session调用。总账 S01/R03/S31核对并确认费用与公司账务是不同边界。 | 零成交/缺对手盘是诚实结果，不是异常修复理由；未来申赎/收入、股东分配属新用户决定+ADR。CAS 8/其他官方依据缺失是法源/游戏假设证据债，不可借此反称减值未实现（总账 §6 S03-C1）。 |
| 文首 Escrow 门禁「仍须密封旧语料与新路径真实比较」（L9–11） | 与较新清理决策冲突，不能按此文字直接执行/重新打开旧 corpus。 | 总账 `candidate-checks.md` 的 S26已对照 `docs/test-cleanup-checklist.md` §12：旧 sealed corpus adapter/replay example/bundle装配及旧测试工具已批准退役；当前 K7验证保留守恒/价时/依赖等并按同受理事实定义检查，见 G39、R02。 | 这是本三文档发现的明确文档漂移/旧结论未真正同步：Open Questions仍把旧 corpus比较写为“仍须”，而总账第5节又写历史对比工具“已退役”，§6说不可复验非缺口。应修改正式文档，使其保留现存比较与证据限制，不再要求退役适配栈；不要伪造缺失历史 witness。属于唯一明确新候选，需正式文档负责人收口。 |

## Principles 全项矩阵

| 原则/原文 | 当前生产调用与总账结论复核 | 现存边界/候选 |
|---|---|---|
| 1 TDD，「公共 API、核心业务逻辑必须有测试」（L8–17） | 核心分散在 `packages/engine/tests/`，Web/host/Server/desktop各有对应测试源码；总账按 G 表将纯函数已有测试与生产消费分开，未以旧执行结果宣称当前通过。 | 文本是过程原则，不可从测试文件存在推出新路径覆盖；G01–G39及 §4代表性生产边界仍是当前修复验证入口。对我本次只读文档审计不运行测试符合不伪称通过。未发现原则本身与最新决定冲突。 |
| 2 防御校验、显式错误；存档支持编辑/无配额替代（L19–33） | Web导入及Server bounded serde → `SaveSlot` → engine深校验；REST/actor/host错误映射见 `apps/server/src/routes.rs`、actor及Web error UI。ADR-0019移除请求数/挂单配额仍保留资金、股份、交易规则校验；总账G01/G21/G22分别保留认证、工具投影与字段错误缺口。 | 不得将输入投影窄化误作放宽业务读档校验。G22当前错误可见但非字段级；与“显式给上下文”的体验承诺有局部差距，已在总账列出。无新候选。 |
| 3 核心纯逻辑、渲染/I/O外壳（L35–43） | Rust engine由Worker/WASM、Server actor、Tauri actor调用；生产核心并行任务入口 `session/pipeline/authoritative_tick.rs`。总账 R19/S02确认依赖方向主干存在。 | 宿主协议/生命周期缺口G01–G05/G18–G20说明“解耦”不等于三个宿主行为完全等价或可用；不需要整体重开架构。 |
| 4 单一数据访问层（L45–51） | Web存档经 `apps/web/src/save/`，UI命令经 host/store投影；Rust状态通过Session/SaveSlot；总账R01/R19认为持久化主干有单入口。 | 需要审查的关键 caller并不是散落浏览器直读：Stage 1槽及JSON IO集中；Server/desktop actor各自是宿主适配，不是重复前端本地存储。当前未在三份文档中发现具体生产缺口。 |
| 5 显式副作用/诚实类型（L53–57） | host API有`start/stop/save/restore`等显式命令，Rust错误为`Result`路径，统一HostUpdate作投影。总账 S02记录公共host行为。 | 老计划中命名/API差异已由主控 candidate-checks逐项核销（例如`total_assets`由会话个人权益消费替代）；不把旧名字缺失再做候选。G04继续重复启动重送baseline是caller行为边界，非抽象本身未实现。 |
| 6 小步、可回滚、重构/功能分开（L59–63） | Session tick使用candidate/commit、失败回滚，生产实现见 `session/pipeline/candidate_commit.rs`；总账 R02、§5检查了失败次序及新诊断提交。 | 不将内部候选回滚解释为用户可用日内存档；新架构长方案ADR-0018仍proposed，仅已接受的G16局部所有权目标是现行债。无与阶段决策矛盾。 |
| 7 文档与代码同等重要（L65–69） | ADR与交易规则文档对应生产模型；新总账记录调用链。当前发现Open Questions L9–11对已退役旧语料仍有动作要求，与总账/cleanup决定不符。 | 这正是原则7的当前候选：应同步正式文档，不能仅在代理工作记录里登记漂移。历史算术/API差异已由candidate-checks核销，不再重复建项。 |
| 8 性能基于证据（L71–75） | 多线程受理/股票流存在生产路径，`ComputeBackend`/Rayon、diagnostics及性能example有入口；总账G16/G17/R15将“实现存在”与“生产批量接线/性能证据”拆分。 | 缺稳定多核收益/年龄矩阵/真实宿主的长验收是证据债；性能不能从依赖Rayon推论。遵守AGENTS里共享300000ms/多核门槛。本次无任何性能声明或测试。 |
| 9 大A语义+独立复核（L77–86） | Q9和Roadmap Stage 1需受 `docs/trading-rules.md` 支配；Money以分、数量股、web手数换算。总账§1明确沿用已登记简化而未联网，G15等具体制度边界保留。 | 不能把依据证据债说成交易代码缺失或声称官方依据已重新核实；正式改动仍须非实施者复核完整diff。本复核本身没有产品diff、也不代替交易所规则取证。 |

## Roadmap Stage 与完成度矩阵

| Stage/原文 | 当前 caller与交付 | 最新决策、总账复核及界限 |
|---|---|---|
| Stage 1「本地可玩（已实现）…Rust/WASM Worker、行情与交易UI、A股核心竞价规则、本地/文件存档」（L5–7） | Web启动/Worker host驱动engine；`apps/web/src/App.tsx`呈现行情交易；save组件负责快速槽及JSON导入导出；Rust市场/orderbook/settlement执行核心语义。总账R07–R10/R17/R19确认核心路径已接线。 | 标题解释为可玩的阶段能力而非“所有UI契约/发版验收已通过”，由L3明确限定；G10–G15、G22–G34、G15官方覆盖输入边界仍有效。Q7明确不含多槽/云同步。没有证据要求撤销Stage 1已实现标签，但应防止把“核心竞价规则”扩大解释成全交易制度无简化。 |
| Stage 2「可选权威后端（进行中）…actor/REST/WS/重同步/销毁已实现」（L9–14） | `apps/server/src/actor.rs` session actor承载引擎，`routes.rs`负责REST/WS，浏览器remote host为调用端；总账G01–G05指出浏览器凭据、speed权限、pull取帧、暂停继续baseline、自动恢复等断点。 | 不矛盾：Roadmap陈述已实现的正向能力，仍有必要边界；公网部署前真实身份、授权、TLS、数据库、运营配额、运维指标是明确发布条件。ADR-0019移除引擎固定事件配额，运营限流未来不得冒充合法A股委托拒绝。 |
| Stage 3「Tauri actor、事件桥、暂停恢复、存档读档已实现；签名、自动更新、平台E2E后续」（L16–18） | `apps/desktop/src-tauri/src/actor.rs`及`lib.rs`管理会话与命令，Web desktop host消费事件桥；总账R19、G19确认actor骨架有而固定倍率聚合仍缺。 | 桌面应用不是“平台发版完成”：签名/自动更新/平台级E2E仍是roadmap未来范围；GUI/Wayland/性能统计证据亦在§6。暂停/恢复功能路径存在不代表Remote/Tauri重复start不重置状态，G04明确该具体caller风险。 |
| 总约束「Stage状态不是完整发布验收承诺」（L3） | 与G/Q矩阵与发布结论一致。 | 旧实现总账总体判法保持成立：局部生产实现、局部缺口、将来范围、验收证据并列；不能把老计划DoD、代码存在、历史短测互换。 |

## 旧总账结论重核与新增候选

1. **总账主要状态判断得到支持：** S05将Roadmap/Open Questions视为范围/状态索引而非逐模块完工证明；R/S映射保留G缺口、未来能力与验收债的层次；原则文档则是规范而非所有caller均已符合的实测结论。Stage 1完成与局部UX gap并存，不构成矛盾，因为Roadmap L3明确阶段标签不是完整发布验收承诺。
2. **文档旧结论需升级为实际同步候选：** 总账 `candidate-checks.md` S26指出Open Questions L9–11仍要求sealed legacy corpus真实比较，但 `docs/test-cleanup-checklist.md` §12批准退役适配器/复验能力；总账§5/§6及 coverage H01也明确不恢复这些工具。本全文复读确认冲突仍留在正式文档。应更新L9–11，只保留当前仍批准的证据边界、同条件性能报告/宿主门禁（若仍有效）及历史witness缺口表述，删除必须用退役语料比较的要求；修改范围仅文档，实施前应核实“性能报告及宿主最终门禁”本身是否仍为现行批准验收，不将其连带删除。
3. **编号碰撞：** Open Questions Q11表示NPC策略；总账候选表的Q11表示 `Correction/CreditDefault` 专门分发。这两个问题无语义关联。旧总账自己的引用易让后续处理错误复活NPC问题或漏掉会计事件问题；建议限定该项为 `Q11-Correction`/独立候选键，并全文更新交叉引用。它是总账分类修正候选，不是新增产品范围。
4. **精简的文档澄清候选：** Q3的“暂用MIT/备选”及Q4的npm/pnpm选项仍留在已经标“已解决”事项旁，容易误读；可把它们明确标为决策前历史背景。无需为此更改工程/许可证。
5. **未增列实现缺口：** Q1–Q12均已有决策或明确未来范围；唯一Q8新局取种问题早已正确归G20。Q11旧描述被新决定明确替代，G06–G09等独立调用断点不因总体决策落地而消失。Stage 1–3文案中的完成能力均在对应调用链有代码；不同scope的宿主认证、固定倍率、视觉/平台验收仍按G及未来范围保留。未从三份全文发现应新增生产功能G。
