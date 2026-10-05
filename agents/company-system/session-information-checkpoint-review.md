# Session 与 information checkpoint 独立复核

日期：2026-10-06。复核者未实施本批 Session／information 改动。检查基准为 `HEAD` 到当前工作树，包含 index、未暂存差异和新增未跟踪 Rust 源文件；未编辑 Rust、Cargo 或 index 文件，未运行测试。

## 审查范围与清单

- [x] 阅读 `AGENTS.md`、`docs/principles.md`、`docs/architecture.md`、`docs/open-questions.md`，以及 ADR-0035、ADR-0036、公司系统与 information 契约和既有 Session／information review。
- [x] 检查 `packages/engine/src/lib.rs` 的完整差异及新增 test-support fixture 导出边界。
- [x] 检查 `information/` 中 `mod.rs`、`prehistory.rs`、`public_view.rs`、`publication.rs`、`queries.rs`、`schedule.rs`、`monthly_schedule.rs`、`simple_disclosures.rs`、`source_tests.rs`，包含来源字段、公开库插入与恢复、更正链、完整报告供给、定期排期及查询时点。
- [x] 检查 Session 的公司组装、CompanySystem 状态复制／提交／hash／存档恢复、日终推进与披露、盘中月报 checkpoint、pending correction 日终事务及恢复校验。
- [x] 检查共享 ingress 的锁内收件、收据分配、tick cutoff、晚到输入、失败重试、账户登记及 save 投影；检查市场成员身份与 admission funding 校验。
- [x] 检查个人成交确认的 CommitTick 写入、SavedRuntime 恢复校验、账户隔离分页；检查 retained 市场历史及按账户记录历史读取的 query facade。
- [x] 阅读新增 Session/information 文件和测试：`company_corrections.rs` 与 group/time tests、`intraday_disclosures.rs`、`retained_history.rs`、`memberships.rs` 与 tests、`shared_ingress.rs`、`trade_confirmation_query.rs`、`live_minute_history.rs`、`monthly_schedule.rs`、`simple_disclosures.rs`、`source_tests.rs` 等，核对实施与测试所主张的边界。

## 结论

当前 Session 主路径只装配 `CompanySystem::Simple`；`Simulation` 在 setup/create 明确拒绝，未发现仍以 `CompanyOperations` 或旧账套作为新 Session 财务权威的路径。日终先应用 pending 更正，再用实际结算日推进系统，最后进行定期披露；整日外层 checkpoint 负责失败回滚。SaveSlot 对所选系统、身份集合、股票股数映射、推进自然日、公开来源和报告更正事实作严格校验；字段由严格 serde 契约承载，没有发现 schema 代际、兼容或静默补字段。Simple 汇总现金没有被当作投资者账户现金或实际分红资金来源。

`Information` 的 Simple 来源是显式必填值；发布仍依赖真实登记、勾稽通过的完整 `ReportSet`，月报扫描受配置排期和公开时点约束。公开查询与本人获知仍是不同边界，未见从私人 CompanySystem 财务读取未来公开事实的新增路径。股数、A 股撮合、T+1 及交易所时段规则未由本次检查发现变化；本复核不是股本行为的完整 A 股规则审查。

并发 ingress 在一个 mutex 临界区中校验玩家身份、检查当前已发布自然日是否可交易、分配账户／股票 ordinal 并登记输入；冻结快照以单一 `next_input` cutoff 固定本 tick 前缀，晚到输入留待后续 tick。读取器消费位点使失败 checkpoint 可以保留输入并重试。新增成员先在 ingress 注册账户，再执行无失败分支的本地插入；存档恢复核对成员账户与全部账户集合。个人成交确认按 receipt 归属到账户键并以账户参数分页，市场分钟／日历史是公开市场数据，但记录谁读取仍写入该账户自己的历史读取账本。

## Must-fix

1. **发行人会计类别被静默设为 Industrial。** `session/company_assembly.rs::issuer_specs` 对每只证券写入 `CompanyKind::Industrial`。`CompanyKind` 决定报表行业列报口径；股票配置并未显式提供公司类别，交易所／证券代码也不能推出银行、保险、地产等类别。结果是非工业发行人会被以工业口径装配并披露，模糊领域身份。应在进入 Simple Session 的发行人配置中显式承接会计类别（与交易所/板块分离），或对未配置类别明确拒绝；补充非工业身份及报告列报的短测，再由非作者复核。此项已在作者先前的 `session-review.md` 中提出，当前源仍未关闭。

除该项外，本 checkpoint 未确认新的 must-fix。`CompanyCapabilities.cash_settlement == false` 是共同公司行为实际投资者结算 caller 尚未接通的已标记状态；按本次委托边界，不将它扩大为新的 Session finding。既有 `host67` 是理由映射修正前的失败日志；当前状态只采用作者记录中的 host68 定向 Q17 7/7 fresh 结果和 host64 指定短测结果。它们不是 regression 或完整 workspace 验收证据，本复核没有自行运行。

## 依据与边界

- `.tmp/checklist-wave4/host64-short-tests.log` 中可辨认的定向组结果：group 8/8、Simple core 15/15、finance 11/11、Session 9/9；此处仅引用对应短测组，不将并行日志拼成整体验收。
- `.tmp/checklist-wave4/host68-q17.log` 及 `agents/company-system/q17-simple-independent-review.md` 记录当前 Q17 Session 定向组 7/7；`.tmp/checklist-wave4/host67-q17.log` 是修复前失败轨迹，不作为当前失败。
- 未运行 Cargo，也未检查非本任务范围的宿主 UI／Server／Tauri 完整接线。无交易所规则变化的静态发现不替代股本行为的官方依据审查。
