# Luna21：PR 模板与 K7 兼容性映射全文复核

## 范围与阅读记录

基线：产品 `a7c7ce357bdc9f88c03633744b2d5815db49e9b2`。本记录只增审计文件，不修改产品代码或 Git 状态，不运行测试。全文连续读至 EOF：

| 文件 | 行数 | 全文覆盖范围 |
|---|---:|---|
| `.github/pull_request_template.md` | 46 | 1–46，含 HTML 注释、所有 checklist 与 EOF |
| `.omo/HANDOFF.md` | 88 | 1–88，含 §1–§6、表格、代码围栏、EOF |
| `.omo/evidence/company-information-npc-intentions/compatibility-removal.md` | 80 | 1–80，含 §1–§6、表格、EOF |

读取 worktree 根 `AGENTS.md` 与 `docs/principles.md`；确认现行决策 `docs/decisions/0025-day-end-only-persistence.md`。按原文承诺搜索了当前 Rust/Web 的生产定义、caller、持久化校验与既有复核结论。未因类型/函数存在宣称端到端通过，也未宣称测试已运行。

## 原文条款矩阵：PR 模板

| 行 | 原文条款 | 当前调用/实现证据 | 复核结论 |
|---:|---|---|---|
| 1–4 | 中文注释说明模板适用原则：读 AGENTS/principles；三铁律 | 本仓 AGENTS 与 principles 有对应规范；模板是提交者自述，不是代码功能调用点 | 规范一致；勾选内容仍须由提交人诚实填写 |
| 6–8 | 关联 Issue、Closes # | 模板文本无自动闭环代码 | 项目流程约定，不当产品漏实现 |
| 10–12 | 改动 what/why | 模板提供说明位 | 约定，不声称验证了某个 PR |
| 14–22 | 测试 TDD、通过与不弱化断言，列具体用例 | 原文是 PR checklist | 诚实核验项；本文没有运行测试，不替任何实现勾选 |
| 24–29 | 显式错误、输入校验、fallback 论证 | 原文是自检项；源码规则见 AGENTS/principles | 规范一致；不以模板存在证明错误路径均完成 |
| 31–35 | 文档/ADR/提交格式 | 项目目录和提交规范存在 | 流程要求，无新产品承诺 |
| 37–41 | 开放问题选择 | `docs/open-questions.md` 是当前登记源 | 自检要求，无自动核销机制主张 |
| 43–46 | 新增依赖说明 | 模板要求列依赖/理由 | 约定一致，EOF 已读 |

## 原文条款矩阵：HANDOFF

此文件开篇明确为 **2026-09-30 核销的历史快照**，保留 2026-09-11 交接原貌；并明确后续实现完成状态不可由旧 `[~]` 或 orchestrator `completed` 推出，权威现状转到 `docs/work-status.md`。因此下列旧行动指令不可按当前待办重放。

| 行/章节 | 原文条款族 | 当前 caller/证据追踪 | 复核结论 |
|---|---|---|---|
| 1–8 | 历史声明、计划/进度、提交链、当时测试基线、复核状态 | `docs/work-status.md` 为当前完成/未验收/阻塞分类入口；当前 HEAD 为本次指定基线，不是文内 `14cc965` 状态 | 时效性由文件自身限定。历史计数、远端分支、旧 commit、旧测试基线不能当现状 |
| 9–35（§1a） | Task 27 WIP 状态、未验证内容、剩余工作与旧机器动作 | 现行存档入口为 `packages/engine/src/session/persistence.rs`、`session.rs`、Web `apps/web/src/save/schema/`；`tests/save_contract/` 有独立新格式用例 | 全部为历史 WIP 快照。禁止照搬“重写 happy/raw 证据、重跑旧基线、最后提交”等旧交接指令；是否验收按 work-status/当前源证据查，不由本轮推定 |
| 37–53（§2） | Task 27–42 依赖图、性能/产物隔离 | 现行各模块与验证脚本可单独追踪；该计划的工作状态文档已替代交接进度表 | 依赖图作为历史计划保留，不是当前执行授权；不能仅据计划完成宣称真实验收已通过 |
| 55–62（§3） | 新机器克隆、环境版本、WASM build、跑旧基线、启动 OpenCode | 当前产品层代码/脚本而非该历史交接负责运行；本轮未执行任何流程 | 已过时的操作步骤；绝不可据此触发 Git、网络、长测或实现旧任务 |
| 64–75（§4） | learnings/issues、RNG 纪律、存档、环境陷阱、clippy、证据、before、engine_error_events | 当前 RNG/存档由 engine owner 与 caller 实现；错误观测项需按现行工作记录解释 | 约束中仍有些可作为历史证据/领域线索，数值/路径/环境状况不可无验证沿用。尤其“TS 生成积压 ~44”及旧 clippy 清单是快照，不是当前状态 |
| 77–84（§5） | Task 27/29 探索要点摘录 | 现行 DTO、WASM/TS 生成边界可在 `company/query.rs`、`apps/web/src/save/schema/` 与 generated types 追踪 | 历史探索摘要；当前字段契约由代码、ADR、正式文档决定 |
| 86–88（§6） | Git 未推送、remote、bundle 和保护用户改动 | 不执行 Git 操作；当前任务要求只读审计 | 历史 Git 状态不能当当前状态；全文 EOF 已读 |

## 原文条款矩阵：compatibility-removal

| 行/章节 | 原文条款族 | 当前 caller/owner/consumer 证据 | 复核结论 |
|---|---|---|---|
| 1–6 | 范围是 Rust SaveSlot/restore；TS validator、schema 特判、fixture、WASM normalization 归后续任务 | 当前 TS schema 在 `apps/web/src/save/schema/root.ts` 与子 schema；Rust 是 `packages/engine/src/session/persistence.rs` | 这是 Task 27 的范围切割，不等于当前 TS/WASM 的状态承诺；以现行实现/ADR 为准 |
| 8–24（§1 #1–#7） | 删除日历重放、adopt_all_pending、重置个人状态、剥离 linked plan、重发披露、默认日历重建、共同 V | restore 生产路径为 `GameSession::restore` / `validate_save_slot`；`mirror_is_exact`、计划互洽和 `CivilClockSave.policy` 有实际恢复校验；共享 V 路径维持删除 | 除 schema 版本说法外，所列当前状态保存/验证主干仍可在现行代码找到；不因此声称每个不变量被运行验收。V 保持删除与后续决策一致 |
| 26–30（§2） | “没有 schema_version 字段”；旧形状通用拒绝；unknown fields 拒绝 | `SaveSlot`/Web 当前显式有 `schema_version`；`tests/save_contract/failures.rs` 覆盖缺版本、旧版本/未来版本，decoder/validator 负责显式版本拒绝；ADR-0025 要求严格格式无迁移 | **原文过时且被后来版本化契约替代。** 旧形状不迁移仍成立；“没有版本字段”及 schema_version 属于普通 unknown 顶层键的说法不成立。不要恢复删除版本字段的旧要求 |
| 32–48（§3） | K7 必需状态、交叉校验、pending 事件晚到淘汰、保存限额 | `SaveSlot` 持有 K7 状态；`validate_save_slot` 做恢复交叉校验；`PendingPlanEvent` 生产入队/同步路径可在 execution/plan_execution 追踪；`decode_save_slot` 对输入字节执行边界 | 存档整体结构存在。需留意个人记忆容量的实际语义缺口（见下文候选反证）；不能让“有一个全局股票数+8检查”冒充 per-account 未保护集上限。旧 256 company 与其他 count 限制须按 ADR-0019 当前决策判读，不能只抄本节旧额度 |
| 50–56（§4） | `PersonalPriceMemory` 要求随档保存，观察事实与 belief 同键；拒绝坏价格/乱序/未来/不合理记忆；held+8 上限 | `roots.rs:323–337` 每次候选推进 `observe_price`；`roots.rs:481–486` 计算持仓∪活跃计划集合后只调用 `watchlist.prune`；`price_memory.rs:182–198` 有价格记忆 prune；生产源码搜索未找到其调用者。恢复 `persistence.rs:1184–1193` 以 setup 总股票数+8 限总量，之后要求键都是 setup 股票 | 保存与多项逐条验证有实现，但有限未保护记忆在生产更新时没有淘汰；restore 容量检查也不是实际“保护集+8”，而是比全部合法股票数宽的上界。此点仍有效，接续 S03-N2/S21-C01，不另造重复编号 |
| 58–66（§5） | 原子恢复/测试强度、replay 锚点及数值 | `save_contract`、session 与 replay 测试文件提供当前入口；本轮未运行 | 只核实有测试入口，不能报告断言或数值现已通过。文内旧锚点属历史证据，不据其反推现行全验收 |
| 68–80（§6） | Task 29 Web 映射：无 schema version；根字段拒绝；SessionSetup 移除 V；整数用字符串 | Web schema 当前版本字段及相应测试见 `apps/web/src/save/schema/root.ts`、`save-schema-v2.test.ts`；Rust current setup / generated TS 定义可现查；Web整数边界仍需以具体字段校验器为准 | 首两行“无 schema_version”已经被现行契约覆盖；V 删除部分方向上仍成立。数值序列化不能靠本旧总结泛化到所有字段，须逐字段检查。修订记录只说 `engine.ts` 删除 V 类型的事后状态也须按当前代码核验 |

## 旧结论复核与候选反证

1. 旧复核 [S03](sweep03.md)、[S21](sweep21.md) 报告 `PersonalPriceMemory::prune` 缺生产调用；后续 [S54](sweep54.md) 明确指出共享 `RetentionCandidates` 的抽取并未补齐 Session caller，仍沿用 S03-N2。当前检查直接证实 `roots.rs:486` 只修剪 watchlist，未修剪 `personal.price_memory`，故旧结论仍成立。
2. 候选路径可形成不超过 setup 公司数、但超过 held/active+8 的个人记忆状态：root 对持仓、belief 与本轮 discovered candidates 观察价格；当 belief/候选历史覆盖多股且保护集合很小，旧条目保留。此处静态链足以指出 prune 漏接，但没有构造运行复现，不描述成已观察到的用户损坏。
3. restore 上界 `setup.stocks.len()+8` 后会拒绝未知证券，而合法 memory map 中每个 key 都必须是 setup 股票；所以合法 key 总数本来就不超过公司数，这个 cap 并未实现每账户保护集之外至多 8 条的承诺。又因恢复路径明确不应静默裁剪，本地应由准确校验拒绝超额或存档写入前维持不变量；具体修复设计不在本审计范围。
4. 不把此发现扩大为“不限量存档/任意请求配额”问题。其语义是已承诺的个人淡忘规则；与 watchlist 的确实生产 prune、`record_public_history_read` 的 Q02 历史读取留痕、资金/交易制度皆独立。
5. “无版本字段”是最重要的历史反证：旧文自身提出无 schema version，但产品现在明确使用版本字段、检测 legacy/future 并无迁移。故兼容移除仍是无迁移原则，具体报错路径已由后来的版本化协议定型。HANDOFF 已在开篇自证为历史快照，不能覆盖此决定。
6. 尚未发现这些三份指定文件中独立于既有登记、又有当前有效需求支持的新候选。此结论仅限三篇全文及其跟踪路径，不是全仓审计或运行验收结论。

## 结论

保留已登记的 S03-N2/S21-C01：价格记忆观察有生产 caller、修剪算法和测试，但没有生产 prune caller；恢复侧所谓 held+8 检查没有落实保护集语义。修订兼容性映射对 `schema_version` 的历史断言，应服从现行版本化存档契约。PR checklist 是自检模板；HANDOFF 明确为旧快照，既非当前任务列表，也非验收证据。全文范围内未发现 A 股交易概念、单位或规则被改变；本审计是静态存档/个人状态调用审查，未运行测试。
