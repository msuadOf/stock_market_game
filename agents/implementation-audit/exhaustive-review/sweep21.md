# sweep21：PR 模板、历史交接与存档兼容移除全文复核

## 范围与阅读

已按连续行完整阅读 `.github/pull_request_template.md` 第 1–46 行、`.omo/HANDOFF.md` 第 1–88 行、`.omo/evidence/company-information-npc-intentions/compatibility-removal.md` 第 1–80 行，合计 214 行，全部到 EOF。先前已读根 AGENTS.md 与 docs/principles.md；本轮路径下未发现更深 AGENTS.md。源码绑定 b76ece3；工作树 HEAD 后续合并变动不改变产品（`git diff --stat b76ece3 -- apps packages scripts .github` 无输出）。只新增本工作记录，未改产品/测试，未运行测试或构建，未 Git 写操作。

判定依当前正式决定：ADR-0017 的 schema v2、ADR-0019 的取消任意容量配额、ADR-0025 的公共日终档、ADR-0028 的发布仅构建，以及 docs/testing.md 的实际受理轨迹边界优先。历史勾选与 APPROVE 不直接证明当前实现，更不等于本轮运行通过。

## PR 模板逐条款族

| 全文章节/原文 | 当前判定 | 当前代码/正式决定与反证 |
|---|---|---|
| 注释 1–4：读守则、TDD、防御、诚实 | 协作门禁，非独立产品功能 | 根 AGENTS.md 和 docs/principles.md 已读；不根据文件存在推断每次历史实现都执行了 TDD。 |
| 关联 Issue 6–9、改动说明 11–13 | PR流程，不产生runtime caller | 不是要求软件提供Issue系统；本轮审计文档不对外开Issue/PR。 |
| 测试 15–24：先失败测试、全绿、不得弱化 | 协作门禁；全绿文案须按现行验证范围解释 | `.github/workflows/ci.yml:7` 仅手动开发诊断；`:213` Clippy `-D warnings`，`:219` Web lint。发布依 ADR-0028:15 不执行测试，不能因PR模板反向恢复自动PR/发布测试。现有Web warning门槛缺口是G26，模板没有新增更广的产品承诺。 |
| 错误自检 26–30：显式处理、外部输入校验、可见fallback | 公共输入主干已有；专题G/Q不核销 | Rust `session/persistence.rs:963` decode→`:258` validate→`session.rs:2716` restore；Web `save/schema/root.ts:51` exact/parse，再交给Rust深度恢复。没有以“总体已校验”否认本轮发现的具体容量语义遗漏。 |
| 文档/决策 32–36 | 协作约束 | 当前ADR演进会覆盖旧工作证据；不把历史证据当现行正式规范。 |
| 开放问题 38–42、新依赖 44–46 | 协作约束，未新增产品路线 | 当前工作不引入依赖、不替Q03选择法规政策结构、不开展GPU/多人同局。 |

## HANDOFF 全章节追后续

| 全文章节/原文 | 当前判定 | 后续生产/正式证据 |
|---|---|---|
| 标题与历史快照说明 1–9 | 已显式核销WIP入口 | 第3行直接标“历史快照，非当前待办”；`docs/work-status.md:317` 核销26/46与任务27WIP，`:318` 保留末尾未完整验收，不能再按第9行操作恢复旧协调器。 |
| §1 状态表 13–22 | 历史数字/分支/HEAD，不作为当前测试结果 | 不按944/0/4测试数判断当前删测试；当前代码和正式测试边界另验。旧分支、Boulder、远端属于交接事实，没有产品漏实现。 |
| 本轮完成 24–27 | account/plan/共同V删除主干已有；旧G不核销 | 生产 `session.rs:2518` 玩家请求进入统一链；公司信息/个人计划持久状态在`:2659`；旧V不再在生产Rust/Web DTO中出现。`rg`只命中历史测试注释的旧V名字不算残留产品路径。个人分析/披露等已知G06–G09/G28/G35–G38仍需专题判断。 |
| 复核门禁 29–30 | 历史复核记录，不替代当前全diff审查 | 历史REJECT→修复→APPROVE记录可保留，本轮未重验全部任务。 |
| §1a 任务27 WIP 32–43 | 后续实现已替代 | `session.rs:2926` 直接从档覆盖公司/信念/计划/信息；`:2955`至`:2978` 实际赋值；`persistence.rs:1011` mirror精确检查；`.omo/evidence/.../task-27-review.md:5` 历史APPROVE。不是只依赖勾选核销。UTF-16/原始happy证据债不能凭当前源码改写历史绿灯；不重启全量验证。 |
| §2 下一步与依赖图 45–56 | 历史执行编排已退休 | 任务27生产实现已有；task37等末尾验收仍由`work-status.md:318`单列。测试资源隔离有意义，但旧依赖图不要求重新实施27–42。 |
| §3 恢复步骤 58–64 | 历史环境/启动指令，不是产品缺口 | 当前 `package.json:17` types生成、构建脚本有WASM构建入口；旧分支checkout、944数字、OpenCode会话指令不在当前授权操作。机器pnpm PATH/PowerShell编码不是软件运行遗漏。 |
| §4 契约/陷阱 66–76 | 真持久状态/RNG已有；条数配额被替代；旧锚/工具退休 | 个人RNG `session.rs:1887`/`:1895`/`:1908` 分stream；`:1932`注意力独立。新档已有schema v2而不是第71行无版本旧形状（ADR-0017:131）。旧before语料/适配栈按`docs/test-cleanup-checklist.md:81`已退休，不要求修复或复原。旧Clippy债不能从旧行号认定当前必红，当前手动CI已有Clippy门禁，本轮没运行。 |
| §5 Task27/29探索摘录 78–82 | 主干已有；新增候选S21-C01；其余旧约束核销 | 公司/个人/时钟域入档见SaveSlot+save/restore。`company/query.rs:16`/`:17` 页20/上限100，`session.rs:285`/`:294`有两种Civil/Disclosure事件；TS root严格字段，u64字符串；共同V导出删除。公司256配额由ADR-0019:27明确取消，只保留512MiB解码边界。历史个人记忆容量保证未完全兑现，详见候选。 |
| §6 Git交接 84–88 | 纯历史协作/保护规则 | 不对“未push”做当前远端假设，也不据此执行push/reset/clean；本轮无Git写。 |

## compatibility-removal 全章节与全部映射行

| 条款族/原文位置 | 当前判定 | caller→owner→consumer及边界 |
|---|---|---|
| 标题/范围 1–6 | 历史任务分工，TS后续已接 | Web `root.ts:51` parse→宿主load→Rust ProtocolSession::restore，不再因为最初任务27不负责TS而留下豁免。 |
| §1 #1 12：删除逐日经营重放 | 已实现 | `GameSession::restore` `session.rs:2926`明确不再逐日重放，`:2955`整体覆盖CompanyOperations；当前new用于构造合法owner，再赋权威档事实，不把new时合成前史误判旧逐日经营重放。 |
| §1 #2 13：删除adopt_all_pending | 已实现 | 未找到生产adopt_all_pending，`persistence.rs:1011`镜像==调度待办，`:1021`时钟due多重集包含检查。 |
| §1 #3 14：四类个人状态必填/不复位 | 已实现；逐字节未来演化承诺修订 | `session.rs:2962`以保存book/information/watchlist/memory构造participant；`:2978`恢复pending队列。恢复立即全事实等价与未来自由调度一致是两回事，后者不按旧措辞要求。 |
| §1 #4 15：linked_plan_id保留且交叉校验 | 已实现；公共日终无日内母单是新契约 | `persistence.rs:1441`校验存在/非终止/同账号股票；不为旧“母单入档”要求公共日级档保留日内订单，ADR-0025:29禁止。跨日计划仍入档。 |
| §1 #5 16：披露游标无补发/前视 | 主干已实现 | `session.rs:2959`恢复disclosures；`persistence.rs:1040`之后重验披露/library日期；已知G28完整集团披露是独立问题。 |
| §1 #6 17：冻结日历policy/digest | 已实现 | `session/civil_clock.rs:238`from_parts→`:243`CalendarPolicy::from_parts→TradingCalendar::from_policy，保存`:459`policy spec。它不等于Q03法规冻结结构已裁决；也不核销G15非空official覆盖回退错误。 |
| §1 #7 18：删除共同V | 已实现 | 无生产VParams/v_initial/v_params/evolve_v/fundamental_value_means，测试注释历史锚说明不算残留实现。 |
| §2 20–25：无schema_version/通用旧形状拒绝 | 无迁移器仍成立；字面无版本/通用拒绝被ADR-0017替代 | `session.rs:411` schema_version必填；`persistence/v2.rs:97`先header、`:109`显式legacy/future拒绝，无迁移/兼容fallback；Web `root.ts:54`至`:56`同语义。重新删除schema_version会破坏现行v2契约。 |
| §3 27–36：必填权威域/pending过滤/policy ID宿主拒绝 | 权威域与过滤已实现；v1/宿主仅拒绝被替代 | `session.rs:2659`公司域到`:2688`memory，`:2697`过滤未知/终止计划pending事实；`persistence.rs:260`在engine拒绝非v2 policy。Q03仍是ID与完整规则集合关系，不能因engine承担拒绝判功能漏接。 |
| §3 38–46：公司/镜像/披露/个人键集/引用/计划交叉校验 | 主干已实现；记忆容量S21-C01仍缺 | `persistence.rs:977`公司域，`:1101`个人键集及来源/时序，`:1184`memory形状，`:1441`plan链接，`:1453`pending plan/order/day。`session.rs:2945`重建账户集精确相等。不是只验证serde字段。 |
| §3 48–51：512MiB/256公司/计划等条数额度/失败原子性 | 字节额度与错误已有；条数配额被取消 | `SaveDecodeLimits`在`persistence.rs:948`仅max_total_bytes，`:964`ResourceLimit；ADR-0019:17/27明确取消计数额度。restore在临时新session内运行，宿主只在成功后换owner，不能恢复任意条数限额。 |
| §4 53–58：PersonalPriceMemory恢复严格/held+8 | 部分已有，新增S21-C01 | 键集与非正/未来/极值校验已有；容量cap以setup股票总数计算，未验证真正未保护条目≤8；生产price memory也缺prune调用，详见下文。Q02主动读历史留痕未接与本项不同。 |
| §5 60–68：原子/T+1/锚点/同seed字节 | 原子/日级语义保留；锚点被后续场景替换 | `tests/extraction_replay.rs:3`明确单worker受控场景，`:47`记录v2变化，`:52`说明当前非旧语料等价证明；`:63`及`:71`记录后续真实锚漂移原因。公共加载`protocol/civil/session.rs:100`先deep restore，`:111`再日终限制；不能恢复旧日内存档。未重跑字节/原子测试。 |
| §6 Web映射 70–80，行74根schema | strict新格式已实现，旧无version承诺被替代 | `root.ts:49`精确keys，`:54`当前v2检查，缺/extra拒绝。 |
| §6 行75/76：K7必填、删除V默认/DTO | 已实现主干 | `root.ts:62`各domain parser；`save/schema/market.ts:45`setup精确fields且无V；`types/engine.ts`未导出VParams。 |
| §6 行77：整数/会计值不经Number | 已实现u64传输；Money范围仍Q01 | seed/rng股份字符串parse见root/各parser；Money仍number安全整数，旧“整数全不经Number”不可据此核销Q01，也不要求直接移除精度guard。 |
| §6 行79–80：不导出VParams | 已实现 | 未找到Web生产VParams导出。 |

## 新候选 S21-C01：个人价格记忆淘汰及恢复容量校验漏接

原文 `.omo/evidence/company-information-npc-intentions/compatibility-removal.md:57` 要求恢复拒绝超过 held+8 的价格记忆；正式公司计划 `docs/superpowers/plans/2026-09-10-company-information-npc-intentions.md:135`同一承诺，后续`:491`细化受保护集为「持仓∪活跃计划」，其余至多8条。这是个人认知/淡忘规则，不是ADR-0019取消的机器容量配额。

实际生产链：InstitutionDecisionRoot观察候选在`session/decision_chain/roots.rs:325`写`personal.price_memory`；root末尾`:482`构造持仓∪活跃计划protected，但`:486`只调用watchlist.prune。`experience/price_memory.rs:187`的prune算法已存在且`:193`上限8，未找到生产调用；`personal_state.rs:109`整体安装未经淘汰的memory，`session.rs:2684`整体保存，`:2970`直接恢复。故当自定义市场超过8只且账户陆续观察不同股票时，已无持仓/活跃计划保护的旧价格记忆不会按约定淡忘。默认五股票无法暴露该边界，不声称默认玩家已发生错误或存在无界世界增长。

恢复侧`persistence.rs:1165`与`:1184`分别对watchlist/memory计算`cap = setup.stocks.len()+8`，随后又在`:1176`/`:1195`要求每个map代码属于setup股票。合法代码map数量天然≤setup.stocks.len()，因此这两个cap检查对合法代码永远不能触发；错误信息“held+8”不能证明已按账号实际保护集验证。不应在restore默默prune或补默认记忆，原文`:58`明确禁止修剪/重建缺失记忆。

反证已考虑：watchlist生产prune确实已接，不能报关注列表正常运行也完全未淘汰；price memory库prune单测确实存在，不能报算法没有实现；活跃计划可以保护未持仓股票，不能机械按positions.len()+8拒绝合法计划记忆。restore后的`reconcile_institutional_holdings`（`session.rs:1547`）只协调成本经历，没有修剪memory/watchlist，未补上该边界。当前`tests/save_contract/failures.rs:206`覆盖缺个人键、未来分钟、读取计数/时间不匹配，未覆盖未保护第9条容量或protected计划例外。本轮未执行fixture，不将以上源码推导称为运行复现。

建议总账独立复核后按一个条款族登记：生产记忆prune接线与恢复合法未保护条目上限校验；代表性短fixture为≥9只股票、无持仓无活跃计划的9条已观察记忆，另加持仓/活跃计划保护条目不误拒绝。保持本人实际观察历史，不能为凑数量伪造亲历或删除仍保护的记忆。

## 其他候选的反证与总账变化

- “任务27仍WIP/恢复经营逐日重放”已被当前直接恢复路径反证，不新增G。
- “旧格式schema版本识别/256公司上限没有兑现”是后续ADR替代，不恢复旧要求。
- “仍有serde(default)就表示K7缺字段会静默丢状态”不成立：权威个人/公司域必填，setup开局默认/可选报告字段必须分别看具体语义。本批不凭关键词添加G。
- “HANDOFF旧完整回归基线未复现”归历史/当前验收债，不是缺模块；旧适配器/语料已获批退休。
- 新候选1项S21-C01；旧G/Q不核销，G15/G26、Q01/Q02/Q03与本批关联但保持原分类。无新增未来产品要求。
