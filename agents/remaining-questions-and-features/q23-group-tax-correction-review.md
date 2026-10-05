# Q23 固定单层集团重述独立复核

复核日期：2026-10-05。复核者未实施本批生产代码。仅进行静态审查及读取既有红灯日志，
未运行 Cargo、未修改生产代码、Cargo 配置、Git index 或提交。

## 范围与结论

已完整阅读本批 `consolidation/{mod,aggregate,eliminate,minority,sale}.rs`、
`reports/{mod,consolidated_window}.rs`、`session/company_groups.rs`、
`session/company_groups/corrections.rs`、`tests/consolidated_restatement.rs` 的当前源码及
已跟踪文件 diff；`closing/mod.rs` 仅评审新增 `latest_reports_for_scope`。另外核对
`reports/{balance_sheet,notes,window}.rs` 与 `information/public_view.rs` 的消费契约。
共享树有其他作者改动，本记录不代表整个工作树或父单体税务实现的复核结论。

**本批静态实现方向合理，尚不能宣称 Q23 完成。** 当前未发现成员有效期读投影中的
新确定性金额错误；文档、测试绿灯及跨层恢复口径仍有下述待解决项。父任务尚未安装的
`GameSession` 外层候选事务明确属于待交付接线，不把 helper 单独调用当作已满足原子性。

## 大 A 语义与依据

- 依据沿用 `docs/company-accounting.md` 及任务方案登记的财政部财会〔2014〕10号、
  CAS 33 已核全文。逐成员合并及少数股东损益／权益拆分符合已登记范围；不重新声称
  本复核取得了新的官方正文。
- CAS 18／28 全文仍不完整；有效期重述、实际现金双口径及税级联继续属于登记的
  游戏简化，不得称作完整准则合规或法定追补申报。
- `MemberId → BusinessEventId` 借用映射避免公司本地 source 冲突；未知成员、未知
  source、非历史有效期均明确拒绝。Ledger projection 只选择有效期不晚于 cutoff 的
  已存在 Journal，不造凭证、不改日期/source、不触发实际付款守卫。
- 原 Journal 进入 Accumulator，CF 按实际期间、利润和权益按有效期间，且 mapped
  标志进入显式 `restated_cash_correction`；母子公司少数份额分别从有效期扫描计算。
- CIT report keys、ReportClassification、BS 正负方向重分类和 Notes 使用同一成员
  namespace；不会把一个成员的 CurrentTaxAssets 与另一成员的 TaxesPayable 净额相消。
  合并算法内部仍用原科目，但税务科目没有 worksheet 抵销，最终报表的分成员键未丢失。
- 固定单层、严格多数、整数基点、未售批次等既有拒绝边界没有借此次更正扩大；
  内部交易 request 仍按真实截止期派生。上年内部 worksheet 不追溯仍是既有 D3 简化。

## 必要性与复杂度

Books 只读接口收敛为 Ledger 加期间覆盖，使正常 consolidate 和历史读投影共用
同一抵销／少数算法，是需求必要变化；没有平行业务引擎、集团持久重述映射、compat
或多层扩展。私有 Original 追加新 sequence、公开 Correction 保留旧版本及旧 ID、
正常 scheduled 选择最后 sequence 的职责也属必要范围。

helper 会修改传入 ClosingEngine／PublicLibrary 后再返回错误，因此必须且只能在父层
whole candidate 内调用。不得在权威状态上先安装成员更正，再调用本 helper。

## 阻断与待补证

1. **正式文档已与代码漂移。** `docs/company-accounting.md` §2.7 D5 和 §8 仍声明
   Consolidated 重述不支持。交付前必须改为“固定单层工商成员组合更正支持；旧无 owner
   adjustments 的 Consolidated 请求仍拒绝；结构化子账及原简化边界不支持”，并注明
   这是已有简化模型上的工程能力，不改变 CAS 18／28 来源状态。
2. **绿灯尚未取证。** 已读取 `.tmp/checklist-wave4/group-restatement-red.log`，原业务
   case 真实失败于 `ConsolidatedRestatementUnsupported`。当前三个 case 分别覆盖
   同 source 不同有效期、历史投影不重跑付款守卫、非法成员/source/期间；本复核未看到
   它们的绿色执行日志，不能用静态代码代替测试通过声明。
3. **恢复 coverage 的条件性跨层风险。** `session/persistence.rs:1264` 调用
   `validate_groups`，后者使用普通 consolidate 的全部实际 Journal periods；重述窗口
   则排除 mapped entries 的实际期间。若一名成员的更正为其新增 actual 月、其他成员
   当月尚无非更正 Journal，则合法重述读投影可以通过，恢复却会因覆盖不一致拒绝。
   这是源码可见的两套判定差异，尚未运行 end-to-end 复现；父入口若不允许此状态，应以
   明确契约和测试固定。若允许，则恢复须使用同一真实非 mapped 覆盖事实，并先验证映射，
   不得补造普通 Journal、放宽所有 coverage 或静默忽略非法映射。
4. **version helper 的短集成门禁未覆盖。** 需验证公开报告连续两次 Correction 的
   sequence／supersedes／root ID、旧报告逐字节不变；私有 Original 重建后正常 scheduled
   发布末 sequence；未变化报告不新增版本；最后一个集团 publication 失败使成员、
   ClosingEngine、PublicLibrary 整体回滚，修复后同 source 可重试。
5. **领域边界验收仍应由父任务补齐。** 三个纯读测试未覆盖 80% 子公司税后差额／DTA
   手算、母公司更正少数影响为零、成员一侧税资产另一侧税负债、税跨年级联、真实内部
   销售抵销保留及严格 save→restore→下一次普通集团生成。双映射歧义拒绝和真实缺期
   coverage 拒绝也应至少各有一个短 case。

上述项修复或补证后，应再由非作者复核最终 diff；本记录不授权报告完整交付。

## 增量复核：正式文档与恢复 coverage 修复

2026-10-05 再次静态核对 `docs/company-accounting.md` §2.7 D5／§8、
`company_groups.rs::validate_groups` 及其 new／seed／persistence 三处调用；未执行 Cargo。

- **原发现 1 已修复。** 正式文档现在说明固定单层成员命名空间、真实 Journal、
  有效期／实际 CF 双口径、公开 Correction／私有 Original 与外层候选契约，并明确
  不能以生成器存在代替完整验收。没有把 CAS 18／28 全文缺口改成全面准则合规。
- **原发现 3 的源码差异已修复，集成证明待验。** init／seed 传 `None`，保留原始
  Journal 覆盖要求；保存恢复传 `Some(savedclosing)`，先读取成员 Standalone maps，
  工商 tax maps 必须一致，逐 source 检查真实来源和历史有效期，再以非 mapped 的
  actual periods 检查成员覆盖。全 Ledger 仍是当前真实 Books 的 Ledger，没有把
  有效历史期伪造为真实活动月，没有增加付款或现金。对当前完整状态进行抵销时使用全
  Ledger 合理，历史报告另由 cutoff projection 负责，两者职责不混淆。
- **范围必要且最小。** `Option<&ClosingEngine>` 区分无重述事实的新局校验与有重述
  事实的恢复校验，不是跳过集团检查的开关；两个分支仍走同一集团图、科目、内部抵销
  与少数股东算法。没有修改保存结构、迁移或建立集团副本映射。
- **未发现该增量的新确定性错误。** 所有生产调用已核对：new 与 seed 为 `None`，
  persistence 为保存的实际 ClosingEngine。来源先检查再从 coverage 排除，不能靠一个
  不存在的 source 隐藏缺期；不在 map 中的普通活动月仍必须真实一致。
- **新增边界建议。** 恢复短测应明确只有单成员更正新增 actual 月的成功状态，并保留
  未映射普通缺期的拒绝、未知 source 的拒绝、非历史有效期的拒绝、工商 tax／closing
  maps 不一致的拒绝；否则成功 case 不能证明没有过度放宽覆盖。重复成员、错 root 与
  缺 holding 仍走原集团图守卫，无需重新扩张业务模型。

父任务告知已接外层并补 save→restore、最后 publication 失败、二次版本 case；本增量
没有审查父作者的完整接线 diff，也尚未读取这些 case 的绿灯证据。原发现 2／4／5
仍按根任务统一测试和独立外层复核的最终证据判断，不能强行写成完整 PASS。

## 增量复核：真实集团日终短集成测试

2026-10-05 完整静态阅读新增
`packages/engine/src/session/company_corrections/group_tests.rs`，并核对相关 prehistory、
company assembly、公开库及集团 scheduled API。未执行测试，不声明绿色。

### 确定性阻断

`session()` 的 `start_date` 为 `2030-01-05`，但 `IndustrialConfig.as_of` 写为
`2026-12-31`。`company_assembly.rs::assemble_custom` 的严格 opening 校验要求
`start_date.year() - 3` 的 12 月 31 日，即 `2027-12-31`。三个 case 会先在
`GameSession::new(...).unwrap()` 失败，尚未到达目标更正行为。应仅改正 fixture
opening 日期，不能通过制造历史收入、放宽 assembly 守卫或弱化金额断言绕过。

### 测试语义

- 在现有 25% loss DTA 全确认简化下，100 元费用对应税后损失 75 元；80% 子公司
  更正的少数损失 15 元、归母损失 60 元。集团权益 1925 元、归母权益 1740 元；
  母公司再更正后集团损失 150 元、少数损失仍 15 元、归母权益 1665 元，手算正确。
- 同一 source 在 root／child 本地重复是合法的命名空间覆盖；旧公共 ID 查询值与
  原值逐对象相等、Correction sequence 2→3 和 supersedes 链的断言有实质价值。
- zeroFlow fixture 没有虚构原始业务，能够暴露真实的有效期比较事实；不能因
  `prior_year_facts` 仍按 actual periods 产生假拒绝而加历史 flow 让测试通过。
  若实际红灯证实，应修复生成器／诚实性验证有效期契约，并独立审查生产改动。
- 私有 2029 Annual 追加 Original sequence 2 后通过真实 scheduled API 发布最新
  sequence，符合排期与不可变旧版本职责；此 case 不声称推进整个真实日历到三月。
- 使用真实 scheduled API 提前构造三月 future publication 作为故障注入，能够触发
  最后集团报告的时间拒绝，所有经营／结账／公开库和 pending 请求保持原值的断言
  正确。cancel 走公开 API；仅在测试中恢复 pre-fault library 快照是撤销故障 fixture，
  不是产品允许删除 immutable 报告，不能据此新增产品删除接口。

### 尚缺边界

- 第一个 case 只对 restored session 做 round-trip 相等，后续 root 更正在原 session
  继续执行，**未证明恢复后继续更正／普通集团生成仍消费累积映射**。建议在 restored
  session 上取得新的 epoch 并执行后续步骤，不能跨恢复沿用旧 epoch。
- 可补直接 DTA、minority BS 及历史 CF 为零／实际 CF 单计断言，使净利总额之外的
  税资产和现金边界更显式；纯读投影三个 case 已有 CF 覆盖，但不代替行业集成事实。
- 未变化报告不增加版本、真实非 mapped coverage 缺期仍拒绝，以及恢复错误映射拒绝
  仍需根任务最终测试清单对应，新增三个成功／故障 case 未覆盖这些负例。

opening fixture 修复、实际红绿与上述恢复后续路径补证后，才能把相关测试门禁标为通过。

## 增量复核：opening 修复与类型化时间拒绝

2026-10-05 已核 `group_tests.rs` 的 opening 精确改为 `2027-12-31`，符合 2030
开局契约，没有新增 flow 或修改手算断言；上一节的确定性 opening 阻断已解除。

`InformationError::CorrectionPrecedesOriginal { original, original_at, published_at }`
及 helper 对 `original.published_at > published_at` 的直接返回已静态核对：ID 与两个
真实时点来自故障报告及当前请求，中文错误显式说明原因。新增这个 typed variant
必要且范围最小，避免用 `InternalWindowInconsistent` 或错误字符串判定可重试业务拒绝，
也没有把准则差错判断变成可恢复默认值。future publication 故障 fixture 仍会触发
此明确拒绝；候选中此前的临时 record 必须由 whole candidate 丢弃，不能局部提交。

复核当时读取的 `company_corrections.rs:202` 仍把所有集团 helper 错误统一映射为
`ReportCorrection`，尚未看到父任务承诺的“仅该 typed variant 可重试、其余保持 fatal”
分流。因此本节只确认 error 类型与 helper 层，不误称外层分类已完成；父任务需要
精确匹配 variant，并针对普通内部不一致仍 fatal 的路径独立核对。

新增三个日终 case 仍待根统一构建和执行，本记录没有绿色证据；恢复后继续执行的
测试路径建议仍保留。

## 增量复核：外层精确分流与恢复后继续更正

2026-10-05 最新源码复核确认下述先前待办已落实；保留以上各次当时读取的证据，
不反向改写早期状态或测试执行记录。

- `company_corrections.rs` 集团 helper 调用的错误分流现在只匹配
  `InformationError::CorrectionPrecedesOriginal`，返回
  `SessionError::ReportCorrection(ReportCorrectionError::Information(...))`；其他集团
  InformationError 保留 `SessionError::Information`，不再统一变成可重试业务错误。
  外层这一精准 typed 分流已确实实现，没有字符串匹配或吞掉内部不一致。
- 第一个集团 case 现在在 round-trip 断言后执行 `session = restored`，重新获取
  `report_correction_epoch()` 才提交 root 更正。测试现已覆盖恢复后的继续更正与集团
  sequence 3、旧 ID 保留，消除“只在原 session 继续”的先前缺口。
- opening 日期仍为合法 `2027-12-31`，未为比较项制造历史业务。
- 父新增 `prior_year_facts_with_adjustments` 按 source 对应的有效期间判断历史事实，
  closing 的 generate_with 与 prehistory 的 Original 定稿均传入其真实累积 maps，
  静态上修复了 actual-only 比较事实与重述生成器口径的漂移。这只是既有宽松诚实性
  守卫的有效期接线，不声明本复核已覆盖父生产改动的所有边界或取得执行结果。

截至本次复核，新三个集团集成 case 仍未有本复核读取到的执行绿灯，根统一测试结果
及完整最终 diff 门禁仍需另行补证；不将静态修复误记为完整 PASS。

## 增量复核：私有 Original fixture 与完成事实 scope 拒绝

2026-10-05 完整阅读 `group_tests.rs` 当前四个 case。父任务转述 root host14 实际结果：
恢复后继续 root 更正 case 为 0.30s green，最后集团 publication 失败／cancel／同 source
重试 case 为 0.33s green；私有 Original case 在取得缺失 fixture 版本的 `.last().unwrap()`
处失败（0.05s），尚未进入更正路径。本复核没有独立执行这些命令，也未定位到 host14
原始日志，因此该段明确作为父任务转述，而非独立取得的绿色证据。

私有 Original case 现先调用真实 `ensure_group_report(..., 2029-12, Annual)`，从已完
会计年度 Books 生成私有报告，并断言 PublicLibrary 没有该 scope／period／kind。
这是补齐合法前置 fixture，不提前公布、不改历史金额，也没有弱化 Original sequence 2、
旧版本不变及 scheduled 使用 latest 的断言。原 fixture 缺版本错误的修复正确且必要。

新增 `completed_root_correction_cannot_replace_standalone_target_with_consolidated_identity`
先完成真实 root 更正并证明未篡改 save 可恢复，再只改完成事实：Standalone 目标 ID
替换为同公司同年度 Consolidated Original ID，outputs 只保留真实合并 Correction ID。
未改变 Journal、有效期映射、真实公开报告或集团关系；同公司身份仍一致，故能专门定位
原恢复 guard 的 `ScopeId::Standalone(owner)` 条件，而不是混用其他未知 ID 导致提前失败。
精确匹配现有中文 `InvalidSave("更正操作公司与原始公开报告不一致")` 合理，生产 guard
未被修改，case 是有实质性的跨层负例，不是为了通过测试而改变恢复业务规则。

本增量未发现新的确定性测试语义问题。修正后的私有 Original case 与新增 scope case
仍待 host15 构建／执行；此前转述的两条绿色不能代替冻结后四个 case 的最终结果。
