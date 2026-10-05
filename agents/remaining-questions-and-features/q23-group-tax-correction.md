# Q23：固定单层集团的税务与报表原子更正

## 要完成的目标与依据边界

本记录包含实现方案、当前代码进度与边界盘点，不代表完整验收。当前
`CompanyOperations::correct_industrial_report` 用 `GroupedTaxCorrectionUnsupported`
阻止集团成员只更改单体、留下合并旧结果，这是安全过渡，不能作为合法固定集团的最终交付。
用户要求完整推进 Q23；既有固定单层集团已支持 `Standalone` 与 `Consolidated`，其成员更正必须
联动当期税、递延税、资产负债、归母权益、少数股东权益与不可变报告版本。

官方依据沿用 `docs/company-accounting.md` 已核验的财政部财会〔2014〕10号及 CAS 33
全文（2014-07-01 起施行，核验记录包含 2026-10-04）：控制范围、统一期间、逐项抵销以及
少数股东权益和损益分别列示。该依据不意味着集团各公司可以共享税务亏损池、互抵应交所得税，
也不把本工程处理声称为完整 CAS 18／28 或法定税务追补程序；两者全文缺口与简化税模型
沿用 `q23-tax-official-research.md` 的明确限制。

既有游戏简化继续保留：单层固定控制、持股严格超过 5000bp、精确整数基点、期间覆盖一致、
无长期股权投资与子公司权益抵销、内部销售单批次与比例毛利分摊、跨年未售批次显式拒绝。
不会借更正扩展并购、变动控制、多层集团或未支持的结构化子账调整。

## 当前代码中不能只补一个参数的地方

1. `ReportRequest.adjustments` 是 `BusinessEventId → AccountingPeriod` 单映射；
   `BusinessEventId` 只在各公司本地唯一。母公司和子公司都可合法使用 source 1，不能合并成
   一张无 owner 的映射，更不能为报表改写权威 source 为全局号。
2. `reports/consolidated_window.rs::consolidated` 先按实际 `entry.period()` 截断成员 Books。
   晚于报告期、却对历史有效的更正凭证会在计算前被丢弃；只把有效期传给后续扫描无法修好。
3. `WindowConsolidationBuilder::scan_members` 的当年损益、窗口损益、上年权益、根资本与
   非根权益全按实际期间；`add_entry` 的 `mapped` 固定为 false。损益、比较项与现金的双口径
   均需要接入成员各自的有效期间。
4. `consolidate` 最终期末权益及少数股东权益来自成员 Ledger 的累计实际余额。
   仅修当期利润表，仍会留下期末权益和少数份额错误。
5. `company_groups::ensure_group_report` 已有版本就直接返回；正常后续集团生成也未消费
   成员累积重述。它不能承担“已公开版本追加更正／尚未公开 Original 重新定稿”的职责。
6. 单体组合处理器可以在候选中发布一组后续版本；集团更正还必须在同一外层候选内完成。
   不能先把单体候选安装到权威 CompanyOperations，再尝试会失败的合并生成或发布。

## 最小职责接线

纯输入与成员 Ledger／窗口接线已实施，集团版本刷新 helper 已提供；外层 GameSession
组合事务由 Q23 父任务接线。新 source 的失败 stub 使用 host-build-7 的真实新 Engine rlib
链接 `consolidated_restatement` 测试，明确列出 1 个 case 后在
`member_local_sources_restate_profit_equity_without_backdating_cash` 返回
`ConsolidatedRestatementUnsupported`，实际 0 passed／1 failed、exit 101，日志在
`.tmp/checklist-wave4/group-restatement-red.log`。此前 Cargo 总批次因另一任务的 SaveSlot
比较编译错误失败，不把该编译错误冒充集团业务红。host-build-8 成功后，真实新 binary 的
`--list` 确认三个投影 case，10000ms 外部 deadline、`--test-threads=3` 执行得到
3 passed／0 failed、exit 0，日志在 `.tmp/checklist-wave4/group-restatement-build8-green.log`。
原合并 March 金样、独立纳税主体与比较项、混合行业应收、重复成员拒绝四个 exact case
也以四进程并行短测分别通过，日志在 `.tmp/checklist-wave4/group-existing-build8-{0,1,2,3}.log`。
这些结果只验读投影与原合并代表边界，不代表外层集团事务已经完成。

### 成员命名空间的纯输入

新增纯输入 `ReportSource::ConsolidatedRestated`，携带现有 `ConsolidationRequest` 和
`MemberId → (BusinessEventId → AccountingPeriod)` 借用映射。单体原 `adjustments` 不套给
集团；两套输入若同时非空，应明确拒绝歧义。映射在每次生成时从
`ClosingEngine::restatement_periods(Standalone(member))` 派生；工商成员同时校验其
`income_tax_restatements()` 完全一致。不要再保存一份集团来源映射。

映射必须逐成员校验：owner 在本集团、source 确实在该成员 Journal、有效期间早于实际期间。
未知 owner／未知 source／无效有效期显式错误。没有重述的成员使用明确空映射，这是无事实，
不是兼容旧存档或补缺字段。当前格式仍严格，不增加 schema、迁移或代际标记。

### 合并读投影，而非伪造 Journal

合并窗口应保留原始成员 Journal 供现金流与有效期扫描，并为合并截止日构造纯 Ledger
读投影：选择“成员有效期间 ≤ 报告末期”的已验证真实分录，直接计入派生 Ledger，
不改分录日期、source、kind 或 cash_flow，不给成员 Books 再过账，也不保存投影。

建议把合并内部余额／抵销／少数股东核算使用的 `&Books` 读需求收敛为 `&Ledger` 加
明确期间覆盖事实；普通 `consolidate` 从真实 Books 提供相同读视图，重述窗口提供截止读投影。
这只是已有算法的读接口，不能建立备用业务引擎。全部成员都采用同一 cutoff 和有效期口径。
期间覆盖保留真实日记账基础：截止期内、非重述调整的实际记账期间集合须一致；重述有效期
不反向创造历史日记账期间，晚记更正也不为单个成员制造假的当月覆盖。不能比较全部有效期
集合，因为母子公司可以各自更正不同历史月，两张合法更正映射本来就不必相同。

不能用临时 `Books::post_batch` 的权威付款守卫验证有效期投影。某笔真实付款可能依赖历史期后
收到的现金，而有效期重述的历史现金余额可能为负；派生投影不应冒充该历史时点发生了付款。
现金流继续由未经改写的 Journal 按实际期间累计，`Accumulator::add_entry` 接收该成员有效期及
mapped 标志，显式 restated_cash_correction 配平，不制造退款或双计现金。

`scan_members` 的当年／窗口净利、上年权益、根资本及非根权益使用各成员有效期；期末少数和
归母权益用截止读投影经真实抵销后的结果。已支持抵销科目的结构化更正仍走现有显式拒绝，
因此本批更正不改内部往来／库存身份；正常集团 request 的内部交易 cutoff 派生不能被省略。

### 外层集团事务

集团编排归持有真实 `GroupStructure` 的 session 层；company/accounting 不 import session。
先由实际公司注册表与 GroupStructure 验证更正成员归属，不允许调用方用任意 owner 或
“跳过集团检查”布尔值绕过组合入口。

候选中依次：成员业务更正与年度税级联 → 成员单体目标及后续版本 → 全集团报告重建 →
所有受影响已公开合并版本追加 `Correction` 与 supersedes → 受影响未公开 Original 重新定稿 →
成员税／结账映射一致性、集团余额与报告勾稽、公开库恢复守卫。全部成功才一次安装
CompanyOperations、ClosingEngine、PublicLibrary。失败原因带原更正理由；整个候选丢弃，
同一 source 可在修复条件后重试。

已公开报表逐字节保留，NPC 以前获知的 PublicationId 不改写；新合并 ID 仍属于集团 root，
不是被更正子公司。未变化的报告不新增版本或公开 ID。未公开 Original 重建策略与本批单体
处理器保持同一契约，不能覆盖已经公开的版本或把财报排期提前。

非作者完整静态复核发现正式文档仍写“不支持合并重述”以及恢复 raw coverage 与重述
coverage 不一致的跨层风险；前者已在原 D5 和集团公开链对应段落修正，后者已让恢复
`validate_groups` 显式读取当前 `ClosingEngine` 成员映射并校验真实 source／历史有效期。
恢复的累计 Ledger 仍全部来自真实 Journal，只从覆盖事实中排除重述调整，绝不补普通
缺期凭证。新局／前史初始化没有重述时保留原覆盖规则。非作者增量复核认可修复必要、
未发现新增确定性错误；外层版本、回滚及保存恢复成功与拒绝矩阵仍由父任务补齐短集成证据，
不能把这份静态结论标为完整 Q23 验收。完整回执见 `q23-group-tax-correction-review.md`。

## 短单测清单

- 母子公司相同 source、不同有效期：各自金额只归本人的历史期，不相互覆盖。
- 80% 子公司历史费用更正：当期／年度利润、税及 DTA、归母和少数股东损益／权益逐分手算，
  母公司同额更正对少数股东影响应为零。
- 同一集团不同成员一侧 CurrentTaxAssets、另一侧 CurrentTaxLiabilities：不能跨成员净额抵销。
- 实际更正月晚于历史报告末期：历史有效更正必须出现；当前实际期现金只记一次，历史 CF 不造
  新付款；历史报告窗口、季度、YTD、上年比较、上年期末权益均分别检查。
- 更正引发跨年亏损抵扣级联，后续已公开集团报告追加更正，未公开 Original 正确重新定稿；
  原 PublicationId／报告字节、获知历史和下一年度普通计税仍正确。
- 保留合法内部应收应付／未售利润抵销，集团现金等于真实成员现金合计；原简化拒绝不变。
- 集团末个发布失败、税重算溢出、未知成员／source、错 root／scope：所有成员、ClosingEngine、
  PublicLibrary 完全不变；修复后相同 source 一次成功；重复操作不得新增凭证。
- 严格保存→恢复→后续正常集团生成，累积成员重述仍有效，不新增集团持久映射或兼容分支。

`session/company_corrections/group_tests.rs` 已提供四个真实 GameSession 日终短 fixture：
子公司及母公司分别更正后 seq 2→3、20% 少数损益手算及恢复后续更正；私有 Original 重建
再由真实排期接口发布末版；集团最后一份未来公开报告触发时间拒绝，整日回滚后公开取消
待办、使用同 source 重试并恢复存档；完成事实的原目标不能由 `Standalone(root)` 偷换成
同期间 `Consolidated(root)`，即使只留下真实合并更正输出也须恢复拒绝。fixture 的零经营流、1000 元开局现金和 100 元费用
不制造历史收支，25 元 DTA 来自已有显式 25% 全额确认游戏税模型。2030 开局的 opening
按实际契约为 2027-12-31。故障用真实发布接口提前登记已闭年报，在测试中恢复故障前的
Library 快照仅为排除故障，不表示产品允许改写 immutable 公开库。

集团更正公布早于原报告的条件使用明确 `InformationError::CorrectionPrecedesOriginal`，
携带原 ID、原时点和请求时点；只有这类已预期业务拒绝允许修复后重试，不能把任意报表
勾稽／coverage／账务错误按字符串统一降成可恢复请求错误。host-build-14 已实际验证
恢复后 root／child 续更正与最后集团失败／取消／重试两项分别为 0.30s、0.33s 绿色，日志
为 `.tmp/checklist-wave4/q23-build14-{5,6}.log`。私有 Original case 先因 fixture 尚未登记
未公开 2029 年报而失败（0.05s，`q23-build14-7.log`），现已通过真实 `ensure_group_report`
显式生成完期私有版本、确认尚未公开，再保留原字节／新序号／排期末版断言；没有把私有
版本伪装成未来已公开报告。修过的 case 与新增完成事实篡改 case 仍待下一次实际短测，
不将 fixture 初始化修复当业务验收。父外层分流已获非作者增量静态确认。

每个独立 case／命令维持 10000ms deadline，构建由 root 集中并发执行，最后由非作者独立
复核完整 diff。集团作者没有运行 Cargo 或操作 index／commit；已修改上述分工范围的生产
接线，但不把源码存在或 rustfmt 成功等同业务短测通过。
