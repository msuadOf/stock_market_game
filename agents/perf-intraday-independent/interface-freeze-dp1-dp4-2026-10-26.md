# #122 DP1–DP4 最小接口增量（2026-10-26，待独立复审）

> 只补已批派工的接口与失败时序；**未授权E2/E3/H1/P1实现**，不修改原合同或派工冻结61d114。
> I-D已正式APPROVED：生产 `c8e9a4f51dfb9021cf95b6da8f7f52cfbcf20957`，证据
> `3984e14167202dc645ec340fbd2a73e661c71c8a`；依据 `plan-identity-combined-review-2026-10-26.md`。
> E1已在`.worktree/runtime-sync-e1`从3984建立独占writer，仅纯覆盖表/新短测，不注册生产模块。
> 首轮130行稿76b162已按原字节保存在`interface-freeze-dp1-dp4-before-review-2026-10-26.md`。
> 本次仅修`interface-freeze-review-2026-10-26.md`的I1/I2及提交/移交必验条件；DP1/DP2原字节不变。

## 2026-10-29 受限失败政策修订

本次明确收窄原 DP4 F2/F3 的无条件 terminal 政策，承接已批准 Host 的更正重试方向；
原冻结、旧准字及原 FAIL 只作历史，不因本修订改写。仅真实更正 prepare 返回 typed
`ReportCorrectionDayEnd`，其 `CompanyErrorClass` 为 `InvalidInput` 或 `BusinessCondition`，
civil 候选未提交且尚无首fatal，
可保已装市场、retained、旧存档候选，允许取消坏更正后 civil-only 重试。不新增错误变体。无额外市场 `Start`、无重跑收盘、无 checkpoint 回退。

只在真实 prepare 调用点设置局部来源标记；后续 project callback 的同型错误不继承该标记。
`UnsupportedOperation`、`SystemState`、INFO／输出／内部不变量和已提交 civil 后的失败仍
terminal。两类必须显式穷举，不按 `!= SystemState` 放宽。完整边界同步见
[convergence-contract.md](convergence-contract.md) 的 2026-10-29 受限修订与 §2。

未安装实际市场收盘 Full 的首个 Closed 日保留原有可逆 callback 拒绝，不新增按错误外形终止的政策；
SystemState 或已 terminal 的 World 仍按原健康门封闭，不得由本例外复活。

## 实际前置与接手范围

- D1当前冻结生产 `35158546bd3863c63148b1e7f3470de38c326f3f`，证据tip `995eeb339da0b0b8ecdd4b1f5f6049696ef6db9a`。
  现`runtime-diagnostics-d1-review`终裁仅限最小D1代码；不是D2–D5/整feature通过，也不允许单独合main。
  旧派工中的dirty快照只作历史。本增量读冻结D1的prepare/take/publish，不移植其旧rollback/discard语义。
- SRC-A/SRC-B是已批设计、尚无生产实现；`CompanyReadFacts`由INFO-P提供，不另建同义类型。
  INFO-P当前`.worktree/information-prepare-p@62ba8666`加dirty仍未终审，中央drive/failure写窗仍由INFO
  及Leader串行调度；本页不给#122新writer提前取写权。按INFO最终可信包重新绑定方法，不借设计批准冒充实现。
- E2精确新增接缝白名单：`session/protocol/commit.rs`（只读投影adapter）、`session/protocol/delta.rs`
  与`snapshot.rs`（进度/镜像来源及phase）、D1的`session/causal.rs`（raw批保留与纯prepare），
  `runtime/drive/convergence.rs`/`session/runtime_mirror.rs`（新）。E3另按Leader交接接手memberships/
  shared_ingress与World只读来源getter/非tick提交游标窄hunk；不得覆盖INFO/Admin协议或重写D1sideband。
- I1/I2必需的`GroupIn/UpMsg`、`World::collect_personal_full`、组端只读full handler、legacy信息
  adapter及无Drain清理接缝，均先由INFO原writer交出精确hunk，再串行交给同一engine writer。
  这是待审接口增量，未取得INFO最终包/Leader交窗前不得实施；不授予整份group/protocol/world写权。

## DP1：最后callback读私有candidate；sideband失败保原件

现`protocol/commit.rs:9–40`硬借`&GameSession`再读runtime_snapshot；D1 `failure.rs:626–674`
先安装再callback。目标采用**同一投影结构的两个构造入口**，不另建交易执行器：

- 保留serial入口 `TickCandidateProjection::from_session(&GameSession, seq_from)`；新增内部
  `from_prepared(&PreparedTickProjection)`。后者只借私有字段，不持live session、不临时swap。
- `PreparedTickProjection`准备并持有：`progress={tick,seq_from,seq_to,day,phase,civil}`、
  `TickTimeseriesPayload`（当前已提交World/事实来源）、`MirrorSnapshot`及working orders
  （本次candidate或上次完整镜像的同源投影，带DP3来源）。两入口复用纯frame/runtime构造及验证。
  公共runtime不再调用live `player_working_orders()`补读；最新timeseries也不能借滞后mirror填造。
- `MirrorCandidate`仅持待替换域、未变域Arc/借用句柄、来源与可移动结果；账户/个人态/attention/
  plans/market/K/resting/mirror/派生reservation构造、编号及所有验证先完成。普通frame最后callback
  读取该Prepared输入、校验frame/runtime/游标，并预备全部输出；成功后唯一`install_prepared`
  仅移动已验证值并更新来源/清覆盖表，不返回可恢复Err。禁止clone整GameSession/World或先装后undo。
- plain：prepare mirror＋诊断delta → 验证全部输出 → swap（若触发）→ publish causal一次 → 返回。
  frame：同prepare → `project(TickCandidateProjection::from_prepared, facts)`成功 → swap →
  publish causal一次 → 安装protocol已验证输出。延迟档本拍无swap时仍每拍完成frame/事实/诊断提交；
  诊断seq只在最终publish推进，不受mirror是否安装影响。任何普通拍晚失败均terminal，不重跑World。
- D1消费改为**先move完整raw batch进准备对象，再借raw验证/翻译**。概念返回
  `Result<PreparedCausal { raw, delta }, CausalPrepareFailure { first_fatal, raw }>`；错误携原FactTime、
  tick/round/stock/owner/order/Unsupported来源与未交付范围。callback失败时同样把raw移动到
  terminal诊断槽；不回放、不重投、不按新ordinal重排，不覆盖已有collector或上一成功事实。
  成功publish后raw可释放，report_log不另永久留副本；失败诊断只保该终止批，不能变成无界历史。

**验收加到T03/T15**：分别注入diagnostic prepare、最后callback失败，swap=0、全部镜像/来源/
覆盖表不变、collector不追加、World已发布tick不退、raw完整可诊断且后续入口同首错；成功plain/
frame恰一次追加。后续civil失败不删前拍已追加collector。保D1原支持范围及featureoff零载体。

## DP2：receipt立即登记；OnDemand在冻结消费后、资源读取前flush

冻结两个阶段，**不改变enqueue的同步返回承诺**：

1. shared及direct enqueue先健康/只读policy门，再原身份校验与真实receipt登记；DayEnd拒绝必须
   在next_input/ordinal/pending任何改变前。共享句柄保存不可热切换policy及terminal首错投影，
   已持有旧句柄也返回该首错，而不是继续入队或只给无上下文closed fallback。
2. 可变session消费顺序：`require_healthy → freeze_shared_ingress(cutoff) → 保留冻结pending →
   若OnDemand本入口将读运行账户则flush_candidate(ComputeRead) → take/route ExternalPlan → World`。
   同一cutoff内多个请求只需一次最新版本flush；cutoff后到达者按真实receipt留下一拍，不能等待flush
   后重新登记或令来源类别排优先。直接入口无共享binding时沿同样pending消费门。
3. 已登记receipt而flush失败：保输入/原ordinal及诊断，terminal、关receiver，**不发ExternalPlan**；
   不回退ordinal，不把请求说成没收到、不重试。业务余额不足仍由原资源校验/事件面的精确
   InsufficientFunds表达，不虚构shared enqueue当场返回余额错误。计算读取失败不能按旧mirror继续。

**T07/T08补断点**：共享/direct下单撤单DayEnd零receipt变化；OnDemand两个请求跨flush/cutoff到达，
原receive顺序与tick归属保持；flush失败前已登记请求原序保留、无route，旧shared句柄同首错。

## DP3：完整join移交；公开mirror phase与私有CompanyReadFacts分开

- **公开字段**：Snapshot/PublicRuntimeState/RuntimeDelta/Refresh保现`tick/seq/day/phase`为最新
  运行进度；新增统一`mirror_origin`，至少含`authority_epoch, install_sequence, source_tick,
  source_generation, day_end_stage, non_tick_revision, mirror_phase`。账户/行情/working orders/
  reservation/desk镜像作为一个读点使用mirror_phase与原可见截点；不得用最新self.phase重滤旧簿。
  TickFrame的tick/seq连续及timeseries为当前真实提交事实，另与旧mirror来源区分。
- `authority_epoch`为每会话driver成功替换的checked单调身份（新会话另由既有host generation隔离）；
  `install_sequence`仅成功mirror swap推进。外部新鲜度按(epoch,install_sequence)拒旧；内部构建
  还须核source与当前authority相等。新Admin同tick可以只增non_tick_revision而不造PublishedTick。
  显式flush不造PriceTick/seq：无新市场tick时走Refresh/baseline交付同进度新来源，不造空TickFrame。
  RuntimeDelta可以进度前进且mirror_origin不变；Rust/host/Web严格字段/验证器须同批接线，不补旧格式。
- **join准备**：先在旧名册/旧driver完整且静止时捕获全账户含cash+purse/在途/成本/T+1、全部
  PlanBook/OwnerSequence/terminal、belief/attention/个人经历、resting/serial、未消费事实/receipt与
  dirty输入来源；这些进入一次`PreparedHandover`，不能从旧session镜像补齐或只flush账户。
  捕获后保持旧driver静止；先验证新名册与分片、构造等待激活的新driver和新镜像（可失败，
  不发布），并预计算`authority_epoch.checked_add(1)`、`install_sequence.checked_add(1)`及
  其它全部checked游标、source与公共输出。耗尽/校验失败须在receiver新Player注册前拒绝。
  receiver注册是最后可失败的业务操作；随后只move已验证成员/账户/镜像/driver与预计算游标，
  不再spawn、分配receipt、调用fallible getter或重建投影。期间新到请求只由原receiver登记，
  cutoff/consumed/input序列不重置，不在两个driver上分别route。
  注册失败先显式清理全部prepared workers，成功才报告保旧权威可继续；清理失败保旧权威但
  旧会话也terminal，首错连清理详情保留。注册成功后旧worker退役失败则新状态terminal，
  不能回滚receiver或复活旧driver。
  必须新增窄`stop_without_drain(self) -> Result<StopReport, StopFailure>`（命名可同义）：
  置既有永久shutdown标记、停止下行、耗尽分类诊断并逐一join全部已spawn句柄，报告send/
  worker/panic/join与未交付范围；不Drain、不Abort业务、不安装Final、不发布或清算。
  partially-built spawn guard与完整PreparedDriver的注册失败清理复用同一接缝，原始错误和
  清理错误同时保留；不能依赖最终World尚未形成时的Drop。现`World::shutdown()`会Drain，
  `Drop`忽略join结果，两者均不能充当该可报告接口。仅改handover新建/退役对象，不重写常态协议。
  旧布局dirty表不回灌新(group,local)；必要事实先按AccountId解释，成功handover后重建空覆盖基线。
- **私有计算来源**：INFO-P的`Arc<CompanyReadFacts>`只含issuer/kind/u64股本，仍由所选模型与INFO
  同事务提供给SRC-A消费者；不塞进mirror_origin、公开Snapshot或存档新权威。不从session旧metadata
  回写组。SRC-A未实现时不预接该hook；待其最终可信接口只接窄getter，不等待SRC-B/R4整链。

### I1：新增无dirty门控的完整个人读取

现`World::collect_group_belief()`发`EchoBelief`，组端只回dirty/pending的Some且take pending；
它明确不是Full。新增内部只读`collect_personal_full()`，使用窄`EchoPersonalFull`/
`GroupPersonalFull { group, source, slots }`（名称为待实现契约），DP3与DP4共用，不加第二份常驻个人权威。

- `slots`按本组**真实**名册每人恰一条`(local, AccountId, Option<BeliefParticipantSnapshot>)`；
  padding不进真实名册，不能用None冒充缺员。全部组恰一回执，source同静点；按group身份收齐/
  查重后规范排序，mpsc实际arrival不要求组号递增。缺组/重复/组内local乱序/错owner/local/
  缺员/多员/非法Some或None均显式失败，swap=0并terminal。
- 构造/restore时从实际`runtime_personal_states()`能力集合及实际strategy、belief participant、
  npc_attention存在关系生成并验证`PersonalPresence`名册契约：先核真实账户strategy的
  `retail_style().is_some() || belief_chain_params().is_some()`能力及实际名册；具此能力者必须
  有participant/attention及Some seed，缺行不得因过滤后为空被宽免。不能拿组NpcProfile为Player
  填充的baseline Retail当实际策略能力。当前真实StrategyState仅按现封闭变体验证，不补造新变体。
  已验证真实seed为Some者为Required；
  无strategy（含Player/IssuerRepurchase）或确实没有belief participant者为明确Absent，并记录
  对应原因。Hot/Inst的kind本身不意味着Required；有strategy+belief participant却缺attention
  是不完整构造输入，不能分类为Absent。Required缺Some、Absent出现Some均报错；合法能力变化
  只可经已验证名册/个人能力移交同时变更presence与值，不能在Full读取时临时放宽。
- snapshot保七个现存字段：watchlist/price_memory/information/belief/attention/history_reads/
  retail_experience；核information.owner与belief.npc为该AccountId。retail_experience的Some/None
  按实际seed/能力保存，非retail合法None，不能补造经验行或假定所有retail空历史都有Some。
- 读取不take/清dirty或pending，不执行observe/delivery，不推进个人/计划/事实序号，不改变权威。
  candidate还从同静点的既有全量`collect_group_attention()`读取真实profile的next_due与seed
  （保持现组因子逆变换），按真实owner校验；合并时**这两个调度/RNG字段优先于个人snapshot
  内同名镜像字段**，cadence/check与其它个人字段取组个人权威。无个人能力的真实账户保None，
  其profile参与情况按构造契约验证；padding不得生成调度或个人owner。

**T09/T12/T14补断点**：竞价mirror→连续进度仍按mirror旧phase可见性；同tick新来源可Refresh、旧来源
拒绝；真实多组尾组join带非零在途/child/成本/attention/高计划游标，迁移前后守恒，旧dirty布局不重投；
早拍已改但当前不dirty者仍在Full，合法None与缺员分开，重复读取相等且dirty/pending不变；上述
完整性负控均拒绝，至少两种合法组arrival顺序产生同一Full。游标耗尽先于注册；部分spawn失败、注册失败都回收所有新worker且保旧状态；
注册失败的清理再失败显错并terminal；注册后退役异常全部join且新状态terminal；全程无Drain/发布。
公共DTO往返不包含私有CompanyReadFacts。

## DP4：非tick提交游标＋按域dirty；retained发布早于swap也不可撤

现D1a `world/admin_transaction.rs:220–229` commit后不发新tick/generation，AdminTransactionId又含
失败尝试，不能仅凭最后attempt id或普通generation判断镜像是否最新。选择一个**World内部专用
`non_tick_revision: u64`＋固定`PendingMirrorDomains`位集合**，不新增业务PublishedTick/共享锁：

- 下表穷举当前生产可写入口；任何新增投影writer必须在接入前登记写集和source，否则入口显错，
  不能静默沿用无变化。revision checked_add在首个不可撤commit/广播前完成；可能无变化的入口可
  先纯prepare判断写集，再checked，之后commit。全部成功才安装revision/OR域；业务prepare/
  inspect拒绝不增不清，部分commit/transport/panic直接terminal，保最后成功revision和失败attempt。
- AccountsFull读取现有完整账户源；PersonalFull必须走上面新增I1接口，不能用EchoBelief补旧镜像；
  AttentionFull读取全量profile调度/RNG源再按上述字段规则合并；PlansPending沿I-D完整簿。
  私有current facts/library由INFO同commit安装、同revision绑定，不进入公共mirror权威。即使
  personals为空，只要library/facts/observed-through改变也增revision并置PersonalFull；只有这些
  以及账户/个人值全都未变才是no-op，不以空personals或Arc指针相等冒充无变更。
- driver构建前捕获revision/dirty掩码，与tick/generation/day_end_stage合为内部来源戳；其变化
  禁止no-op，戳还含authority_epoch与civil_date。全部prepare/诊断/最后callback完成后、唯一swap
  **之前**再次核对该戳与当前权威相等，并预计算install_sequence；失配/耗尽必须swap=0，terminal，
  不清原因。校验到无失败安装之间持同一独占静点，不允许回调/worker再改源；共享enqueue仅登记。
  成功swap后exact ack本批捕获域并清覆盖输入，没有“先装旧candidate、事后只是不ack”的合法路径。
  连续两次同tickAdmin或INFO均能再次刷新；不复用公开event seq作该游标。

### I2：信息字段迁交与有限入口表

源码现状：I-D的`RuntimeDriver::{deliver_information,deliver_information_for_test}`经
`collect_delivery`从session旧attention镜像组载荷；组只写information/belief，返回attention后
`set_runtime_information_checks`只在session写cadence/check。新INFO则直接读写组personal.attention。
静态全src检索只有`drive_tests.rs`调用前一native公开方法，后者为强制测试面；这不取消现有API支持。

**选定迁交**：从本批首次driver构造/restore起，cadence/check随现有个人seed一次迁交组，与
information/belief同writer；session保只读镜像。旧实现的session双字段writer不再在runtime信息
入口调用；serial分支保其现有权威。当前不存在运行中更换代码协议；若fixture显式以旧布局driver
做移交，必须在静点先以旧session的这两个唯一权威字段加组当前其余个人态组成PreparedHandover，
验证后一次迁交，之后不再从session补值。join在新epoch只捕获组当前值，不能再次套旧session。

legacy native方法保签名，由薄adapter转同一World信息作用域：从当前组读个人cadence/check/
information/belief，市场/逐股交易日及私有facts按INFO最终可信输入类型准备，禁止旧session attention
payload；版本快路径仅在完整观察签名也已证明相同时才可零回写，library version相等本身不能
跳过当前codes/facts/market/资格完整校验。强制测试方法只绕优化，仍过健康/source/身份/时序/
完整上下文及同签名去重门。不得直接
发送旧的非事务`InformationDelivery`并逐组写一半。组在同prepared commit内写返回information/
belief/attention，组profile调度/RNG不被旧payload覆盖；session只在唯一flush投影安装时更新。
INFO与legacy共享同一可变World作用域、观察水位及有限去重判断：保最后成功观察签名（through、
library内容/版本、facts、market/逐股日上下文、实际Required owner集合及每owner关注codes），
不建永久观察历史。关注codes在pure prepare中按**当前working**在手或pending_sell非零股∪
watchlist∪active plans精确规范生成；不是只比较owner数量/标签。比较真实输入值或已验证不可变
输入身份，不以裸hash碰撞或Arc地址当等价。签名不含本次结果的information/belief/check/
observed-through新值、own non_tick_revision或due布尔；本观察的回写不会让自己下一次变成新输入。
同签名不再次运行本人计算；同through的Admin新持仓/关注集合变化必成新签名，先验上下文仍全量
校验，再按组当前cadence/check判due，不因去重缓存漏合法观察。None仅更新库/facts，不能占据
Some观察的签名；更早观察显错，同through新材料按当前
个人获知记录/检查基线处理，不能重置check或伪造新获知；失败不推进版本/水位。Immediate的
next_after(now)=now，不能仅凭next_information_check相等宣称已去重。此处只共用已有批准的
观察机制，不接生产R4；最终INFO类型绑定后，签名优先借其已验证不可变输入，不新clone整市场。
最后成功观察签名/observed-through、当前library/facts与来源也必须进入PreparedHandover；join
不得reset后令Immediate同through重跑。restore按既有持久个人check/获知事实建立合法初始状态，
不额外持久化签名历史；短fixture须验证换epoch仍保同through去重及None→Some观察资格。

| 真实入口 / 调用面 | 计算前权威与实际写集 | source/revision 与失败边界 |
|---|---|---|
| `World::with_admin_transaction` / civil的四种AdminOperation | 组账户working；CashDividend/WithholdingTax/ShareSplit/ShareReduction仅现金、股数及对应成本域，不改个人或计划 | 纯prepare/inspect后、首个Commit前checked；实际账户变更成功置AccountsFull；业务拒绝仅撤overlay，通信/部分commit/panic terminal |
| `World::with_information_transaction` / INFO薄适配 | 组当前完整个人态＋working账户、载荷library/current_facts/可选观察；同事务可含Admin；写个人、library/facts/through，附带Admin时还写账户 | 首Commit前checked；成功置PersonalFull，实际调度/RNG变化另置AttentionFull，附带账户变更另置AccountsFull；None观察但库/facts改变也计revision；失败依Admin阶段规则 |
| `RuntimeDriver::deliver_information`及`deliver_information_for_test` → 上述World信息作用域 | 迁交后的组个人态计算，版本仅为完整同签名已证后的优化；force_test绕优化但不绕校验/去重；adapter不读session attention、不调用session双字段writer | 与INFO同checked/commit及revision，成功置PersonalFull；旧非事务World入口在runtime活跃时不可绕行（内部化或显式拒绝），失败不更新last_library_version，部分失败terminal |
| `World::run_ticks` / ExternalPlan消费、普通组决策及实体/个人/计划演进 | 已批tick协议及三张覆盖表/PlansPending，逐拍事实仍交付 | 由已提交tick/generation覆盖，不另造non_tick_revision；失败按原合同terminal与已发布边界 |
| `World::run_day_end_public` / retained发布 | 日结清簿/回流/T+1/市场及retained | 由day_end_stage/generation覆盖，含retained先发布swap0的F1；不可因revision未变撤retained |
| `set_civil_date`与session自然日进度 | 仅输入clock/后续snapshot来源，非个人或账户提交 | civil_date入内部来源戳；clock变化不能套用旧candidate，不另增业务tick/seq |
| Full/sparse getter、save、callback、stop_without_drain | 只读或终止回收，不写业务权威 | 不增revision；sparse自身pending消费依E1既有输入接管，I1 Full绝不消费；stop的Final仅诊断 |
| cfg(test)的ForceDue/DeferDue、InstallPersonal/MutateBelief/MutatePlan等直接注入 | 明确测试专用，无生产调用权 | 下拍pending注入归原tick source；静点立即写入者须同步revision＋对应AttentionFull/PersonalFull/PlansPending并使旧candidate失效；不准用测试注入绕source验收 |

| 故障阶段 | 最小处理（全保已发布普通tick） |
|---|---|
| F0 World日结尚未发布retained、未swap、有效Abort窗仍开 | 可只撤本次未提交日结参与者；丢私有candidate；本次失败终止，不能撤普通tick |
| **F1 retained已publish，但Full candidate失败，swap=0** | **保已发布retained、相关市场日结/广播基，不AbortDayEnd/不restore_mirror**；旧session镜像保其原来源并封健康读，原始市场事实/未交付范围保诊断，terminal |
| F2 Full swap成功后civil/Info/输出准备失败（本页受限更正prepare例外除外） | 保已安装市场与所有已发布retained；只丢失败civil/私有overlay，不换旧档，不回checkpoint/旧driver，terminal |
| F3 独立Admin prepare/inspect业务拒绝 | 沿D1a仅撤本overlay；revision/待flush原因/已装mirror不变；若属于F2整批civil失败，除本页明确受限更正prepare拒绝外，外层仍terminal |
| F4 成功civil候选后宿主持久写入失败 | 保成功市场/civil/候选，显式存储错误及旧有效磁盘档；不撤成功状态 |

**T10/T12/T13补断点**：F1单独注入，assert swap=0但retained/日结来源不退；F2保swap新态；同tick连续
两Admin与一次INFO各改不同域，普通tick/generation不变仍三次被识别；失败不清原dirty、无第二次个人
输入执行或重复事实。成功Admin后关闭旧DayEndAbort的原边界不动，禁旧Abort/Final作业务重发布。
新增I2短fixture：组与旧session的cadence/check/认识字段刻意不同，legacy须消费组当前值；随后
Full不得退check/丢获知，下一次同签名legacy/INFO含Immediate也不得重复本人计算；None→Some
同through仍必须观察；Immediate下INFO成功后完全重入不因自身revision/check回写再跑，Admin新增
持仓/移除关注股后同through新签名不得误跳；改材料的同through依当前检查基线处理。Admin→legacy→INFO在同tick
依序成功且精确revision/域；prepare业务拒绝、partial commit、未知直接入口分别断言不清旧dirty、
无假版本推进、规定terminal。最后callback后注入source变化须swap=0，不能以不ack掩盖旧值安装。

## 本轮门禁与派工接续

本页供原未实施contract_final_review逐DP复审，不以作者确认代替批准。原E1只负责稳定覆盖表/
独立source_seq/纯Full原因，不实现本页epoch/Admin/DTO/join。E2/E3必须先取得本页可信裁决及INFO
最终中央交窗，由同一engine writer按精确新基线接线；不能以E1绿直接扩大白名单。默认EveryTick、
save3018目标日守卫、真正完整多组和普通10秒/长300秒边界继续保持，不改变交易制度。
