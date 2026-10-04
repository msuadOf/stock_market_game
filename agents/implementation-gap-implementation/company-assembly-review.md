# G28 / G36 公司装配独立复核

## 范围与依据

- 复核者未实施本批业务改动；本记录仅审查，不执行 stage / commit。
- 工作树：`.worktree/implementation-audit-final`；比较基线为本轮现有 HEAD，完整业务 diff 包括 `session/company_assembly.rs`、新增 `session/company_groups.rs`、consolidation、reports、真实 session 测试，以及共享 `session.rs` / `persistence.rs` / `disclosures.rs` 与 Web save parser 对应 hunk。
- 已读 AGENTS、principles、architecture、open-questions、testing、ADR-0016、公司会计覆盖矩阵及原审计 G28/G36。原需求是四行业可自定义并经真实日终、封账、披露和公开查询闭环，以及固定集团的合并产物公开链；默认 Industrial 或纯报表内核通过均不能替代。
- 会计依据沿用正式文档已登记的 CAS 33（财会〔2014〕10号，2014-07-01施行）与各行业来源，本轮未独立联网复取官方全文。固定单层、精确基点控制及 ConsolidatedRestatementUnsupported 保持既有游戏简化，未改 A 股撮合、T+1、分/股单位或股东侧现金流。

## 当前结论

| 缺口 | 结论 | 说明 |
| --- | --- | --- |
| G28 | 最终主体通过 | 本记录发现均已修复并复测；真实内部单批 sale / 部分外售 / 月末五产物 / restore / 公开披露及三个真实 save Web bridge 已通过，明确保留已登记的装配简化。 |
| G36 | 最终通过 | 四行业真实新局、封月、查询、restore 后继续日终及 Web bridge 已通过；追加真实 Bank / Insurance 非空适用公告消费链短例独立 2/2 通过，不依赖 quiet fixture 或空 all()。 |

## 已报作者与主控的发现

1. `eliminate::balance_entry` 仅逐声明检查金额不超过 `(member, account)` 总余额。一个 root 的 AR 为 100、对两个不同 subsidiary 分别声明 80，双方 AP 各为 80，每 pair 检查仍通过，累计抵销却使 root AR 为 -60。既有 D6 只禁止同成员对重复，不解决多对手方累计上界。需要累计余额上界及顺序对照短测。
2. `consolidated_window` 的 bounded Books 仅截取报告结束月上界；`minority::adjusted_economics` 的 ledger.net_income() 仍包含此前年度损益，而 `income` 对照当年累计窗口。真实两年前史非零经营时，存在跨年损益漂移/勾稽拒绝风险。必须用真实集团新局或跨年手算窗口动态核验，不以仅 2029 OpeningBalance + 2030 单月业务金样证明通过。
3. `seed_groups` 对历史报告使用已推进到开局的 `TradeOpenLedger::open_amount`。历史内部往来若已结付，历史申报会丢失；若开局当前余额超过历史金额，bounded 历史账簿会因上界检查拒绝。集团测试当前没有真实内部 AR/AP，也未证明往来资产身份与期间对齐。
4. `session_company_assembly` 当前仅新局运行，最后 restore 后比较 civil_date 或公开列表数量；未在 restore 后继续日终与到期披露。mixed fixture 是 Industrial + Insurance，缺真实内部往来、保存后集团关系/持股/往来身份不变、混行业 source report key 与重述边界的明确断言。

第 2 项已独立动态确认：真实 mixed group 新局在前史报告生成时返回 `consolidated income mismatch: derived 44732.01 vs task-12 output 188103.24`。其他项目仍为静态调用链发现，未声称已动态复现。有效发现已交作者修复并要求再次复核。

## 独立短验证

- `timeout 10s .tmp/gap-target/debug/deps/consolidation-b42d52ef2fc76e7b --test-threads=8`：26/26，通过，0.01 秒。
- `timeout 10s .tmp/gap-target/debug/deps/industry_reports-ab5fe406c3227d90 --test-threads=8`：19/19，通过，0.02 秒。
- `node scripts/run-with-deadline.mjs 10000 -- node --test --test-timeout=10000 --test-concurrency=8 apps/web/src/save/company-contracts.test.ts apps/web/src/save/company-books-schema.test.ts`：两个测试文件通过，约 0.25 秒。
- 三条短命令并发执行，Rust 各 8 test threads，Node concurrency 8。未执行完整回归、长验收或构建；已有 binary 的 green 仅证明当时生成版本覆盖的既有病例。
- 追加 `timeout 10s .tmp/gap-target/debug/deps/session_company_assembly-778a4441f743ca77 --test-threads=8`：1 通过 / 1 失败，4.47 秒。四行业真实新局日结及 restore 通过；mixed group 在 `GameSession::new` 前史合并报告生成时失败，未进入日终公开阶段。
- Web parser 对显式 company config、groups shape、重复成员、股份安全整数及省略 setup 字段保持/拒绝已有短测；深层成员映射、控制比例和 setup/save groups 一致性仍由 Rust authority 校验。尚未用真实四行业/集团引擎输出跨 Rust/Web parse 往返，不能宣称跨层契约完成。
- Web 增量 `trade_counterparty_events` shape 使用 `(event, counterparty, account)` 身份，不保存可从账簿派生的 amount；exact parser 明确拒绝缺失字段、错误 event 类型与额外 amount。此次只核阅 parser diff；Rust owner 仍在接线，不能将旧 JSON fixture 补空数组称为合法恢复证据。

## 收尾门禁

等待作者修复及最终文件名单 / green。再次复核须核验完整修订 diff，并逐 G 更新结论；未达门禁不得宣称改动完成。

## 增量静态复核（待 green）

- 多对手方上界已改为累计 `(member, account)` 校验，并新增正反申报顺序用例；当前未运行重新编译后的 binary。
- 报表窗口已分别累计 member YTD 与本报告 window NI，少数损益使用窗口金额与同一 ownership，Equity 使用 window 口径；静态修复方向符合现有会计窗口语义，仍待跨年及真实集团短测。
- 新增 `TradeCounterpartyEvent` 只保存 Journal source / counterparty / account 身份；赊销、收款、核销、赊购及付款入口登记身份，group request 按报告期间从 Journal 派生余额，不再读取当前 open_amount 代替历史余额。identity 的缺失、重复、未知账户/对手方在 Industrial serde 边界显式拒绝。
- 补报 shared owner：直接内存 SaveSlot 的 standalone Industrial 缺少上述 identity validator，需与 JSON decode 保持一致；group 成员已有 request 校验不代表独立公司也覆盖。
- 补报业务 owner：OpeningBalance trade 余额当前略过身份校验，custom group 开局内部 trade 余额缺少对手方事实时不能静默宣称完整抵销，需要在既有 scope 内明确支持或拒绝。

## 第二轮独立短验证与剩余阻断

- 修订后的真实 `session_company_assembly` binary：2/2，通过，7.11 秒，10 秒 deadline / 8 test threads。四行业 restore 后继续日终、mixed group 月末封账后 restore 并推进到真实排期披露均通过；更正 Public DTO 的期间末日字符串断言属修正错误测试契约，不是弱化断言。
- 新增多 pair 金额累计上界正反序用例包含在 `consolidation` binary：27/27，通过，0.01 秒；既有 single pair 上界和恒正拒绝仍在。
- `engine` lib 中 `session::company_groups::tests::historical_group_balances_survive_real_trade_settlement_and_restore`：1/1，通过，0.01 秒。真实赊销/赊购/收付后，历史 March 113 两侧申报仍存在、April 清零；CompanyOperations serde 往返后历史 report 全等。
- 真实引擎输出已导出至 `.tmp/company-assembly-review/four-industries.json` 与 `.tmp/company-assembly-review/mixed-group.json`，已交 Web parser owner 做实际 bridge，尚待结果。
- **剩余 G28 阻断：内部销售 / 存货资产身份未派生。** 上述真实历史用例中 root 向 sub `sell_credit`，sub 从 root `purchase`，但 `company_groups::request` 仍固定 `intercompany_sales: Vec::new()`。因此内部收入、成本及尚未售出库存的未实现利润不会抵销。AR/AP 已抵销不等于五类合并产物正确；原 Task12/26 声明由交易与存货批次派生 IntercompanySale，当前没有获批新简化支持静默空清单。已报作者与主控，等待修复或明确报告未完成，G28 仍不放行。
- Opening trade 余额已在 group request 显式拒绝为 unsupported；未扩大普通独立公司 OpeningBalance 接受集合。该边界需要正式文档登记与短测试，不能把内部销售缺失一并核销。

## 内部销售修订复核进展

- root 确认至少一条真实 Session / SaveSlot restore 携内部单批 sale 与部分外售、再月末 group 报告及公开披露的组合短 fixture；不以 group helper 单独核销完整生产闭环。
- 作者新增 InventorySourceEvent 身份及 derive_sales；原 `intercompany_sales` 空清单路径已改为真实源配对。必要身份事实只保存 event / item / account，成本读取 Journal，采用已批准 D5 均匀摊法，不新增 FIFO。
- 首次静审补发现：遗漏真实外售 issue identity 会伪造 unsold；未来另一个 item 同 account 会错误阻断历史 cutoff。作者已补 Journal 存货移动完整身份覆盖、非 Opening `(event, account)` 唯一及 cutoff 后歧义检查。
- 更新后的 historical group 短测独立 1/1 green，0.01 秒。真实单批内部 sale 200、部分外售 160 的报告手算：Cash 1774、经营 CF -226、NI 110、少数 NI 12、少数权益 212、assets 2104.80、liabilities -5.20、equity 2110。VAT input/output 沿用已有 TaxesPayable 净负债呈列，未改会计分类；删除出库 identity JSON 反序列化明确拒绝。
- 尚待真实 Session 内部 sale 公共入口 fixture / 最终 bridge、正式不支持边界登记、直接内存 SaveSlot inventory/trade identity validator 一致性及最终完整 diff 复核；当前不改变 G28 待放行状态。

## G28 最终复核

- 完整必要范围包含初始文件清单，以及 `accounting/{inventory,mod}.rs` 半偶分函数的 crate 内复用、Industrial 的两个 source 身份模块与 production / purchasing / sales 接线、group sales / tests、shared session / persistence / disclosure / hash 对应 hunk、正式公司会计 §8 与 trading-rules 的简化登记、Web parser / generated bindings。未修改撮合、T+1、股东侧资金或为测试增加新 FIFO 模型。
- 独立最新 lib 历史 group case：1/1 green，0.01 秒，含明确 `ConsolidatedRestatementUnsupported`、未来同账户另一 item 不污染历史 March、缺出库 identity 拒绝与五产物手算。
- 独立最新真实 Session binary：3/3 green，7.14 秒；普通测试使用 10 秒进程 deadline 与 8 threads。内部 sale fixture 克隆保存的真实前史账簿再追加真实 purchase / sell API 交易，不替换为缺历史的空账簿；经公开 restore、月末 group Quarter、排期公开查询，验证内部未售利润扣减与库存成本。披露后再 save / restore / end_day，原 publication ID 仍可查询。mixed scope 两公开 ID 显式不同，closing key 分 Standalone / Consolidated。
- 三份含新版必填库存身份的真实输出（four-industries / mixed-group / internal-sale-group）由 Web owner 执行 `parseSaveSlot` 与 `assert.deepEqual` 完整往返，并逐文件删除 identity 字段做拒绝负控：均通过，共享 10 秒 deadline，约 0.74 秒。独立 Web reviewer 同样核收最终 bridge 与 generated bindings；旧 projection JSON 不冒充合法集团恢复。
- 直接内存 SaveSlot 对全部 Industrial 调用 trade / inventory source validators，和 serde JSON 校验一致；groups / setup 相等、spec / issuer / 股本、行业 active shock 的恢复守卫已核阅。真实省略 setup 字段保持 Rust/Web 同步，新严格 root 字段不做旧档迁移。
- `session/hash.rs` 将 groups 纳入 business state hash，shadow / save / restore 保存固定持股身份；新增 source facts 由既有完整 operations hash 保存。AccountKeys 在 aggregate / worksheet / windows / classification 使用同一映射，保险与工商同码异义不混列，既有分类冲突拒绝与 standalone 重述短测保持。
- 正式 §8 诚实登记单批唯一内部赊销配对、均匀成本扣减、Opening trade / 同账户多 item / 多批 / 跨年旧未售 / 非工商商品买方等 unsupported 边界；无集团不伪造空合并，合并重述仍显式拒绝。官方 CAS 33 范围与少数列示依据另由独立核验 `report-scope-policy.md` 提供，估值 scope / 归母现金流策略问题交其对应 owner，不将其称为已由本批报表装配修复。
- G28 本批有效发现已修复并重新独立复核，允许按实施与代表性短验证范围核销；未运行完整回归、长期市场验收或浏览器矩阵。
- 最后 test 增量重导出的 reviewer-final 与作者 export-final 两目录共 6 份真实 JSON，Web owner 再次逐份完整 parse/deepEqual 并逐份删必填库存身份负控：12 项均通过，共享 10 秒 deadline，1.65 秒。最终 test 绑定不再依赖此前 v2 输出；限定产品 diff 的 `git diff --check` 通过。

## G36 联合最终复核

- operations owner 诚实确认旧 stress sampling 的 all() 可能空成立，原 quiet 四行业 fixture 不能证明公告适用性；按本次发现追加 `company_operations/industry_sessions.rs`，只使用真实 Session 新局 / 自然日日结 / Event publication ID / PublicLibrary 公开查询，不构造公告期望替代生产调用。
- Bank 使用确定性短 seed 选择，真实已激活当日适用信用材料不为空；Insurance 代表性 1 group / coverage 2 的前史 fixture 缩小重复经营，真实 demand / credit 材料不为空。两 case 都明确要求日终公告非空、全部 applies_to(kind)，并排除 ProductionInterruption / AssetImpairmentSignal；Bank 进一步要求实际经济材料全部 CreditDeterioration。PaymentFailure 是独立付款事实，不误当可注入经济冲击。
- 独立运行 `timeout 10s .tmp/company-ops-target/debug/deps/company_operations-4a14711434dbcaa4 industry_sessions:: --test-threads=8`：2/2 通过，1.26 秒；不是仅核收实施方日志。
- 行业 `books_mut` 四分支、sample / injection / disclosure / restore 对应 applies_to guard 与实际非空公开入口证据一致。G36 本批独立复核无遗留阻断，可按代表性短验证范围核销；G28 同样最终通过。
- 未执行完整回归、长期经营分布验收或全部会计准则覆盖验证；本结论不宣称真实 A 股 / CAS 全面合规，不核销其他 owner 的估值 scope / 现金流假设边界。本 reviewer 仅写本审查工作记录，未实施产品代码、改总账、stage 或 commit。

## 根报告归母字段增量（原 PASS 范围之外）

- root 新增必要跨层要求：Consolidated 报告缺 `income.net_income_to_parent` 时，不能让估值抽取把集团 NI 默认冒充归母 NI。`reports/{validate,error}.rs` 增量只拒绝上述非法 source，返回携 scope / period 的 `MissingParentIncome`，不改变合法 Standalone 的 None，不补 0，也不改已有 cross-foot。
- 作者实际实现前短红：`parent_income` 两例为 Standalone 正控通过、Consolidated 删除归母 NI 负控失败，`expect_err` 得到 `()`，0.00 秒；不是编译错误。实施后本 reviewer 独立负控 1/1 与 industry_reports 21/21 通过，0.02 秒，10 秒 deadline / 8 threads，限定 diff-check 通过。
- 该生成 / `ClosingEngine::record` source guard 增量静态与动态复核通过。root 随后批准同一条件的 closing_registry serde / 直接 SaveSlot 恢复增量，仍待作者 ready 与再次复核；不能将这个窄 source 条件声称为全面修复 Q17 的 ClosingEngine 恢复不变量。
- closing 增量已静审：原始 `EngineSave.versions` 每个 ReportSet 在 `Self::from` 重建 key 前执行同一个 presence guard，避免后续重复 key 的合法 row 覆盖早期坏 source 而隐藏错误；内部 helper 与直接 SaveSlot 调用只扫描 parentNI 条件，不顺带检查 Q17 的其他 key / version / restatement 不变量。
- 新 serde source guard 实施前真实红：独立 integration fixture 将正常登记簿 JSON 的 Consolidated parentNI 删除（并另测后接同 key 合法 row），旧 Deserialize 返回成功，`expect_err` 失败、exit 101、0.00 秒。实施后 reviewer 独立 `industry_reports` 22/22 green，0.02 秒；lib `closing_restore_` 三例（缺 parentNI 拒绝、重复 key 不遮错误、合法 Some(0)）3/3 green，0.01 秒。各 10 秒进程 deadline / 8 threads。
- 当前尚待 standalone None 的 Closing serde 正控与直接 typed SaveSlot 负控最终构建、实际 test-list 与 green；没有将旧 binary 的 0 case filter 当作通过。
- **该增量最终 PASS**：最新实际 binary 独立并发运行 Closing 四例 4/4（0.00 秒）、精确 `session::persistence::direct_save_slot_rejects_missing_consolidated_parent_income` 1/1（0.39 秒）、industry_reports 22/22（0.01 秒），各 10 秒 deadline / 8 threads，非 0 case。直接 SaveSlot case 先验证原正常 slot restore 成功，再替换为 cfg(test) 实际坏登记簿，不经 serde 绕回，明确检查 InvalidSave 的 closing registry 与缺字段上下文。
- 最终完整增量包括 `reports/{error,validate}.rs`、`closing/{mod,save}.rs`、`industry_reports/consolidated_gold.rs`、shared persistence 精确 helper 调用与直接 case、正式 §8 必填字段说明。静态检查未发现无关扩张或 silent fallback；合法 Standalone None 与合并 Some(0) 正控通过，raw duplicate row 无法掩错误。限定 diff-check 通过。
- G28 / G36 原主体与本次新增归母 source / 恢复条件均已完成非作者独立复核；Q17 的版本键、scope tuple 与完整重述恢复问题保持原边界，未由这个窄检查核销。
