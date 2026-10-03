# sweep76：OOP 总结、精确短测索引与发布仅构建

## 全文范围与基线

生产基线 `b76ece3`（审计 merge 产品相同）。已读根 AGENTS.md/principles。三文件连续全文读取至 EOF，总计685行；长索引首次输出截断后按行块补全，R001–R278与128 action表均已完整读到正文末尾。没有执行Cargo、产品测试、构建、Git写操作或GitHub操作，只新增本分片。

| 文件 | 行数 | EOF |
|---|---:|---:|
| `agents/oop-refactor-implementation/summary.md` | 44 | 44 |
| `agents/oop-refactor-implementation/validation/rust-short-plan.md` | 600 | 600 |
| `agents/oop-release-validation/build-only-change.md` | 41 | 41 |

## OOP summary 逐章

| 原文/行号 | 状态及当前入口反证 |
|---|---|
| 总述 1–3；实施入口 5–18 | 128动作是历史重构任务完成数量，不是股票产品所有需求完成。当前真实owner存在：`session/pipeline/candidate_commit.rs:97`消费PreparedCandidateCommit并安装候选，`session/protocol/civil/session.rs:12`持有PublicationFactCursor，`:39`方法实现；`experience/feedback/lifecycle.rs:43`等六dated writer消费PositionExperienceTransition。不存在只加空包装的本批新缺口。 |
| 行为与边界 20–26 | 重构明确保持原数据/失败/序列化接受边界；完整候选commit唯一authority入口 `candidate_commit.rs:98`。四份个人字段聚合在 `session/decision_chain/personal_state.rs`，存档仍投影四map。保护这些已有owner不等于补齐G07/G08/G16/G37等生产缺口。 |
| 实际验证 28–38 | 四包编译、278case、TS/lint与环境重跑属于该批历史证据；原文明确未跑复杂回归/E2E/长期/性能、两个旧长fixture未跑、compile_fail未跑，不把它们计入缺失产品功能，也不冒充本分片重新执行。 |
| 原有工作范围 40–44 | b89afb3保护基线、无新增依赖/lock/wire/规则/推送发版是实施边界；本轮只核源码/文档，没有改他人已有文件或远端。该summary不授权追加产品功能。 |

## Rust 600行索引逐章

| 章节与起始行 | 状态、核对范围与结论 |
|---|---|
| 标题/最终索引 1–9 | 文档名是旧plan，但正文已明确转成最终索引；不能把“索引维护者本人未运行”误报成root从未运行。278行case与唯一ID数量均由本次只读Node文本解析确认，final-validation.json自报278/278/0、tests数组278。此为记录一致性，不是重新测试。 |
| 执行参数 11 | 8进程×4Rayon、单case和整命令10000ms明确记录；JSON相同。未运行这些runner，不把历史最大1.778秒当当前基线实测。 |
| 编译证据 17 | build05/07/check08/09/build10/check11/12的记录与时长为历史编译验收；不把编译>10秒误当普通case超时，原本300秒构建上限另列。 |
| package/target/features 29 | 237 lib +36 integration +5Writer=278；required/compiled features分开列。Writer专用workspace/.tmp路径是已有约定，不能因第一次环境失败而重开已成功重跑的旧错误。 |
| 范围限制 57 | hosts-N04、hosts-R2-N05只编译/静态review；compile_fail未运行；server_only_build_rejects_static_modes被feature cfg排除，不计通过。都是诚实验收限制，不能转成产品代码尚未实现或强制本轮跑长测。 |
| 精确case索引 68；engine lib 70 | R001–R219全文读完。针对orderbook/策略/账户/经历/session/pipeline/诊断/会计/四行业owner、失败顺序与serde的定向保护。尤其R026–R029只证明dated writer内核，并不能核销散户生产G08；R190旧重复年度税/R199/203保险极值/R219地产部分写入明确是继承行为保护，不凭测试名新判OOP引入故障。 |
| server lib 294；web-wasm 309；desktop 318 | R220–R237全文读完。Pacing、publisher、registry、fatal路径存在；server的pull容量case R229不证明Web Remote已实际定时发送GetFrame（G03仍有效），desktop Pacingcase不证明固定倍率16ms聚合（G19仍有效）。 |
| information 327；discovery 334；auction 341；behavior 348；belief 355 | R238–R247全文读完。信息权限、曝光、竞价、个体目标、信念已有定向case；不能用这些内核case核销G07/09跨身份/中期生产缺口。 |
| reports 362；plans 369；publications 376；session 383；account 390；strategy_state 399 | R248–R260全文读完。更正底稿/计划成交/公告时点/冻结恢复/账户溢出/策略状态保护已有。合并公开和混合行业仍按G28与对应分片发现处理，不因纯单体更正case通过核销。 |
| CLI 405；static routes 414；API 422；WS 430；save 437；Writer 443 | R261–R278全文读完。CLI/static边界、API恢复、fixture清理、Writer字节/部分失败/路径验证均已有声明；本次静态解析逐一确认所有278个引用源码文件存在且完整filter末段对应`fn`声明，mismatches为空。不是运行这些case或审查所有函数完整实现。 |
| 128 action映射 453 | 128行全文读完并只读解析计数128。99 Rust、27 Web/Node、2长fixture编译review的分配明确，映射允许同case保护多个动作；不以“所有动作实施”偷换“每动作有独立新增测试”或“128功能全部验收”。 |
| 源记录与版本绑定 588–600 | 明确仅机械更新JSON/声明行/SHA、不重跑。N07文件迁移元数据保留原证据时点；最后五proof组数量230+2+39+2+5=278，数学一致。记录SHA为历史绑定，本次没有核全部source SHA一致性，因此不宣称新源码冻结或完整diff复核。 |

## build-only 逐条追现行入口

| 原文/行号 | 当前实现与后续决定 |
|---|---|
| 用户决定/范围 3–4 | `docs/decisions/0028-tagged-release-and-static-pages.md:3`明确2026-10-03发布仅构建；`:16`禁止Release调用CI、测试套件。晚于历史“发布必跑检查”的工作记录，按此核销旧发布测试强制要求。 |
| Release去CI依赖 6 | `.github/workflows/release.yml:32` distributions只needs validate，`:42` publish只needs validate+distributions；当前无CI job/reusable CI引用。标签/SHA校验仍在 `:26`–`:30`，不是误留旧开发检查。 |
| 分发去smoke/测试保留build 7–10 | 当前distributions无Playwright/smoke/契约测试入口；`:77`调用frontend-build，`:93`生产Pages Vite构建，`:98`static打包，`:229`/`:233`Server原生构建，`:238`Desktop compile-only，`:241`其他UI产品构建，`:252`原生打包。固定wasm-pack检查`:75`是工具chain约束，不是被删除的产品测试。 |
| 生产编译/发布边界保留 8–10 | `scripts/frontend-build-plan.mjs:22`生产tsc，`:18`/`:21`WASM线程检查，`:19`/`:24`发布私有导出检查；`scripts/check-web-release-wasm.mjs:15`核一个game WASM且`:26`拒私有NPC诊断。build-only不取消制品安全与wire边界。 |
| 十组与公开前资产核验 9–10 | `scripts/publish-release.mjs:16`要求10分发目录、`:31`合法product/target、`:41`–`:49`核manifest大小/hash；`:83`tag来源核验，`:101`–`:106`远端资产核验，`:109`第二次SHA核验后`:113`公开。原G27已有守卫继续核销，不宣称GitHub原子标签锁。 |
| 手动CI与Pages 12–13 | `.github/workflows/ci.yml:7`只有workflow_dispatch，无workflow_call；build-web仍有workflow_call供Pages复用（这是Pages，不是CI），release`:65`–`:74`在publish成功后Pages复用。不能把build-web的workflow_call当CI删除失败。 |
| 文档同步 15–17 | 正式ADR0028、build-and-deployment/testing当前描述发布仅构建；`docs/testing.md:124`继续保留手动诊断CI。交易模型/存档/运行时未被发布策略文案改动。 |
| 验证及环境失败 19–41 | old-red/green/59case/YAML/独立review都是该批静态契约与短测证据；原文36明确未跑完整回归、三平台实构建、发布或线上smoke。sandbox EPERM被明确排除，不重开已对照验证的旧环境失败；本分片也没有actionlint或线上结果。 |

## 候选反证与总账归属

- **未确认本批新的独立代码遗漏。** 索引是已执行短测的记录，build-only是正式后续决定，并没有未实现产品功能藏在“未跑测试”的措辞中。
- **G08/G03/G19/G28等不能被case存在核销。** 有dated writer、server GetFrame、Pacing、报表纯函数保护不等于对应实际生产caller已完成；这些既有漏项仍沿用专项代码证据。
- **G26仍限定手动开发CI。** 发布不调用lint是最新用户决定，不再列Release lint缺口；手动frontend warning失败门槛按原总账保留。
- **W类长期/浏览器/完整验收边界保持。** 278精准短case与128动作实施不是三宿主多年/统计/性能/平台全验收。已知长fixture只编译review也不意味着代码待实现。
- **不把“末次无caller wrapper删除”列缺口。** 过渡wrapper不属于有效产品契约，真实candidate commit、publication cursor与dated writer内部owner均有当前消费链。

本分片全文阅读685行并做只读记录/声明一致性核查；不把此机械核查称为运行验证、重新全仓源码复核或源码hash冻结。
