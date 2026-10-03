# 三份领域记录全文 EOF 独立复核

- 审阅日期：2026-10-03。
- 审阅者：`/root/luna54`，未参与实现；本记录仅审阅指定三份工作记录及其对应现行 caller / 契约。
- 产品基线：`08e4fc75b52a71a3262a8a938c57b44f8b5b4960`。记录所述源码基线为 `b89afb3346743a4b4fccf26c9ac9ff108595f696`。
- 输入全文行数：`experience-review.md` 93 行，`final-summary.md` 23 行，`industrial-result.md` 52 行；均自首行读至 EOF。审阅期间没有产品源码写入、Git 写入或测试运行。
- 结论：没有发现能归因于本批重构且要求修改产品源码的有效问题。旧结论中应严格限定的范围、机构/零售边界、税务年度口径和重复 API 风险如下逐项复核；这些边界不能用“静态通过”扩写为全域行为通过。

## 章节矩阵

| 文件章节 | 原结论复核 | 当前证据与限定 |
|---|---|---|
| `experience-review.md` §结论、§EXP-01 | 结论仅限静态审阅，EXP-01 的真实观察 caller 漏接已修复；未声称全域通过，成立。 | `roots.rs` 机构观察调用旧 facade；`institutional_behavior.rs` facade 委托 `observe_institution_position_dated`。观察和 stale 清理均已实际接入。 |
| `experience-review.md` §门禁一：大 A 语义与依据 | 将审阅范围描述为持仓经验、零售策略、指标行为而非修改交易制度，成立。不要把这段当作重新核验官方规则。 | 金额/数量单位和申报边界沿用既有模型；本批未新增 A 股制度。规则依据沿用文档登记日期，未重查官方材料。 |
| `experience-review.md` §门禁二：必要性与最小范围 | N01/N28/N34/N35/N40 与 N07 owner 迁移描述大体成立；“N07 是 6 个 writer”需与随后全量细项合读，不能误当只有 6 个接线。 | §N07 表有 6 个主要 writer，另明确记载 `observe_institution_position_dated` 的 legacy 观察；新 transition 还涵盖相应内部写入组合。当前 caller 分别经真实结算投影及 root 观察接入。 |
| `experience-review.md` §门禁三：边界、跨层与复杂度 | 断言限于静态核对，测试未运行，措辞成立。 | 尤其是“未定位 public-history-read 生产 writer”与实际接线检查范围一致；不得把 Watchlist / PriceMemory 所有读写宣称为已有生产闭环。 |
| `experience-review.md` §范围与证据限制、§N07 全量细项、§SHA256 | 证据范围和 SHA 绑定说明明确；action-index 路径在该 worktree 不存在，无法由本轮独立复核其第 145–199 行原文。 | 本轮直接核对了报告所列源码与当前 caller；action-index 的逐字对照只能视为原审阅者证据，不能声称我已复核原文。产品 merge 后仅审计材料有变更，不能因此推定产品实现变更。 |
| `final-summary.md` 全文 | 104 短 case、构建、检查及并发参数属于协调者汇总结果，不是本 reviewer 的执行结果；报告自身注明完整回归未运行，准确。39 动作完成的总括必须按其指向的各 reviewer 证据解释。 | 本轮未重跑也未独立验证这些测试结果；独立领域判断只覆盖下述指定材料与 caller。 |
| `industrial-result.md` N13 | owner、初始化 caller、首错顺序和既有边界描述成立。 | 未扩展审查开局校验或会计模型；本轮只以正式契约抽查所得税相关部分。 |
| `industrial-result.md` N14 | LoanPortfolio 字段 owner 与 Industrial dispatch caller 描述成立；不将 BankBooks 当成该组合 caller。 | `company/operations/dispatch.rs` 的 Industrial 分支为运行时计息接线；记录已说明 BankBooks 误归属历史修正。 |
| `industrial-result.md` N16 | IncomeTaxPosition 所有权迁移、公开 `accrue_income_tax` 的编排和“暂无经营 dispatch caller”描述成立。重复调用行为如实锁定了基线，不能据此说逐年防重已实现。 | 当前 `rg` 结果显示唯一产品定义在 `expenses.rs`，运行调用只在短测和既有税务测试；没有业务 dispatch 调用。契约对年度基础有定义，未找到要求 `accrue_income_tax` 必须于 12 月 31 日调用或提供幂等键/年度防重的条款。 |
| `industrial-result.md` N06、验证与未完成 | policy receiver 调整和“不运行本 worker 测试”记录与源码说明相符；测试、规则官方复核结果不由本轮背书。 | 税法和 CAS 18 原文取证阻塞应继续保持显式，不从静态迁移推导真实税率或准则合规。 |

## 关键原文与 caller

- `experience-review.md:15-21` 记录 EXP-01：初次新增机构 owner 没有生产写入，修复后 facade 调 owner。当前 `packages/engine/src/session/decision_chain/roots.rs` 对已有机构持仓调用 `institutional_behavior::observe_institution_position`；该 helper 最终委托 `RetailExperienceState::observe_institution_position_dated`。开局持仓经 `initialize_institutional_holding_dated`，真实机构成交经 `session/pipeline/retail_projection.rs` 的 `record_institutional_fill_dated`。因此“新 owner 有 caller”可成立。
- 机构成交使用独立 writer：它记录机构真实费用、观察和退出，不调用零售 `record_fill_dated` 的亏损确认/冷静期策略。`.record_institutional_fill_dated` 对加仓/减仓要求已有 epoch、首笔买入不允许旧 epoch，清仓移除 epoch 并记录退出；没有套用 Retail 冷静期。机构观察按传入冻结 policy threshold 计算失败，不增加零售连续失败计数。此语义与 ADR-0026“机构不套散户固定冷静期或固定不利价格门槛”、文档自身范围一致。不得将共享 `RetailExperienceState` 命名或 transition owner 误判为机构错误核销 Retail 行为。
- `industrial-result.md:34-35` 的税务文字可由当前源代码核实：`accrue_income_tax(date)` 用 `date.year()` 查询全年期间索引，生成过账日期仍是传入 `date`，无日期为 12 月 31 日的守卫，也没有已计提年度标记；该 API 目前没有经营 dispatch caller。`year_pretax` 汇总 1–12 月全部 `AccountElement::Expense`。重复负税前调用中，首次递延税分录会记入 `TAX_EXP`；该科目为费用，因此第二次同年调用的税前基数从 -400 变为 -300，随后再加一项 -300 亏损。`ownership_tests.rs::repeated_annual_tax_keeps_existing_non_idempotent_loss_behavior` 明确锁定这一行为。
- 正式契约基准：`docs/superpowers/specs/2026-09-13-company-information-learnings.md:543` 将所得税描述为年度 `year_pretax`（1–12 月期间索引）及游戏简化；`docs/company-accounting.md:60` 明确 CAS 18 原文取证受阻。现有契约/本次 N16 迁移目标未声明重复调用幂等、年度唯一计提、仅年末日期准入，故不能把缺少这些 API 能力判为本次重构的有效回归。重复调用和非年末调用确有行为风险，应由后续正式产品契约决定是否拒绝、设计年度结账时序或引入唯一性状态；不应伪装成已由本报告解决，也不应把该旧行为当成真实税务语义。

## 旧结论复核与新候选

| 候选 | 旧结论 / 原文 | 复核判断 |
|---|---|---|
| “institutional fill 应共用 Retail 冷静期或失败计数” | `experience-review.md` 明确称机构失败无零售连续失败计数/冷静期；ADR-0026 明确机构不套散户固定冷静期。 | 反证成立，驳回候选。机构单独记真实费用和机构观察门槛有正式领域依据，不应按零售逻辑核销。 |
| “年度所得税必须拒绝非年末日期” | N16 文档称年度所得税，但生产 API 参数仅为 `CivilDate`，没有 12 月 31 日守卫；没有经营 dispatch caller。 | 未找到正式契约规定计提只可在年末调用。记录仅声称 API 暴露且暂无调度 caller，属准确披露，不是声称日流已完成。作为产品契约空缺登记为后续候选，不归因此次 refactor。 |
| “重复同年 `accrue_income_tax` 必须幂等/拒绝” | 实施记录和测试明确承认非幂等；第二次会把已入账所得税费用算进 `year_pretax` 并新增亏损。 | 这是可复现的旧 API 行为，不是此次 owner 迁移引入；但现有正式材料没有 exactly-once / 幂等约束，故不判本批有效 finding。它也不能被描述为已核实的 A 股企业所得税年度语义。若产品希望支撑任意重复调用，需先正式定约并单独修复/测试。 |
| “工作区路径或验证说法不实” | `experience-review.md` 引用 action-index，`final-summary.md` 报告指定短测结果。 | action-index 文件在本 worktree 不可见；测试结果仅能认定为文件中的既有汇总，本轮未执行。两点均明确记录限制，不据此下反向结论。 |

## 独立结论

三份文档叙述的范围、caller 和限制整体自洽。当前未发现新的有效产品缺陷或需要修改这三份记录的事实错误。特别是：机构路径不套 Retail policy；所得税是按年份读取全年索引、当前没有生产日流；计提日期约束和重复 API 幂等性都没有可据以判定重构违约的正式契约。后两者是应由产品层另行明确的接口语义风险。本轮没有核验官方税法、CAS 18，也没有运行编译或测试。
