# N3 批：公司行为接续进汇总账簿 + 账面现金/投资额展示字段

- 日期：2026-10-08。
- Worktree：`wf_20860509-c0a-1`（基线 main `5bdf0953`，分支 `worktree-wf_20860509-c0a-1`）。
- 用户决策（2026-10-08 原文）：「公司账面现金/投资额展示字段：现金=累计留存收益（净利润−累计分红）、投资额=累计收入×固定比例（默认 30%，新局可编辑）；均为展示值不建模真实资金流（账面/真实分离铁律不变）」。
- 审计定位（R1 余项）：送转/配股增发/回购注销此前只更新行为 fact 与 `legal_facts.registered_capital`，不进 `Books`——股本、资本公积与 ROE 口径断链；capability/Books 接续为 Simple 自身缺口。

## 实现

1. **新科目与报表行**（仅 Simple 账套）：
   - `simple_capital_reserve`（Simple 资本公积—股本溢价（账面），权益）→ 新权益行 `BsLine::CapitalReserve`（标签「资本公积」）；
   - `simple_issuer_funding_asset`（Simple 发行人募集资金账面调整（非现金），资产）→ 新资产行 `BsLine::SimpleIssuerFundingAdjustment`。
   - 四行业基础科目表未归类资本公积科目，`EquityPresentation` 按科目表归类存在性决定是否列行（非 Simple 账套列报不变）。
   - 附带修复（自查发现，HEAD 既有）：`balance_sheet.rs` `prior_lines` 比较项主循环改为跳过全部权益行（原只跳 `PaidInCapital`/派生行，法定公积金被主循环与 `EquityPresentation::from_prior` 追加段各列一次；修复后权益行恰好出现一次，fixture 复核确认）。
2. **行为汇总分录**（`SimpleFinanceState::post_capital_action`，全部 NonCash、日期必须晚于 `as_of`、消费 `next_event_id`）：
   - 送股：借 4103／贷 4001；转增：借 simple_capital_reserve／贷 4001（声明与入账均按资本公积余额显式校验）；
   - 配股结算：借 simple_issuer_funding_asset（认购款）／贷 4001（面值）／贷 simple_capital_reserve（溢价，零溢价不落行）；
   - 回购注销：借 4001（面值核减）／贷 simple_capital_reserve（等额归集）——简化登记：不按库存股成本核减权益总额（合成资金从未进入账面权益）；
   - 缩股：借 4001（消灭面值）／贷 simple_capital_reserve；拆股面值总额不变、无分录；
   - `BusinessKind` 新变体：`CompanyStockDistributionCredit` / `CompanyRightsOfferingSettlement` / `CompanyRepurchaseCancellation` / `CompanyShareReDenomination`。
3. **ROE 联动**：`actual_equity_events` 把配股结算（权益净增加）分类为规则 9 `Increase`（按实际生效日加权）；送转/注销/缩股净额为零、跳过（不触发 `UnclassifiedEquityEvent`）。
4. **账面展示字段**：`SimpleFinanceState.cash_book / investment_book`（cash = 累计净利润−累计已付分红，可为负；investment = 累计收入×bp 半偶舍入落分）；每结算周期末与分红付款后从权威账簿重算；`validate()` 重算不变量。`SimpleFinanceConfig.book_display: SimpleBookDisplayConfig`（`investment_of_revenue_bp` 0..10000，默认 `DEFAULT` = 3000，const 非 serde default）。**两字段不进资产负债表**（报表勾稽零影响；理由：展示值进报表会制造与真实权益的勾稽假象）。查询面：`SimpleFinanceState::book_display()` → `CompanySystem::simple_book_display(company)` + ts-rs `SimpleBookDisplay`。
5. **勾稽与守卫**：`validate_capital_action_entries` 按行为事实重构期望分录与账簿做多重集匹配（缺失/多余/漂移显式拒绝；行为分录不被周期结算回退）；`validate_book_display` 重算展示字段；分录日期守卫拒绝向已封账期间回写。
6. **附带修复（fixture 自检发现）**：`balance_sheet.rs::prior_lines` 主循环此前只跳过派生行与实收资本，法定公积金（及本批新增资本公积）会被主循环与权益追加段各列一次——HEAD 已装 fixture 的 `prior_year_end` 即有重复 `StatutoryReserve` 行。本批改为统一跳过全部权益行、由 `EquityPresentation` 追加段唯一提供，三份重生成 fixture 复核无重复行。

## 测试（红→绿）

- 新文件 `packages/engine/src/company/simple/finance_books_integration_tests.rs` 12 项：红证据 `.tmp/company-system/books-integration/red-compile-errors.log`（22 个缺失 API 编译错），绿证据见下。
- 迁移一项既有断言（`bonus_share_cap_uses_registered_capital_evolved_by_prior_credit`）：送股入账现真实核减 4103，第二次送股上限按扣减后可分配利润（21_300）核定，24_000 超限被拒、21_000 受理——原断言编码的是不过账的旧行为（即审计断链本身）。

## 独立复核（2026-10-08，非作者 subagent `code-review`，静态只读）

结论 pass：0 major、2 minor、3 note；发现与处置：

1. [minor] cash_book 公式中「分红」未显式写明仅指现金付款（送股借 4103 后 RetainedEarnings 行减少而 cash_book 不变，存在口径背离）。**处置**：按用户公式字面口径（净利润 − 累计分红）保留实现，在 company-accounting §8 与 trading-rules 显式登记「分红=已付现金分红；送股/转增不改变 cash_book」及已知口径差异，不静默。
2. [minor] `validate_capital_action_entries` 负路径（删除/篡改/多余分录）无 engine 用例。**处置**：新增 `tampered_capital_action_entries_are_rejected_by_restore_validation`（删除分录、篡改金额、伪造多余注销分录三向均被 validate 拒绝）。
3. [note] 配股「发行价 ≥ 面值」前置校验：M 批 `declare_rights_offering` 已有显式校验（《公司法》第 148 条拒绝折价），结算按冻结事实重算，premium 不可能为负——已向复核者确认，无需改动。
4. [note] 分录构造逻辑三处重复（posting/校验重构/文档表格）：登记为后续重构候选，本批保持最小范围不动。
5. [note] prior_lines 去重修复建议补 engine 断言。**处置**：`report_roe_survives_cross_action_windows` 内追加「比较项每个权益行恰好出现一次」断言。

## 验证

| 项 | 结果 | 证据 |
| --- | --- | --- |
| 新增 12 项集成测试（含复核轮补入的篡改三向负例与比较项权益行唯一断言） | 全绿 | 终端复测（10000ms 外部 deadline） |
| `company::simple` 全组（105 项，拆分子过滤） | 全绿 | finance 40、preferences 20、tests 18、period 15 等 |
| 受影响组：cash_dividend 35、share_split 6、rights_offering 11、issuer_repurchase 3、stock_distribution 13、ex_reference_price 24、corporate_actions 14、company_simple_session 36、cash_dividend_tax 20、dividend_tax 29、simple_preferences 16、share_split_session 8、information::source_tests 10、period_roe 7、disclosure_roe 13、company_mechanism 3、report_correction 5、closing 5、consolidation 1 | 全绿 | 各组外部 10000ms deadline |
| engine lib 全量 | 复核修复后终跑 1566 过 / 257 败，**失败集与 main@5bdf0953 基线逐项一致（257=257，diff 为空）**；其中 balance_sheet 2 项与 session::persistence 13 项经独立 HEAD worktree 复跑证实为存量 | `.tmp/company-system/books-integration/engine-lib-full-final.log`、`engine-lib-failures-n3.txt` vs `engine-lib-failures-head-baseline.txt` |
| ts-rs typegen | 156 项导出通过；`SimpleBookDisplay.ts`、`SimpleBookDisplayConfig.ts` 新增、`SimpleFinanceConfig.ts` 更新 | 终端复测 |
| Web 受影响 schema 消费组（company-schema、simple-finance、system、company-system-config） | 全绿 | `/tmp/web-schema-tests.log`（本地临时，正式复跑证据见 web-full-2） |
| Web 全量（231 文件，8 分片） | 6 项失败全部为 main 既有/环境性（public-financials-render「当季」标签、personal-trade-confirmations、Worker×2/Tauri 分配查询、file-target-permissions 需 desktop 构建产物）；另一次无 node_modules 运行中 8 个 react 导入失败文件经链接主仓 node_modules 后全部通过；**零新增失败**（关键项在 HEAD 基线 worktree 复跑复现） | `.tmp/company-system/books-integration/web-full-2.log`、`web-full.log` |
| tsc -b --force（apps/web） | 0 错误 | 终端复测（node_modules 链接主仓，只读） |
| oxlint（本批 web 文件 9 个） | 0 警告 | 终端复测 |
| cargo check --workspace --all-targets --exclude stock-market-game | 通过（desktop crate 需先构建 web/dist，属既有环境前置，与 P 批同口径） | 终端复测 |
| fixtures | 三 producer（rustc 直连 debug engine 编译，确定性复跑逐字节一致）重生成 `current-schema-save` / `current-closed-day-save` / `minimal-current-save` + company 切片投影；producer 内置守卫（restore/resave 深等）全过；HEAD 基线 producer 对照 diff 仅含本批预期变化（新科目、展示字段、config、报表行），零市场侧漂移（对已装档的市场侧差异为基线提交后的存量漂移，非本批引入）；终版 fixture 复核 prior_year_end 无重复权益行（附带修复验证） | `.tmp/company-system/books-integration/producers/`、基线对照 `/tmp/head-baseline/` |

## 边界与遗留

- 展示字段的宿主查询面（web-wasm 导出、worker/host/MarketRuntimeProvider 接线、CompanyPanel 呈现）未接线：引擎查询与 DTO 已就绪，UI 批次后续接。
- 「新局可编辑」投资比例：engine 配置面已可编辑（`book_display`），新局 UI 编辑入口未接（与偏好输入同类的后续批次）。
- 回购注销不按库存股成本核减权益总额（简化登记于 company-accounting §8）；盈余公积转增通道与 25% 留存下限未建模（本模型无该类别）。
- 拆股/缩股的税账连续性等既有 S1 简化不变。

## N3 修复轮（2026-10-08，worktree `wf_ff9d1264-f14-1`，基线 N3 tip `3d5bbd2a`）

非作者独立复核（`code-review` + 独立探针）确认 2 major、1 major、1 minor、3 项 code-review 行为缺陷与 1 项性能 note；本轮逐条 TDD 红→绿修复。**本节为并行双方各自登记的两段台账按最终代码核对后的合并记录**（同轮同向并行写入痕迹与收口裁定见下文「修复轮复核收口记录」「终版收口」两节；条目已按终态代码核对，尤其预留恢复校验的存废与注销面值 outstanding/current 口径）。

### 修复项（业务断言红→绿；红证据 `fix-round-red-engine.log` 8 项 + `fix-round-red-engine-2.log` 2 项、`fix-round-red-web.log` 3 项 + `fix-round-red-web-2.log` 1 项；编译红单独留 `fix-round-red-build.log` / `fix-round-red2-build.log` 不作红证据）

1. **[major-A] `install_correction` 换入更正 Books 后未刷新展示字段**：`finance_state.rs` 安装更正候选后先 `refresh_book_display()` 再 `validate()`——合法 +1 元收入更正此前被「账面展示现金或投资额与权威账簿重算值不一致」拒绝（main@5bdf0953 同输入 Ok 的独立对照实证回归）。测试：`correct_report_accepts_legal_income_correction_and_links_book_display`（成功路径：收入 +100 分 → 投资额 +30 分、现金 +75 分（税后））、`failed_correction_leaves_finance_and_library_unchanged`（不平衡更正原子回滚，finance/library 深等）。
2. **[major-B] 送股/转增来源重复占用**：送股与现金分红共享可分配利润预留池、转增按「资本公积余额 − 既有未入账转增占用额」在**声明时点**（`declare_*` 路径）显式校验，双批超占即拒（红用例：两笔各占满可分配利润的送股、6_000 送股 + 6_000 现金分红双批（可分配 6_300）、两笔 3_000 转增（余额 5_000）第二笔声明即拒绝；拒绝后合法链路声明→入账→月结不卡死）。**预留只在批准/入账时点判定、不作跨期不变量**（终态无 `validate_stock_source_reservations` 恢复期复核，Rust validate 与 Web parser 恢复时均不按当期来源重查未入账部分）：批准后亏损月/向下更正照常月结，已批准送股**入账为冻结义务**（不按当期利润复核，4103 转负合法——分配决议先于利润变动生效）；转增进账仍按当前余额复核（更正借记资本公积极端路径未统一为冻结义务，登记为已知边界）。未入账送转的预计股本计入 50% 免计提门槛核定的注册资本（保守口径，防批准后由自身增资重新触发计提而卡死）。测试：`bonus_share_declarations_reserve_distributable_profit_at_declaration`、`bonus_share_declaration_blocks_full_cash_dividend_double_spend`、`capital_reserve_conversions_reserve_balance_at_declaration`、`declared_bonus_reservation_does_not_block_later_loss_month_close`（批准后亏损月月结 + 冻结义务入账 + 可分配利润归零）。
3. **[major-C] Web 行为分录勾稽缺失**：`simple-finance.ts` 新增 `validateCapitalActionBooks`——与 Rust `validate_capital_action_entries` 同构的事实↔Journal 计数多重集匹配（删除行为凭证/篡改金额/追加多余凭证三负例此前 Web 接受而 Rust 拒绝；NonCash 参数化，8 旧断言保留全绿）。Web 存档始终经 Rust validate 产生，受理门禁由 Rust 承担，两侧恢复勾稽同构。
4. **[minor-D] cash_flow 分类**：Rust 行为分录匹配强制 `NonCash`（NonCash 改 Financing 此前 Rust 接受/Web 拒绝）；`JournalEntry::validate_invariants` 与恢复校验两侧同构。测试：`tampered_action_entry_cash_flow_is_rejected_by_restore_validation`。
5. **[code-review] 分红批准时点注册资本重构漏加回减资**：分红/送转/配股/拆缩股统一走双向 `capital_increase_after` 闭包（增资扣回 + 回购注销/缩股核减加回）；已结算配股/已注销回购缺金额改为显式拒绝（不再 `unwrap_or(ZERO)` 静默）。测试：`repurchase_cancellation_keeps_earlier_dividend_declaration_valid`（分红批准后回购注销 1_000 分，既有分红计划不再被恢复校验拒绝）。
6. **[code-review] 回购注销面值权威链**：核减面值读 `outstanding_par_value()` 在册面值链——按最近**已入账**（settled）拆股/缩股重锚解析，已批准未入账的重新计值不改变在册股份面值（注销与未生效拆股天然隔离，与声明次序无关）；无已入账重锚时退回全部一致的送转冻结面值，再退回「法定注册资本 ÷ 已发行股数」整除推导（不可整除显式拒绝）。不再取历史第一笔送转/配股冻结面值。声明链 `current_par_value()` 按最新声明重锚、未区分 settled/unsettled，为 main 既有口径，登记为已知边界。测试：`repurchase_cancellation_uses_current_par_value_after_share_split`（送转锚 100 分 → 1 拆 2 入账 → 注销按新面值 50 分核减）、`repurchase_cancellation_ignores_declared_but_unsettled_split`（拆股已批准未入账，注销 10 股仍按在册面值 100 分核减 1_000 分，不得提前按 50 分错核 500 分）。
7. **[code-review] 送股入账未计提法定公积金**：`StockDistributionFinanceFact` 新增严格必填字段 `statutory_reserve` / `reserve_basis_year`（未入账送股与转增必须为零；已入账送股与已批准分红共用年度计提及幂等——`distributable_profit` 的 `already_reserved` 汇总两者，同一依据年度计提额不重复，先现金分红后送股入账或反向均不重复计提）。送股分录变为借 4103（面值总额 + 计提额）／贷法定公积金（零不落行）／贷 4001；《公司法》第 210 条（2023 修订，2024-07-01 施行，gov.cn 全文本轮读取）10% 提取、50% 注册资本免计提门槛按「当前 + 未入账送转增资」保守核定。测试：`bonus_share_credit_accrues_statutory_reserve`（含幂等）。
8. **[code-review] 孤立核减额双向拒绝（all-or-none 合并条件）**：回购注销回填字段（`cancelled_shares`/`cancelled_on`/`capital_reduction`/完成回填）为 all-or-none——任一注销侧字段有值（含**孤立核减额**反向情形，独立复核发现）即要求全部齐全；Rust `validate` 与 Web parser 同构单一条件（不设重复的独立 reverse-check）。测试：`orphan_capital_reduction_without_cancellation_is_rejected`（Rust 三向：篡改存档注入孤立 `capital_reduction` 被拒）、simple-finance.test.ts「未注销的孤立股本核减额必须拒绝」（Web，红证据 `fix-round-red-web-2.log`——此前 Web 以 `cancelled_on` 字段不完整错误冒充拒绝）。
9. **[性能 note] O(n²) 多重集匹配**：Rust `validate_capital_action_entries` 改 BTreeMap 计数多重集（O(n log n)）；Web 侧计数 Map 同构（同内容事实按次数消费）。`validate_book_display` 深克隆登记 defer（候选：重算抽为 `&self` 纯函数返回二元组，消除整体 clone）。

### 迁移的既有断言

- `credited_bonus_distribution_posts_entries_reflected_in_next_reports`、`tampered_capital_action_entries_are_rejected_by_restore_validation`：送股分录现含法定公积金计提行（借 4103 = 3_000 + 2_700；贷 simple_statutory_reserve 2_700）——原断言编码的是「distributable_profit 要求提取但分录未贷记」的缺陷本身。
- `bonus_share_cap_uses_registered_capital_evolved_by_prior_credit`：入账计提 2_700 后第二次送股上限语义由「仅按上限核定」改为「上限核定并同分录计提」，断言核对计提额 2_700 与 2030 年度幂等（第二次 distributable_profit statutory_reserve = 0）。
- Web「送转入账演进注册资本后…」两用例：事实 fixture 补齐 `statutory_reserve: "0.00", reserve_basis_year: null` 与对应送股汇总分录（缺失行为凭证的状态此前冒充合法入账）。

### 过程验证

- 红证据：Rust 8 + 2 项、Web 3 + 1 项全部按预期业务原因红（编译红另存，不作红证据）。
- 受影响组逐组复跑全绿（各组 10000ms 外部 deadline）：`company::simple` 119（`fix-round-green2-finance.log`）、cash_dividend 116（含税账）、share_split、issuer_repurchase、rights_offering、stock_distribution、company_simple_session 36、session/corporate_actions/ex_reference_price/dividend_tax 组合（`fix-round-green2-*.log`、`fix-round-green-*-all.log`）。
- Web simple-finance + system 消费组 28 项（`fix-round-green-web-2.log`）；终态（04:59 测试文件终编后）复跑 33 项全绿（`fix-round-final-web-company.log`）。
- ts-rs typegen 156 项导出通过、`apps/web/src/types/generated/` 无漂移（`StockDistributionFinanceFact` 非 ts-rs 导出，新字段不触类型面；`fix-round-typegen-2.log`）。
- 终态全量复跑（单一写入者）见下文「终版收口」。

### 本轮边界与遗留

- 送股计提的依据年度金额与现金分红同池核销；同一年度先现金分红后送股入账（或反向）均不重复计提（`already_reserved` 双向核销）。
- 未入账送转参与 50% 免计提门槛核定属**保守预留口径**（按全部入账后注册资本计），非法定时点口径；登记为游戏简化。
- 转增进账按当前余额复核的不对称及更正借记资本公积极端路径登记为已知边界（trading-rules「送转预留与冻结义务」）。
- 声明链 `current_par_value()` 未区分 settled/unsettled 为 main 既有口径，登记为后续统一项。
- Web 侧未入账来源预留由 Rust 受理门禁承担（Web parser 只做勾稽不做受理）；两侧恢复勾稽（分录多重集 + 注册资本双向重构）已同构。
- `validate_book_display` 深克隆、分录构造三处重复（posting/校验/Web 重构）维持 N3 原登记的 defer。

### 修复轮复核收口记录（2026-10-08，`code-review` 非作者两轮 + 本轮处置）

第一轮 7 项发现、终版 5 项，处置：#1 冻结义务与「预留不作跨期恢复校验」已登记 trading-rules「送转预留与冻结义务」；#2 转增进账仍按当前余额复核的不对称及更正借记资本公积极端路径登记为已知边界；#3 孤立 `capital_reduction` 字段 Rust（触发条件 + 字段一致性双查）与 Web parser 双向拒绝；#4 system.rs/session.rs 陈旧 docstring 更正、finance_share_split 模块头更正（缩股入账有过账）；#5 Web NonCash 条件保留为防御性断言（parse 层已强制）。公司法 210/214 条 gov.cn 原文已读（2024-07-01 施行），复核日期 2026-10-08。engine lib 全量复跑：1575 过/257 败，失败集与 N3 基线逐项一致（`fix-round-green2-failures.txt` diff 为空）；`current_par_value` 声明链未区分 settled/unsettled 为 main 既有口径，登记为后续统一项。**收口时观察**：本 worktree 存在同轮并行写入者，最后快照中 `repurchase_cancellation_ignores_declared_but_unsettled_split` 与 `declared_bonus_reservation_does_not_block_later_loss_month_close`（含 `red-check` 占位错误）为对方在途 TDD 红测试——合并前须以单一写入者的最终全量复跑为准，两段修复轮台账（并行双方各自记录）已合并为上节单一记录并按最终代码核对条目（尤其预留恢复校验的存废与注销面值 outstanding/current 口径）。

### 终版收口（2026-10-08，单一写入者最终全量复跑；基线 `3d5bbd2a`，未提交）

主协调者裁定本 worktree 唯一实施者为 N3 修复 subagent；同轮同向并发编辑已合并（含注销字段 all-or-none 触发条件的合并形式，与独立 reverse-check 去重为单一条件）。上一节「收口时观察」所提两条红测试即本实施者的 TDD 红（`red-check` 为临时回退复现红的占位错误串，红证据取得后已移除）；红证据：`fix-round-red-engine-2.log`（亏损月被预留校验拒绝、未入账拆股面值错核减两项业务断言红）、`fix-round-red-web-2.log`（孤立核减额 Web 侧红：解析器已以「回购注销回填字段不完整」拒绝该存档，测试红因断言正则陈旧未匹配实际错误文案，正则修正后转绿——非 Web 真接受，终态两侧均双向拒绝）。终版全量复跑：

| 项 | 结果 | 证据 |
| --- | --- | --- |
| engine lib 全量（64 线程，300000ms 长验收） | 1578 过 / 257 败，**失败集与 N3 基线逐项一致（257=257，diff 为空）** | `fix-round-final-engine-lib-full.log`、`fix-round-final-failures.txt` |
| `company::simple` 全组 | 119 过 / 0 败 | `fix-round-green2-finance.log` |
| cash_dividend 47、share_split 19、issuer_repurchase 20、rights_offering 26、stock_distribution 35、company_simple_session 36 | 全绿 | `fix-round-green2-*.log` |
| 两条在途红测试（未入账拆股注销面值、批准后亏损月月结） | 转绿（终态源码直接复跑 2 过 / 0 败） | 终端复测 + `fix-round-final-engine-lib-full.log` 全量内含 |
| Web 全量（本 worktree，231 文件、8 分片，10000ms 级） | 544 项 / 536 过 / 8 败，**失败逐项对照 N3 基线（`web-full-final.log` 545/538/7）零新增**：7 个同名基线失败（利润表「当季」标签、本人交割单必填、移动KDJ 指标源、Worker×2 分配查询与 microtask restore、Tauri 分配查询身份）；+1 为 `file-target-permissions` 整文件 ENOENT（`apps/desktop/src-tauri/gen/schemas/acl-manifests.json` 未生成，N3 台账已登记的 desktop 构建产物基线环境项）；−1 为 N3 基线中时序敏感的「Tauri更正真实host命令携带generation」本轮通过（环境波动，双向均非本批代码差异） | `fix-round-final-web-full.log` |
| Web simple-finance + system | 28 过 / 0 败；终态（04:59 测试文件终编后）复跑 33 过 / 0 败 | `fix-round-green2-web.log`、`fix-round-final-web-company.log` |
| ts-rs typegen 156 + `check-generated-types.mjs` | 全绿 / 无漂移 | `fix-round-typegen-2.log` |
| tsc -b --force（apps/web） | 0 错误；终态复跑（早前 04:58 证据先于测试文件终编，已重跑覆盖）0 错误（日志 0 字节） | `fix-round-tsc-2.log`、`fix-round-final-tsc.log` |
| oxlint（本批 Web 两文件） | 0 警告；终态复跑同 | `fix-round-oxlint-2.log`、`fix-round-final-oxlint.log` |

docs 同步核对：trading-rules「送转预留与冻结义务」（outstanding/current 双口径）与终态一致；company-accounting §8「回购注销面值」原文残留中间态 `current_par_value()` 表述，已更正为 `outstanding_par_value()` 在册面值链并补记未区分 settled/unsettled 已知边界。两段修复轮台账已合并为单一记录（见上节）。终态导出（本目录）：`fix-round-final.diff`（修复轮未提交 16 文件）、`fix-round-final-branch-full.diff`（相对分叉点 main@5bdf0953 完整终态 49 文件、含 4 个新文件——合并入 main 的净贡献面；两 dot `git diff main` 会混入 main 侧 N1 提交反向差异，不适用）、`fix-round-final-manifest.txt`（逐文件 manifest 与证据索引）。fixtures 重生成仍按上文边界留待合入前主仓执行。未 commit——由协调者落地。
