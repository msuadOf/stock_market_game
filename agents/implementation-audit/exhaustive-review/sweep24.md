# sweep24：Task 14–16 历史独立复核全文追踪

目标源码 `b76ece3`。指定 worktree 为 `.worktree/implementation-reaudit`，HEAD `4ad5a2e`；只读 `git diff b76ece3 -- packages/engine/src` 为空，以下源码行号适用目标提交。仅新增本工作记录，没有产品/Git写操作或长测试。已读适用根AGENTS与principles（承接sweep06）。

连续全文阅读 `.omo/evidence/company-information-npc-intentions/task-14-review.md` **230行**、`task-15-review.md` **170行**、`task-16-review.md` **121行**，合计**521行**。读取包括Task14原REJECT与末尾Re-verification，不用首行APPROVE或旧勾选取代代码核对。输出超限部分另按连续行段补读完毕。

## 结论

无新增独立G编号。**G36再次被历史F-O1支持**：Task15历史复核把“只遍历active”当作过滤惰性冲击的依据，这在当前实现不成立，采样的所有公司事件本身都会进入active。**G35/G28/G09/G07、Q11维持原台账边界**，不能用Task14–16的模块APPROVE核销后续生产消费缺口。Task14 F1/F2/F-O2/F-O4与F-O3已分别找到登记/装配/注释/生产裁剪反证，不能重开。

## Task 14 逐章覆盖与条款状态

| 原文章节/行号 | 当前状态 | 当前代码证据与反证 |
|---|---|---|
| 首部最终裁决（1–20）、§一隔离重跑（24–36） | 历史证据保留，不代表当前验收 | 后文191行明确复验翻绿。历史773/775与clippy结果不重新宣称本轮运行。 |
| §二Q1 经营事实驱动会计（44） | 已有经营内核；生产遗漏仍G35 | `company/operations/day.rs:79` 先事件/到期再行业flow，`state.rs:15` 经济乘数与风险是整数派生；行业账套业务方法过账，无随机直接修改现金。旧经营链通过不等于工商折旧/所得税/商业债务支付调度已齐全，参照总账G35。 |
| Q1 跨行业适用经济字段（49） | 参数行业门控已有，公告过滤未闭环G36 | `day.rs:125` IndustryId相等才激活行业冲击；银行 `bank.rs:108` 消费信用风险。公司采样没有CompanyKind参数：`events.rs:210`、`day.rs:137`。 |
| Q1 RNG分流（58） | 已实现 | `company/rng.rs` OperatingRng与CompanyOperating/MarketShock/IndustryShock/InitHistory各流；`events.rs:51` 参数采样，`day.rs:102` 明确市场→行业→公司顺序；不消费证券策略RNG。 |
| Q1 持久due队列（64） | 已实现，不等于全部债务已调度 | `company/scheduler.rs` 按due_date/id存待办并拒绝重复/过期/跳过；`day.rs:82` 真正pop/dispatch。未覆盖的工商商业债务支付仍归G35。 |
| Q1 默认/压力参数版本化（67） | 已实现配置字段与持久化 | `events.rs:28` ShockParams、51行default_v1、67行独立stress_v1、82行validate；默认100/100/200bp、5–30日、500/1000/2000bp仍在，不虚称真实市场频率。 |
| Q1 八种最低事件目录（71） | 枚举已有；适用性缺口G36 | `events.rs:135` ProductionInterruption及137行AssetImpairmentSignal；210行公司六类抽样，不根据行业过滤。 |
| Q1 两年前财务前史/独立InitHistory/无历史证券成交（74） | 已有模块及后续装配 | `company/operations/history.rs` generate_history/finish_history；`information/prehistory.rs` 再按真实排期装配报告。经营前史与证券预置K不是一条随机成交链。 |
| Q1 同seed固定输入/违约无救助/股东动作不执行/休市经营（79） | 内核已有，现行范围保持 | `company/operations/dispatch.rs` PaymentFailed业务记录；scheduler拒绝ShareholderDistribution；`session.rs:2105` 自然日推进经营，无制造证券成交/投资者补钱。同seed承诺按现行实际受理输入范围解释。 |
| Q1 接缝与CivilClock恰好一次（86） | 后续已接生产 | `session.rs:2100` end_civil_day_after_session_check→2105 run_day_end→2110封账→2114披露；不再仅是历史Task14独立接缝。 |
| Q1 金样数字（90）、Q2最小范围（95）、Q3边界（104） | 历史验证与范围说明 | 本轮检查调用与守卫，不重新运行或篡改金样数字；模块拆分/fixture行数不是未实现功能。 |
| §三偏差表（113）：多文件、数据表、薄适配、前史as_of、先勾选 | 历史接受项/测试增强备注 | 前史as_of访问器建议不是新的已决产品能力；company flow使用受控日期/类型化业务错误。先勾选属于历史流程，不能证明生产闭环也不单列代码缺口。 |
| §四F1 节奏假设缺登记（126–147） | 已修复 | `docs/company-accounting.md:130`五项operations-pacing；`tests/fixtures/company-model/policy-sources.json:574`同ID；银行/保险/地产映射已有source_ids。139行“顺带shock-params”明确建议，不能升级必做。 |
| F2 seam多余mut（148） | 已修复源码 | `tests/company_operations/seam.rs:26` wiring.install(&mut clock, &ops)，不是&mut ops。未运行clippy。 |
| F-O1 银行/保险惰性激活不可叙述为经营事实（153） | **仍漏，既有G36** | `day.rs:144`直接activate、`state.rs:31`直接push，`disclosures.rs:113`遍历active，121行AnnouncedEvent::from_active直接进入公告；不存在CompanyKind可消费性过滤。Task15的旧“active不含惰性”结论被这条链反证。 |
| F-O2 0日期限配置brick（158） | 已修复装配 | `operations/config.rs` FlowParams::validate_durations拒绝<1；`operations/core.rs:191` live/history共用build校验。非法期限不会等到次日DueSkipped才发现。 |
| F-O3 mirrored只增不减（164） | **后续生产已修复** | `session/company_operations.rs:110` prune_dispatched按pending retain；`session.rs:2123`每次真实日结调用；`session/persistence.rs:1011`恢复要求mirror_is_exact。不仅有未接函数。 |
| F-O4 bank重估条件注释不符（167） | 已修复 | `operations/bank.rs:107` 注释明确激活当日及有效窗口；108/113行newly或credit_risk_add_bp>0与注释一致。 |
| §五原REJECT（172）、Re-verification对象/修复逐项/重跑/最终结论（191、200、213、222） | 有复验及当前反证，按已修复处理 | 不把前段原REJECT反复当新遗漏；F-O1仍G36、F-O3被后续生产裁剪核销。其余历史测试数不作为本轮结果。 |

## Task 15 逐章覆盖与条款状态

| 原文章节/行号 | 当前状态 | 当前代码证据与反证 |
|---|---|---|
| 首部对象/读过清单（1–17） | 历史范围说明 | 公布模块与21文件历史范围不等于现行全会话验收。 |
| AGENTS Q1 排期红线（25） | 已实现游戏排期，官方缺证保持诚实 | `information/schedule.rs` stable_company_offset/scheduled_instant基准日期、0..7、18:00、年/半年窗口、Q1严格晚于年报；`policy-sources.json:374` SSE季度材料blocked，不冒充已取证。 |
| Q1 披露时序与临时公告确认条款（33） | 自然日生产已有；惰性事件仍G36 | `session.rs:2110`先封账后2114披露；`disclosures.rs:117`发布结构化事件，只含terms；`publication.rs:288`校验当日18:00。该结构无“已实现”位，但不解决行业不适用事件。 |
| Q1/F-O1 “惰性事件无公告路径”（38）、探针F-O1（93） | **历史核销理由不成立：G36** | active过滤仅按starts_on，不按业务可消费性；见Task14 F-O1完整生产反证。它不是只有CompanyDayReport的无害日志。 |
| Q1 PublishedReport公司/范围/期间/政策/批准/公布/版本/来源/更正/五产物（39） | 结构与不可变库已实现；集团公开G28 | `publication.rs:108`内嵌ReportSet；`public_view.rs:72`先校验scope/time/origin、94行读定稿版本、107行ReportSet.validate；日终 `disclosures.rs:186`仍Standalone，不能核销G28集团交付。 |
| Q1 不可变查询、EarlyRead、公开更正不覆写（42–46） | 已实现库边界 | `information/queries.rs`读published_at；`public_view.rs:151`恢复检查、247行insert_report；公开更正新ID/supersedes。旧内容不变不等于当前预测读取全部新报告，后者仍G09/Q11。 |
| Q1 SeededPrehistory、1998/2000、比较诚实（47–53） | 前史装配已有 | `information/prehistory.rs`过滤instant<start_instant、SeededPrehistory；按排期公布，未来年报不提前混入。未重跑前史金样。 |
| Q1 correction与Task13跨期修复（54） | 已有更正内核/测试 | `tests/publications/correction_gold.rs`走closing.correct并保留旧版本；不推导生产更正公告自动触发直接重估，Q11仍是另一消费契约。 |
| Q2最小范围（58）、Q3拒绝与休市零市场副作用（66） | 核心边界已实现，有测试代码 | `public_view.rs:72`校验先行；`publication.rs:204/230/253/288/311`统一守卫，恢复共用；`tests/publications/failures/`覆盖对应拒绝，weekend_publish覆盖休市。 |
| 计划K4/验收探针（78–94） | 大部分已有，93行反证、集团与行业另列 | 排期/早读/不平/未定稿/公告时序/不可变/前史/固定seed等已有模块。只用APPROVE不能覆盖G36。 |
| 登记核查：report-schedule与blocked准则（96–108） | 已登记，资料blocked仍不能报verified | `policy-sources.json:268`cas-32 status blocked及summary仍注明原文未取；reporting-closing-simplifications承载游戏假设。这里是证据缺口，不凭审计删除合法模拟能力。 |
| 登记核查APPROVAL_HOUR=8（109） | 约定源码存在，registry补文案建议仍未闭合 | `publication.rs:27`常量；`disclosures.rs:181`、`prehistory.rs:158`产品构造使用。`policy-sources.json:509`排期文案含18:00但未见08:00；作为建议登记债务，不新增G。 |
| D1文件拆分（117）、D2封账移交（123） | 拆分非缺口，工商封账后续已接 | `session.rs:2110/2227/2232`真实close_month/close_year；**不能重新列“全部封账未接”**。非工商 `operations/config.rs:40` books_mut仍unreachable，属G36，工商处理遗漏另G35。 |
| D3裸fn相位观察者（128） | 状态性派发生产已在同report时点 | `disclosures.rs:31`空无状态hook本身不表示吞披露；实际2114日终run_day_end完成，2125通知observer。 |
| D4不可达排期守卫（132） | 正向数学与守卫已有 | 稳定偏移域与基准日保证窗口，拒绝域外偏移；不要求人为改常量制造产品失败。 |
| D5保险偶发失败（136）、D6 LOC（139）、D7 DTO移交（142） | 历史证据/范围备注 | 未复现历史失败不报成当前失败；宿主DTO与生成契约已存在，未在本分片运行全量。 |
| 隔离重跑（144）、QA失败（153） | 保留历史验证 | 815/0/4属于历史，不冒充当前。本轮静态验证守卫，不运行长验收。 |
| 非阻断O1算术笔误（158）、O2批准约定（160）、O3先insert再报重复（162） | 非新增产品缺口/建议债务 | `public_view.rs:254`仍先insert，方法私有；publish_closed分配新id，from_parts失败整体弃建，未找到可让调用者观察旧公开库被重复覆盖的路径。 |
| 结论（166） | 模块历史APPROVE，后续生产另核 | 不用该结论代替当前行业过滤、集团公开或已读中期消费。 |

## Task 16 逐章覆盖与条款状态

| 原文章节/行号 | 当前状态 | 当前代码证据与反证 |
|---|---|---|
| 对象/隔离/提交面（1–12） | 历史范围 | 接线原归25/26；当前应查root生产，而非重复认为模块尚未接。 |
| §一Q1 K4表：逐公司id/观察时间、候选≠阅读、未读拒绝（19–25） | 信息隔离内核已有 | `information/acquisition.rs:141`按owner/库时点/幂等次序记录；184行逐公司记录；`npc_view.rs:83/94`先require_acquired再按首次observed_at查询公开库。 |
| Q1 实际接受注意力才观察/休市不全体读/dev不写/更正不重写（26–29） | 机构生产已接，散户认识消费仍G07 | `session/decision_chain/roots.rs:349`仅本人候选做discovery，360行record_acquisition，404行构造NpcObservationContext；个人状态按账户take/install。自然日日结没有批量record_acquisition。三类NPC全部消费档案不能用机构root代替，仍按G07。 |
| Q1整数时间/金额/序列（31）、Q2最小范围（33）、Q3边界（35） | 纯信息域已有 | CivilInstant/PublicationId与BTreeMap稳定序；信息域不直接读未公开总账，不建立复制报告体的个人库存。 |
| §二Acceptance未披露/已披露未读不影响、对照、逐人延迟、重复、dev（41–45） | 已有信息隔离测试；生产预测仍核G09 | `tests/information_acquisition/view_gold.rs`与acquisition_gold.rs已锁个人输入不泄漏；root349行在真实本人候选时获知。获知成功不等于季度/半年预测已更新，根new_annual_reports在344/370/394行只收Annual，G09不能核销。 |
| Context四输入/零CompanyState/全引用（47）、只存id+时点（49） | 已实现 | `npc_view.rs:41` owner/state/library/market，83行返回引用；`acquisition.rs:164`记录id/time/kind。 |
| QA：未来报告/公告、早于公布、未知ID、跨NPC、存档篡改（51–57） | 已实现守卫与现有测试 | `acquisition.rs:148`owner、154行resolve公开库时点、211行恢复严格递增/全局ID唯一；`tests/information_acquisition/failures.rs:31/55/100/119/146`明确typed断言。 |
| 接线范围/LOC/无浮点（59–65） | 历史模块说明，生产已演进 | current roots显式接线，不因旧“零session改动”重复开接线缺口。 |
| §三登记#1幂等/#2签名/#3QA澄清/#4错误PartialEq（71–75） | 接受项保持，非缺口 | 154行先库查询再156行dedup；组合“重复但时点早于公布”仍EarlyRead，顺序正确且不静默吸收不可能输入。 |
| 登记#5库外ID读取时拒绝/#6Market与接线/#7候选无权重（76–78） | 边界与后续接线已有 | `npc_view.rs:85`透传库错误；候选纯id投影不是偷读；生产attention负责发现，本模块不必重复权重模型。 |
| 登记#8编码事故/#9批准约定建议（79–81） | 历史事故不重开，建议债务如Task15 | 当前模块/测试正常UTF-8源码可读；fixture硬编码合法8时不能认定漏功能。 |
| §四证据对账（87）、§五执行记录（102） | 历史13/828计数保留 | 不宣称本轮复跑；不存在必须重现历史Windows冷编译时长的需求。 |
| §六O1重复+早读组合（115）、O2多公司金样（116）、O3批准文案（117） | 测试增强/文案建议，未升级新G | 底层顺序明确；多公司owner/ID数据结构及生产roots循环已存在。组合case/多公司专用金样是审查建议，不把“无专用测试”直接写“功能未实现”。 |
| §七结论（119） | 历史模块APPROVE可保留 | 完整当前消费者遗漏由G07/G09/Q11追踪，不能泛化成整个公共信息隔离失败。 |

## 新候选的反证与最终归属

1. **“Task15已解决惰性公告”反证成立，归G36。** `events.rs:210`无CompanyKind，`day.rs:144`→`state.rs:31`进入active→`disclosures.rs:113/121`公布；starts_on过滤不是行业适用过滤。不重复建号，不把获批只记录的CreditDeterioration混成此缺口。
2. **“mirrored只增不减”排除。** 已有prune_dispatched且session.rs:2123真实调用，恢复还严格验证pending集合。
3. **“月年封账完全未实现”排除。** session.rs:2110/2227/2232工商生产路径已有；非工商封账仍G36，集团范围仍G28，工商经营项目遗漏仍G35。
4. **“批准日08:00未写registry属于漏实现”不升级。** 常量与真实产品使用均存在；历史反复明确为建议fold-in，仍记录文案债务。没有新增交易所批准钟点规则。
5. **“重复insert破坏公开版本不可变”排除当前可达产品风险。** 私有insert仅新id发布和整体弃建恢复使用，未发现用户可持有部分失败新库；保留风格建议，不凭私有方法的局部变更宣称线上覆写。
6. **“未取得CAS32/季度官方文本应认全部未实现”排除。** registry保持blocked，游戏简化已登记。它是官方依据证据缺口，不等于可运行模拟代码缺失，也不能写成verified。
7. **“全部NPC本人阅读已完成”不能核销G07/G09。** 机构root已走owner/实际候选获知；散户档案消费和已读中期预测仍以总账为准；更正直接重估触发边界仍Q11，不要求所有公告一律估值。

本轮没有运行测试。现有测试位置仅作为边界实现证据，历史pass计数仅为历史，不是本轮验证结果。本文件提交给父agent汇总与完整diff独立复核。
