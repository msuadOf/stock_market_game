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
