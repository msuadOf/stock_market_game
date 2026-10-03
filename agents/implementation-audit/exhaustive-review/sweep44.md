# sweep44：开放问题、工程原则与Stage范围逐章核对

对象为源码 `b76ece3`（worktree审计merge的产品树相同）。本轮连续全文读取三份文件：`docs/open-questions.md:1`至157（157行）、`docs/principles.md:1`至92（92行）、`docs/roadmap.md:1`至18（18行），共267行。覆盖全部标题、段落、bullet及已解决参考表；三篇没有额外未读的checkbox清单。只写审计工作文件，未改产品、未执行Git写操作或长测试；以下源码/测试文件存在不等于本轮测试通过。

## Open Questions全部条款

| 原文 | 当前代码及后续决定 | 判定/旧总账映射 |
| --- | --- | --- |
| 3–7：当前实现按work-status核对、未决方向须人类裁决 | 当前多数Q已有ADR，不按早期问句重开；总账Q01–Q11区分事实与裁决 | 审计不擅自实施未决方案。 |
| 9–11：Escrow已决，旧密封语料真实比较、性能及宿主门禁 | `docs/test-cleanup-checklist.md:81`后续用户批准全清，`:88`明确接受旧证据不再可执行复验；当前`packages/engine/src/session/pipeline/authoritative_tick.rs:20`实际提交；`scripts/simulation/run-escrow-verification-matrix.mjs:103`附近仍验证性能报告 | 旧密封适配链不再是现行必做；同条件性能/当前宿主验收仍独立有效，G39及验收债不能随旧工具一并核销。此段旧表述需按后续删除决定解读。 |
| 17–19、参考表146：Q1 Rust→WASM | `packages/engine/Cargo.toml:1`engine crate；`apps/web-wasm/src/lib.rs:502`Rust绑定恢复，Web `host/wasm-tick-loop.ts:79`调用step | 已实现R01/S01，不再是语言决策。 |
| 23–25、147：Q2 Rust backend | `apps/server/Cargo.toml:1`Rust server；`src/routes.rs:443`等HTTP入口；`src/actor.rs:895`SessionManager | 已实现。 |
| 29–34、149：Q3 MIT | `LICENSE:1`MIT | 已落实，许可证不是产品代码遗漏。 |
| 38–44、150：Q4 pnpm workspace及版本固定 | 根`package.json:5`packageManager=pnpm@11.19.0、`pnpm-workspace.yaml:1`apps/web成员 | 已落实，Rust成员仍由Cargo workspace管理，不要求纳入pnpm包。 |
| 48–50、148：Q5 Redux Toolkit | `apps/web/src/store/store.ts:9`导入、`:39`等createSlice、`:232`configureStore | 已实现。 |
| 56–60：Q6首发中文、不引入i18n，第二语言未来需ADR | 当前Web文案为中文，未添加i18n框架；领域代码/部分技术错误用英文不构成第二语言功能 | B领域扩展保留，不能称具备国际化。没有新第二语言必做条款。 |
| 62–67：Q7快速槽+JSON，边界校验/Rust原子恢复；多槽/成就/流水/DB/云未来 | `App.tsx:112`统一浏览器repository、`save/save-repository.ts:64`单key；`save/save-file.ts:63`写前严格parse；`app/useSaveCommands.ts:63`读档generation门禁、`:70`日终档校验；WASM `lib.rs:502`restore；`session/persistence.rs:258`深度校验 | 基本能力已有。当前公开存档按后续日终候选契约，不能以早期JSON导出句子要求恢复已禁止的日内保存。多槽等为B玩家产品。 |
| 69–78、151：Q8 seed/PRNG可注入、保存、重放；seed不含并发受理轨迹 | `session.rs:129`SplitMix64、`:419`seed、`:440`rng_state；`tests/session.rs:4867`附近待处理/固定单恢复；实际受理在`pipeline/local_admission.rs`局部资源lane与股票gate | RNG已有；同seed自由worker逐字节相等不是现行承诺；G20普通新局固定42与G39旧验收仍独立遗漏。 |
| 80–84、152：Q9 tick+宿主驱动、全簿撮合、公共T+1、统一账户 | `pipeline/authoritative_tick.rs:25`阶段生产路由、`continuous_matching.rs:449`受理/撮合、`stock_auction.rs:330`清算；默认`apps/web/src/config/defaults.ts:114`T+1；Server/Tauri/Worker调用同protocol | 核心主干已有；简化和未支持交易规则见trading-rules，不能由“Stage1已实现”核销G10–G14等局部问题。 |
| 86–90：Q10亮色券商终端、Blueprint/AG Grid/Charts、响应式 | `apps/web/src/App.tsx`、`mobile/MobileStockDetail.tsx:101`分时/盘口组合、`store/store.ts`，实际桌面和移动组件存在 | 主体已实现；G22–G25/G30–G34及Q04/Q07/Q08仍有效，未做视觉像素/平台验收。 |
| 94–106、153：Q11独立策略、每实例参数、可插拔、玩家不走Strategy；共同V历史淘汰 | `strategy/mod.rs:194`trait、`strategy/factory.rs`实例装配；`session.rs:1893`个人分析档案，`decision_chain/roots.rs:95`五路候选；实际Player队列在`session/player_candidates.rs` | 旧共同V描述明确历史记录；扩展方式sealed等由Q09澄清。G07表明类型独立不等于Retail消费完整链。 |
| 108–115：K1–K7固定会计/报表/获知/混合/计划/经历/母单/2030/新档 | `session/decision_chain/roots.rs:323`价格观察、`:360`本人获知、`:394`只收年报、`:467`候选生命周期、`:479`计划执行；`session/persistence.rs:260`当前政策严格恢复 | 这些是当前必做契约，不能全归B；已知G06–G09/G15/G16/G28/G35–G38及Q02/Q03覆盖缺口；sweep25另发现个人价格记忆容量修剪未接。 |
| 117–120：机构本人风险/成本/不利选择、可暂停/恢复，不强卖 | ADR-0026及`session/institutional_behavior.rs`、`decision_chain/roots.rs:465`机构经历反馈；每机构独立参数在`session.rs:1908`流派生 | 已有，不再列为等待真实市场数据；G08只涉及散户dated/衰减，不把机构重复开工。 |
| 122–130、154：Q12费用退出、不补钱/返费/重置/造对手、缩量零成交诚实 | `session/pipeline/account_settlement.rs`按收据结算账户和费用；`pipeline/price_resolution.rs:67`非成交零费，竞价无交叉`stock_auction.rs:811`返回None | ADR-0024现行已决；不能为保证成交自动注入现金。没有发现本段新增未映射的现金来源需求。 |
| 132–138：经营侧收付/借还/息税可追踪，股东侧不执行，经营/投资者资金隔离 | `company/`各行业经营与`accounting/`账本，`session/company_operations.rs`会话接线；`docs/company-actions-design.md`显式股东边界 | 工商支付/折旧/所得税生产遗漏仍G35，四行业会话G36；分红/增发/回购/清算为B领域扩展，基金申赎/居民收入候选已被决定取消当前待办。 |
| 142–157：已解决参考表/统一HostUpdate | `apps/web/src/host/engine-host.ts`统一接口；`packages/engine/src/session/protocol/commit.rs:53`统一提交帧，Server/Tauri/Worker各自传输 | 各项引用与上表逐条对应；R01/R04/R06/R10/S03主体已有，G01–G05/G18/G19不能被“统一抽象存在”核销。 |

## Principles九原则及全部bullet

| 原文 | 当前实现/核对边界 | 状态 |
| --- | --- | --- |
| 8–17：TDD、红绿重构、小提交、公共API/核心测试 | `packages/engine/tests/session.rs`、`auction.rs`、`tick_protocol.rs`；Web `app/market-chart-runtime.test.ts`等配套测试；历史任务证据记红绿 | 公共主干测试存在，但无法仅由当前最终树证明每笔历史先红后绿。总账各G条目仍需生产边界补测，不能说“有测试即全部完成”。 |
| 19–26：所有外部输入校验、上下文用户错误、禁静默fallback | Web `save/schema/root.ts`严格根键、`host/protocol/parse.ts:87`等wire验证；`save/save-repository.ts:76`上下文错误；`app/useSaveCommands.ts:82`捕获展示；Server `routes.rs:107`结构错误；engine `session/failure.rs`显式fatal | 主路径已有；G22字段即时错误仍缺，Q01跨端Money边界仍待裁决。合法取消返回null、旧generation忽略与队列tail观察失败不自动视为静默业务fallback，须看调用方是否收到错误。 |
| 28–30：存档可编辑、原始最少事实、派生重建、不追溯历史、禁任意条数配额 | ADR-0019:31–43确立精简方向且明确未全部落地；当前`session/minimal_snapshot.rs`/`persistence/v2.rs`重建与提交证据边界；ADR-0019:27取消配置乘积配额 | 最小存档完整重构属于已登记架构方向，需按B长期存档/证据契约逐字段界定，不能为可编辑要求删除必要订单/冻结/账本校验；也不能自行加人为请求容量。 |
| 35–43：核心可序列化纯逻辑与壳I/O、三宿主复用 | `packages/engine/Cargo.toml`无React/DOM/network依赖；`pipeline/authoritative_tick.rs:20`候选提交；各壳timer/I/O在actor/worker | 已有分层。Rust内部可变状态、Rayon及受控allocator不等于依赖渲染/网络，不因字面“纯函数”误报整个engine未实现。 |
| 45–51：单一受控持久化接口、避免散读、校验错误、可替换后端 | `App.tsx:112`构造repository；生产localStorage访问搜索只在此注入入口；`save/save-repository.ts:70`save、`:81`load；文件统一`save/save-file.ts`，日终写排队`day-end-persistence.ts:20` | 浏览器快速槽受控，文件介质有独立受控适配。没有发现新的散落localStorage读写；数据库替换是可局部替换架构能力，不是当前数据库已实现。 |
| 53–57：命名诚实、副作用显式、禁魔法、Result/联合 | `host/protocol/reduce.ts`ProtocolReduction联合，`save/session-replacement.ts`显式generation gate；save/load/step各自命名 | 主干可核；null表示无档/取消并不违反诚实类型。没有据空泛原则新增未经触发链证明的缺口。 |
| 59–63：单问题小提交/PR、重构功能分离 | 本轮只审计文档；历史task17/19复核明确纯新增范围；当前merge不代表原产品提交混入多个领域 | 属历史过程门禁，不能从一棵最终树证明全部历史满足或违反；不新增代码G项。 |
| 65–69：ADR、变约定同步文档、注释解释why | `docs/decisions/0019*`、0023–0028后续决定均存在；open-questions早期sealed/roadmap签名措辞须按后续决定 | 残留文字过期单列文档澄清，不能驱动回退已批准实现。 |
| 71–75：性能约束、先正确、测量后优化、tick不盲猜 | `scripts/performance/`及simulation性能报告契约；`decision_chain.rs:88`有界recent_traded/并行路径；G16完整PlanBook复制、G31引用刷新、G18/G19发布仍有实际链证据 | 不把源码改动宣称速度提升；本轮未测性能。旧总账已有具体遗漏，不另造通用“没有性能优化”项。 |
| 77–86、92：现行A股官方依据、板块差异、单位跨层、简化登记、最小范围、独立复核并修复 | `docs/trading-rules.md`/`company-accounting.md`法源与简化；`StockSpec.exchange/category`，开收盘选择器；`.omo/evidence/`历史独立审查及当前审计复核工作文件 | 有具体规则差异和复核接缝；不把留存历史APPROVE当当前全面通过。G项与Q项需独立裁决/补验收，本轮无新交易制度变更。 |

## Roadmap全部Stage范围

| 原文 | 当前代码及最新决定 | 判定 |
| --- | --- | --- |
| 3、5–7：Stage1本地可玩不代表发布全面验收 | Worker `wasm-tick-loop.ts:79`、WASM Rust `lib.rs:502`、行情/交易、auction与受控存档链实际存在 | 主体已实现；G10–G14等局部遗漏保留。 |
| 9–12：Stage2 actor、REST快照/命令、WS、重同步、销毁、去条数配额 | `apps/server/src/actor.rs:895`registry、`:976`spawn；`routes.rs:675`intent、`:710`snapshot、`:1036`delete、`:1199`require_resync，WS publisher完整protocol | 主体已实现；G03 pull循环/G05自动恢复等按独立当前承诺保留。 |
| 13–14：公网前真实身份/授权/TLS/DB/运营配额/运维，运营限流不伪装交易规则 | `routes.rs:66`已有session_token比较，不能代表用户账号；Server Cargo当前没有DB持久化接线 | 现阶段进行中且明确公网前置，归B部署运维；不对外宣称可正式公网运营，不在本轮实施账号/DB/TLS。 |
| 16–18：Tauri actor/事件/暂停恢复/存档已有，签名/自动更新/平台E2E后续 | `apps/desktop/src-tauri/src/actor.rs:907`发布/`:933`事件emit；文件`save-file.ts:53`Tauri目标；后续ADR-0027:47–49明确Apple/Windows暂不签名/公证，build-and-deployment:80无updater | 安装包签名不是当前必做遗漏，roadmap应按后续用户决定解释；自动更新为未来产品能力；平台E2E仍验收债，不能以源码或构建产物代替。 |

本批新增独立代码遗漏候选 **0**。特别排除三项会造成重复/回退的误报：旧密封语料工具复活、当前签名强制要求、为维持长期成交自动补钱。现行必须链缺口仍按既有G/Q台账与sweep25新容量候选处理。
