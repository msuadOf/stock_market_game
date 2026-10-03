# Luna32：银行会计、行业报表与三宿主披露旅程复核

## 范围与全文证据

- 已读仓库 `AGENTS.md` 与 `docs/principles.md`。指定产品基线为 `08e4fc7`；当前工作树 `HEAD=a7c7ce357bdc9f88c03633744b2d5815db49e9b2`。核对 `08e4fc7..HEAD` 在银行、报表、披露、三宿主查询/保存生产路径的源码差异为空，故以下调用链可对应指定产品基线。
- 从首行读到 EOF：`.omo/evidence/company-information-npc-intentions/task-9-review.md` **203 行**、同目录 `worktree-baseline.md` **9 行**、`.omo/evidence/escrow-parallel-engine/F3/manual-qa/README.md` **78 行**，共 **290 行**。没有以命中片段替代全文阅读。
- 未运行测试、构建、浏览器或长验收；未改产品文件或 Git 状态。唯一写入为本审计记录。

## 章节覆盖矩阵

| 全文部分 | 当前源码与真实调用链 | 复核结论 |
|---|---|---|
| task-9-review §0：隔离执行、测试计数、工具链历史 | 原记录说明当时 detached worktree 的 Cargo 运行与数字；当前审计只静态检查，不重述其为本轮验证。 | 历史结果保留为当时证据，不冒充当前基线重跑。 |
| §1：银行会计语义、政策依据、K3、ECL、核销回收、报表分类、手算、存档 | 银行合同交易仍在 `BankBooks` validate/post/apply 路径；列报分类目前由 `accounting/reports/notes.rs` 的 `IndustryPresentation::Bank => bank::assignments()` 接入 `generate_report_set`。三宿主生产披露通过 `session/disclosures.rs::publish_scheduled` 取 `company.books().books()`、`industry_presentation`，再走 ClosingEngine 与 `PublicLibrary::publish_closed`。 | 银行科目到正式报表归类有后续任务 13/26 的真实消费链；没有把 task 9 的 `BankBooks::presentation_lines()` 当作报表发布本身。其 `bank_presentation_lines` 是便捷读投影，当前没有生产 caller 命中；报表完整生成走通用 ReportSet 与归类表。历史银行语义结论可保留，但将两种投影说成同一消费链会失真。 |
| §2：必要性、范围、依赖方向、原子过账 | `company/bank/chart.rs` 从 `accounting/reports/bank::codes` 引用码表；银行构造仍依赖 accounting，reports 不导入 company。`BankBooks::post_with_commit` 的本次操作提交边界与既有测试不变量仍可见。 | 边界方向与最小接线符合原结论。未发现此处新增跨层 A 股交易语义变化。 |
| §3：覆盖矩阵与 F1–F4 | 当前 `collect_loan_principal` 成功路径在测试中仍未命中（只见超额拒绝、0 金额拒绝）；`assess_credit` 差额为零的 `Ok(None)`/空批路径没有直接测试；贷款超过合同到期后续息没有单独测试；期限恰为 365 天（≤365 短期科目边界）也没有测试。源逻辑仍以 `days_since <= 365` 分类。 | F1–F4 仍是旧审计已登记的测试覆盖缺口；没有证据称已偿还，也不提升为已观察到的正常用户行为错误。 |
| §4：worker 偏离裁定 | 本轮未重新审查当年实现者的偏离清单；以当前源码核对到的银行报告接线与账户规则为限。 | 历史接受/偏离决定只作记录，不作新代码正确性证据。 |
| §5：Clippy 警告归属 | 未运行 Clippy。 | 旧的提交态归属仅为历史结论，本轮不复用作当前工具验证。 |
| §6–7：证据链与残余风险 | task-9-review 自述的 red/green/full-suite 文件、issues/learnings、Windows 临时 worktree 不在当前复核中执行。 | 证据链与残余覆盖项保留其时间边界；没有宣称当前完整验收。 |
| worktree-baseline 全文 | 记载 2026-09-10、`6ad461e`、当时干净状态及 `.omo` 例外。 | 它是 orchestration safety copy，不是本轮 `08e4fc7` 的产品源码基线；不可用其 dirty-path 断言代替当前检查。 |
| F3 README 全文：绑定哈希、Playwright 启动/资源边界、五场景、轨迹与哈希 | README 是 2026-09-23 针对旧 `bf3d444` 的 Web `trading-workflows.spec.ts` 手工 QA 记录，与当前公开报告/公司披露源码无直接覆盖关系。 | 五个交易 UI 场景及 hash 是历史手工 QA 回执；不能当作本轮或三宿主公司报表旅程验证。README 自述的两次 preflight 失败在 Playwright 加载用例前，不是产品用例失败，也不应从 PASS 证据中删除。 |

## 生产旅程追踪

### 银行和行业报表

- 银行交易账套有 `BankBooks::presentation_lines()` 调用 `bank_presentation_lines`（`packages/engine/src/company/bank/mod.rs:165`），但当前生产代码没有调用此方法；该方法不能作为「最终银行 ReportSet 由此生成」的证据。
- 正式列报生成链为 `session/disclosures.rs::publish_scheduled`（`:157–184`）→ `company.books().books()` + `industry_presentation(company.spec().kind)` → `ClosingEngine` 登记 → `PublicLibrary::publish_closed`。通用 `generate_report_set`（`accounting/reports/mod.rs:187`）调用 `notes::ReportClassification::from_industries`，银行分派由 `notes.rs:99` 使用 `bank::assignments()`；测试 `industry_spot_gold.rs` 另对 `bank_presentation_lines` 和完整银行报表分别建断言。这是两条不同的投影/生成路径，不等于前者是后者的输入。
- 工业、银行、保险、地产的 standalone 公司报告分类已在生成时按科目表/`IndustryPresentation` 选择。生产 `DisclosureDispatch::run_day_end` 只对 `CompanyOperations.companies` 逐公司排期调用 `publish_scheduled`；本次没有在该 caller 看到集团/合并报告排期，因此沿用总账 G28（生产合并披露缺接线），不另造重复编号。通用 consolidate `ReportSource` 能被直接调用，不等于 session 已安排其发布。

### 三宿主及 Remote 公开报告查询

- engine owner 是 `GameSession::query_public_reports`（`packages/engine/src/session.rs:2026–2034`），按当前 civil 时点过滤已发布报告；披露不是 UI 临时计算出的私有账套。
- WASM 路径：App 的 `CompanyQueryCoordinator` → `WorkerHost.queryPublicReports` (`apps/web/src/host/worker-host.ts:365`) → worker 消息 (`wasm-worker.ts:258–261`) → WASM binding (`apps/web-wasm/src/lib.rs:406–412`) → 同一 `GameSession` query。
- Server 路径：HTTP route `api_public_report_page` → actor handle `public_report_page` → actor loop `self.game.query_public_reports(&query)` (`apps/server/src/routes.rs:499–515`、`apps/server/src/actor.rs:772,1436`)。
- Desktop 路径：Tauri `public_reports` command → actor method → `self.game.query_public_reports(&query)` (`apps/desktop/src-tauri/src/lib.rs:159–167`、`actor.rs:426,1001`)。Web Remote adapter 则经 HTTP 获取同一 public DTO (`apps/web/src/host/remote-host.ts:274–280`)。静态链证明实际 caller 接线，不证明本轮浏览器/桌面三端运行验收。

## 日内保存：旧结论被现行契约替代，且已重证

- 旧设计/历史任务中“日内可保存当前状态”不能继续作为用户持久化契约。现行 ADR-0025 的口径由 `ProtocolSession::save` 只返回已冻结 `day_end_save` 实现（`packages/engine/src/session/protocol/civil/session.rs:175–196`）；成功 CivilUpdate 才创建候选（`:328`）。普通 `GameSession::save()` 和 verification/checkpoint 用于内存恢复/校验，不是三宿主写盘许可。
- 前端真实写链从 `App.tsx:234–245` 的 CivilUpdate 捕获候选引用，调用 `host.save(reference)` 并 `validateDayEndCandidate`；`DayEndPersistence` 串行化写入。WASM worker 的 `save_candidate` 入口要求明确 `(seq, settledDate)`；Server/Desktop actor 都按候选键取档。首个日结前无候选显式报错，日内继续推进不会把已捕获候选替换成当前 live state。
- 这些是相较旧日内保存结论的正式替代与当前源码重证，不是凭旧内存 checkpoint 测试声称用户可日内保存。三宿主磁盘/权限/跨进程失败等完整实旅仍属于独立验收范围；本轮没有运行它们。

## serde 极值和默认故障辨析

- `bank/behavior_tests.rs` 中 `ecl_validation_precedes_unknown_loan_and_overflow_does_not_commit` 先手工改 JSON 的贷款 `principal` 为 `i128::MAX`，再 `serde_json::from_value` 成功；后续 ECL checked arithmetic 显式返回 `AmountOverflow`，并断言账套 bytes 不变（约 `:160–190`）。这证明被合成的极值对象遇到溢出会显式拒绝，不证明正常开局或正常存档会无故报错。
- `recovery_overflow_retains_existing_post_then_partial_apply_order`（`:335–365`）手工篡改 `written_off/recoverable/allowance`，使用最大 allowance 重现「总账与 event id 已提交、loan recovery 子账只部分应用」的旧行业 operation 顺序。它是 serde 可表达、但被测试刻意拼造的内部不一致/极端状态；同文件清楚断言部分提交。依 `reaudit-accounting-contracts.md` 已核实这是重构前也存在的失败边界，不应归为本次重构回归或默认游戏故障。
- `restored_initial_policy_is_not_revalidated_during_issue`（`:190–218`）同样是人工改存档内 `stage1_default=[]`，serde 恢复后继续发贷且 allowance=0。与单纯 `i128::MAX` 算术边界不同，这是配置语义被编辑后的验证缺口候选：`BankBooks::new` 会校验 policy，但直接 serde restore 没有同等守卫，issue path 使用该策略。原 task-9 §1 所称“ECL 构造期 + 每次重估双守卫”不能覆盖恢复后配置。鉴于 bank 账套独立于当前 session 经营闭环（session 内完整银行运营的 G36 已在总账），本证据只登记为**存档恢复候选**，未把它说成默认 NPC/game path 故障；需与总账对照是否已有专项登记后裁定。

## 旧结论复核与新增候选

| 项目 | 当前裁定 | 证据边界 |
|---|---|---|
| task-9 §1–2 银行科目/会计语义与范围 | 基本维持；补充 task 13 实际 consumer 是 `assignments`/ReportSet，非 `BankBooks::presentation_lines`。 | 仅源码静态追踪；没有重新查官方 CAS 来源，沿用仓库正式文档及原独立复核依据。 |
| task-9 F1–F4 | 仍欠测试覆盖，不是本轮实现缺陷新发现。 | 当前测试搜索：本金成功无调用；空差额分支无专测；贷款过期后计息无专测；365 天整边界无专测。 |
| 日内保存结论 | 旧愿景被 ADR-0025 和当前公共保存接口替代；当前实现重证为只保存完成日终候选。 | 未运行三宿主落盘实旅；不把内存 checkpoint 混作公共存档。 |
| 三宿主报告 | 真实 API caller 均接入同一 engine query；发布端为当日日终 disclosure。 | 不等于 UI、网络/IPC 全场景 E2E 通过；合并披露缺失继续用既有 G28。 |
| 伪造 serde `i128::MAX` 导致 ECL 溢出 | 排除默认故障；实现显式 `AmountOverflow`、失败后账套字节不变。 | 仅在测试人工篡改后的 BankBooks 上触发。 |
| 极端 recovery 子账与已过账后的部分应用 | 保留为已有、非本轮回归的底层失败边界。 | 测试直接注入互相不一致的 serde 字段；与 `reaudit-accounting-contracts.md` 一致。 |
| serde 恢复空默认 ECL policy | 新的防御式校验候选，需总账去重。 | 测试明确证明 restore 后 issue 不重验；不代表正常开局配置。 |

## 总结

当前正式行业报表、公司公开报告查询及三宿主 engine caller 均有真实接线；银行分类层消费路径与银行账套便捷 lines 投影须区分。旧“用户日内保存”结论已被较新的日终候选契约替代且由当前接口/调用链重证。serde 极值失败不能泛化为默认游戏故障；另有恢复后空 ECL policy 被继续使用的候选需要总审计核查既有登记。没有运行测试或三宿主实旅，不报告任何新 PASS。
