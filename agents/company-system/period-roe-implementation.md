# 期间时间加权权益分析指标基础

## 范围与依据

本增量新增 `packages/engine/src/accounting/period_roe.rs` 及同目录测试，为 Q14 蓝图中的共同 ROE 指标层提供精确计算基础。它只实现“期间时间加权平均权益分析指标”，不实现、命名或宣称为法定披露“加权平均净资产收益率”，也没有接入 Simple、Simulation、报表查询、披露或消费者。

依据为 `agents/remaining-questions-and-features/q14-financial-model-design.md` §二.3、§二.5：比率须使用同期间、同报告范围利润和平均权益；有分红、增资、回购时须反映变动时点；各期间比率应按期间口径重新计算而不能相加。`docs/decisions/0035-company-system-simple-fundamentals.md` 也要求 ROE 留在共同指标层，并明确非正平均权益不可用。

按要求检查了 `docs/company-accounting.md`、其 `policy-sources.json` 及现存 accounting/report/equity 与 valuation 代码。仓库有会计列报与权益变动来源，但未找到证监会《公开发行证券的公司信息披露编报规则第9号——净资产收益率和每股收益的计算及披露》的官方原文、公式或适用区间登记。`agents/remaining-questions-and-features/corporate-actions-research.md` 所记录的发行条件资料提到某些发行条件的 ROE 门槛，但不是 ROE 计算公式来源。因此此实现不推断法定披露公式，后续正式披露接线前仍需补齐可核验的官方公式依据及适用对象。

## 口径与行为

- 报告窗口是自然日序列 `(start_exclusive, end_inclusive]`。期初权益表示 start 时点余额；权益变化自输入日期当日生效，所以 start 当日已包含在期初余额、不得重复输入，end 当日计入期间最后一天。窗口必须至少跨一个自然日。
- 期初权益、期间净利润及每项有日期权益变化均携带 `ScopeId` 和 `EquityBasis`。支持 `Total` 与 `AttributableToParent`，不允许用归母利润搭配合并总权益，或将其他范围/归属的变化混入平均权益。现有 `ScopeId` 本身只区分单体/合并范围，故新增 basis 标签补足总权益/归母权益维度。
- 当前报表层未提供单体归母权益来源；调用方不得构造 `ScopeId::Standalone` + `EquityBasis::AttributableToParent` 并宣称得到有效归母披露指标。
- 平均权益按带日期的权益余额乘其覆盖期间自然日数求和，再除以窗口天数；同日变化先整体合并，不依赖输入顺序。带符号分值按 `AccountingAmount` 元（内部为分）处理，不使用浮点数或显示舍入。
- 期间净利润是同范围同 basis 的比率分子，不另行假设其在期内均匀形成，也不自动将利润摊进平均权益。调用方若有留存利润或其他权益变化的实际生效日期，应作为有日期变化传入。此边界避免以未经依据确认的 `净利润 / 2` 假设冒充会计政策。
- 平均权益为零或负数返回明确的 `Unavailable::NonPositiveAverageEquity`。无效窗口、范围/basis 不符、变化日期越窗和精度超限均为有类型的错误，不伪装为指标不可用。
- 平均权益和 ROE 均以精确有理数返回：平均权益为“加权分值日 / 天数”，ROE 为“净利润分值 × 天数 / 加权分值日”。不跨期求和 ROE。临时同日求和使用 `BigInt` 消除输入顺序导致的伪溢出；权益余额及最终结果仍校验回 `i128` 范围。

## 复用调查与接线边界

- `packages/engine/src/strategy/fundamental/valuation.rs::equity_roe` 是 NPC/分析估值函数：接收 `AnnualFacts`，内部开/期末权益简单平均并导出估值情景；它不是供公开报表复用的期间比率接口，也没有有日期权益变化或总额/归母 basis 输入，故本增量不重复调用或修改它。
- `packages/engine/src/accounting/reports/equity.rs` 生成 `EquityStatement`，单体提供期初/净利/期末权益；合并仅列归母权益与归母/少数损益字段。它不是时间加权 ROE；当下普通报告快照没有完整的带日期公司行为序列，因此不能仅凭期末报告构造这里的分析结果。
- `accounting/mod.rs` 由父任务注册模块。本实现的调用方应从同一个 `ScopeId`、`EquityBasis` 的报表/账务事实构造有范围金额，并提供窗口内实际生效的权益变化；在可取得上述日期事实前，不应静默以期末权益、简化净利润均分或跨期间 ROE 替代。

## 验证证据

- 红测日志：`.tmp/company-system/checklist-common/basis-red.log`。同一模块的六项原有测试通过，新增总权益期初搭配归母净利润的 basis 负例按预期失败（期望 `EquityBasisMismatch`，实现当时未校验归属）。这是真正的行为失败，不是编译错误。
- 首轮绿测 `.tmp/company-system/checklist-common/roe-green.log` 是修复归属校验后的 7/7。之后在原有测试函数中追加了期间外 `end+1` 和净利润 Scope 不匹配断言（未新增 case）；fresh 编译 `.tmp/company-system/checklist-common/build-final.log` 成功，最新定向绿测 `.tmp/company-system/checklist-common/roe-fresh-green.log` 仍为 7/7、0.00 秒，并受 10 秒 supervisor 监督。当前源码应以 fresh green 为验证证据。
- 定向文件 `rustfmt --check` 与 `git diff --check` 通过。独立复核记录见 `agents/company-system/period-roe-review.md`；未运行全量回归。
