# CompanySystem 与 Session 接入独立复核

本记录为当前工作树的静态复核，不是最终验收。复核者未实施 Session 改动；没有运行 Cargo、单测或完整回归，没有暂存、提交或修改其他任务的源码。已完整阅读 Q14 设计蓝图、根协作规则和工程原则，并核对 Session 的生产接缝及相关当前类型。共享工作树正在重写共同财务接口，当前结果不代表后续源码自动通过。

## 需求与必要性

用户要求先接通 `company/simple/`，`company/simulation/` 留给独立分支。Session 以 `CompanySystem` 取代强制 `CompanyOperations`、完整经营注册表和独立结账状态，符合该边界；生产 `new` 没有发现偷偷启动旧经营仿真的调用。拒绝尚未实现的 `Simulation` 是诚实的当前边界，不是缺失完整仿真实现的问题。将旧账务覆盖保留为明确的测试 fixture 合理，但不能把 fixture 测试的通过视为新生产 Session 的财务覆盖。

本改动不应改变证券撮合、T+1、申报单位和资金单位。新的公司财务以及股本行为仍须复用共同会计和结算规则；Q14 蓝图不是 A 股法源。当前接缝未增加新的证券交易制度，未独立重新核验交易所规则，不能据此宣称股本行为的领域审查已经完成。

## 需要修复与再次复核的问题

1. **盘中披露游标与低层恢复不一致。** `session/intraday_disclosures.rs::publish_intraday_reports` 每次调用 `run_simple_scheduled` 都把 `published_through` 推进到当日盘中，即使没有新的月报。`session/persistence.rs::validate_disclosure_cursors` 对已经日结的状态仍要求游标精确等于上一日 23:59:59。因此 Monthly 模式次日 step 后的低层内存存档不能正常恢复。公共日终入口仍应严格只接受日终档，低层 checkpoint 则必须允许不超出当前观察时点的盘中游标。补一个完成一次日结后再走一步、保存并恢复的短测，以及超过当前观察时点的负例。

2. **更正事务接缝已重新接入，尚待共同 API 与短测证据。** 初次审查时日终流程删除了 `apply_report_corrections_at_day_end` 调用。重新核对发现，初版记录声称 pending 被序列化并恢复不准确：`pending_report_corrections` 只属于内存 state 和 clone/commit，不在 `SaveSlot`，也不从档恢复，因此不存在所述可注入待更正存档的路径。当前日终已恢复消费 pending，先在系统、公开库和 completed facts 的候选副本中执行整批，再同时安装成功结果并清空 pending；外层日终失败仍回滚。恢复已调用共同系统的 `validate_report_correction_fact` 验证实际凭证。相关实现与测试还在完成，不能据接线本身报告原子性及幂等已经验证；仍需失败无部分过账、重复载荷、同 ID 改载荷、恢复后不重复处理的短测。

3. **旧生产接缝尚未完全收敛。** `session.rs` 仍生产导出 `CompanyOperationsClockWiring`、`DayEndDisclosureCtx`，`disclosures.rs` 保留直接接 `ops/closing/groups` 的另一套 `run_day_end`。它们没有被新局偷偷调用，但仍是公开旧入口。保留账务测试需要的接缝应转为明确测试 fixture，或放到唯一的仿真实现内部；正式 Session 只调用共同系统，不长期保留平行入口或兼容别名。

4. **发行人类别硬编码问题已关闭（静态复核及 fresh 定向验证）。** `session/company_assembly.rs::issuer_specs` 现在只接受 `Simple`，按 `CompanyId("C-" + StockCode)` 精确匹配必填的 `SimpleCompanyConfig.company`，再复制其必填 `kind`；重复配置、缺少公司配置及 `Simulation` 均返回 `InvalidSetup`，没有从证券代码、交易所或板块推断会计类别的 fallback。`SessionSetup::validate` 会先构造该发行人集合并调用 `validate_for_issuers`，新局装配也走同一 `issuer_specs`；存档校验将构造/映射错误改为 `InvalidSave`，随后比对恢复系统与 setup 的完整发行人身份。静态检查未发现身份映射错误或被吞并的错误。Fresh host70 日志 `.tmp/checklist-wave4/host70-simple-short.log` 中，完整 Session 定向批次为 `12 passed; 0 failed`（0.96 秒）；逐项包含本节新增的三项 issuer tests，其中真实 `GameSession` 用 Bank/Insurance/RealEstate 合法现金科目，校验身份、报告 policy chart version 3/4/5、非空公开报告及 restore 后 hash 一致。日志还包含并行 suite 输出交错；上述 Session 批次的完整 `running 12 tests`、三项命名用例和对应结果行可亲读。host69 前置红测 `.tmp/checklist-wave4/host69-real-kind-red.log` 保留为根修前证据，不作为通过证据。本结果仅关闭该 Session kind 接线批次，不代表全部 checklist 或完整回归通过。

   **仍需后续确认的身份字段边界：** Session 为所有这些虚构发行人写入同一个 `IndustryId("listed-simple")`。这不是本次会计 `CompanyKind` 推断问题，且当前 Simple 财务报告由 kind 选择行业列报；但 `IndustryId` 在共享规格中明确表示行业，并被旧经营仿真用于行业冲击分组。后续若该字段继续进入行业经营/行业查询，应为虚构预设提供独立且语义诚实的行业配置/分类，避免把上市身份或 Simple 模式标签当作行业。当前没有据此发现交易所或上市板块映射错误，Simulation 也不在此改动范围内。

5. **旧测试引用与错误类型断言已修复，静态发现关闭。** 当前 `session/persistence.rs` 的所审字段引用已改到共同发行人或明确财务 fixture。缺少合并归母损益的负例现在直接调用 `ClosingEngine::validate_consolidated_parent_income`，这一底层覆盖迁移合理，不应报告为新 Session 恢复覆盖。再次亲读实际源码确认，断言已明确匹配 `Err(error @ ReportError::MissingParentIncome { .. })`，同时保留 `income.net_income_to_parent` 字段名文字校验；不再接受任意错误类型。真实编译与短测证据仍待根任务执行。

## 已检查的正确接线及尚待证据

`CommittableSessionState` 的 clone 与 commit 都显式带 `company_system`；hash 已包含所选系统投影；save 导出完整系统、restore 覆盖选中系统；restore 校验系统配置、发行人集合、股票映射及 `advanced_through == civil_clock.current_date.prev()`，没有发现这几项漏接。日终保留 checkpoint，在公司推进或披露出错时恢复整体状态；新盘中披露测试也明确检查库与本人信念不泄漏。

共同财务接口的其他实施者正在替换旧轻量 capabilities 和报告派发位置，暂登记为未闭合接缝，不将中间状态当作最终失败或已完成。已重新完整阅读最新 Q14 蓝图：用户取消 `SyntheticFunding`，Simple 的公司现金是账面展示、不作为分红实际预算；因此此前要求实现这套资金生成机制不再适用。待最终接口稳定后，仍须检查完整报告、税务、股本行为与投资者实际账户结算、私有状态不进入公开披露，以及历史材料截止日和恢复时不重抽前史。Simple 的账面边界不能扩展到 Simulation 的有限现金约束。

当前 Simple 内部历史起点尚未显式与 Session 新局日期及 `prehistory_months` 绑定；新版状态应确认恢复不会接受与选定开局配置不一致的前史长度。新日终/盘中失败短测及真实编译证据由根任务统一执行，本记录不报告测试通过。

月末后置失败与重试短测已增量复核（静态）：`session/notices.rs::information_revaluation_failure_rolls_back_entire_day_end` 以 `quote_setup(0)` 为底，继承真实 `Simple` fixture 的 24 期前史、月结、季度报告排期及 1 名机构（零售/热点为 0），起始日为 2032-01-31（周六且月末），没有构造 clock due。调用顺序为 `CompanySystem::advance_day` → `SimpleFinanceState::apply_period`（summary、所得税及月结版本）→ Simple 披露 → `deliver_public_information`；故损坏公开库后、季度材料触发的缺失 Annual 锚错误确在财政结算后发生，属于被编辑公开材料破坏既有信息不变量，不表示合法无历史或非正分母应失败。故障断言现匹配 `SessionError::InvalidSave`，并核验“公告重估失败”和 no-own-known-annual 上下文；恢复原库重试后，断言当日 `SimplePeriodSummary` 与 `TaxAccrual` 各一条且分录金额为正、Monthly `Closing` version 恰一份，并检查月报 `OperatingRevenue` 与所得税均为正。重复同日 `CompanySystem::advance_day` 精确匹配 `CompanySystemError::Invalid` 及日期连续上下文，且比较调用前后完整序列化系统状态；Session hash 在重试提交后保持不变。失败路径还比较失败前后的 `business_state_hash` 与完整 serialized save。增量复核认为这些断言现具体覆盖后置失败、整体 rollback、真实月度收入/税/闭账及单次 retry；没有发现 tautological retry assertion。该项仍是静态结论，fresh alone 短测待根任务执行，本复核未运行测试。

## 公告材料与测试迁移的增量审查

`record_company_disclosure_events` 从原日终事件函数抽取后，生产流程仍先记录公告、再定期报告、最后 `CivilDateAdvanced`，没有发现生产顺序变化。抽取仅复用同一事件构造逻辑，没有新增伪造股份、现金或成交能力，范围必要。

新的 `confirmed_announcement_fixture` 在 cfg(test) 财务 fixture 内真实建立旧 Ops 前史，应用具名 shock，通过 `CompanyOperationsClockWiring::run_day_end` 和 `DisclosureDispatch` 得到已确认公开材料；没有直接拼接假的 Announcement JSON。订阅资格与非交易日日结测试保留本人获知时点、淡出资格、现金、委托游标、无行情读取及恢复一致性的原断言。这些测试是共同公开材料消费者覆盖，不能证明 Simple 自己真实履行合同或产生违约经营事实。

Protocol 测试用真实材料替换公开库，再于 `end_civil_day_update` 返回后追加由同一事件函数构造的公告事件，重建 facts、seq 和 snapshot 并校验。它仍验证公开索引与私人 acquisition 无关、恢复后的公开库可查询，但属于测试组合的 CivilUpdate 消费覆盖，而非生产公告到宿主更新的完整发布事务证据。再次亲读源码确认测试已改名为 `confirmed_low_level_announcement_consumer_exposes_only_public_index_after_reconnect`，明确这个范围，命名发现关闭；不应把其通过据称为 Simple 合同公告闭环或整个发布 barrier 完整验收。真实执行证据仍待根任务编译后短测。
