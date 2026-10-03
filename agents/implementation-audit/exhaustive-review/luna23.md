# Luna23 独立全文复核：task 11–13

复核对象：merge `08e4fc7`（当前 HEAD `a7c7ce3`）；全文读取 `.omo/evidence/company-information-npc-intentions/task-11-review.md`（332 行）、`task-12-review.md`（168 行）、`task-13-review.md`（141 行），包括原始 REJECT 与全部复核/修复章节。另读仓库 `AGENTS.md`、`docs/principles.md`，并跟读当前 engine 调用者和 08e4fc7 后现存实现。未改产品代码，未运行测试或长任务。

## 章节覆盖矩阵

| 文档章节（原文标题） | 覆盖 | 当前复核结论 |
|---|---|---|
| task 11：§1 Isolated execution record、§2 AGENTS.md gate questions、§3 K3 red-line audit | 全章 | 旧数值/测试结果仅作为原复核证据，不在本次重跑；语义链、调用边界复查无新问题。 |
| task 11：§4 Registered-deviation adjudication、§5 CRITICAL provenance adjudication — CAS 17 | 全章 | 旧 REJECT 的登记门禁与 070ee58 修复链前后一致；当前代码/登记相符。 |
| task 11：§6 Hand-verified gold-chain math、§7 Non-blocking observations、§8 What this REJECT does and does not contest | 全章 | 手算只复核算式与既有结论；cas-8 并行登记问题仍为观察项，不能据此宣称政策来源已经统一。 |
| task 11：§9 Re-verification after fix 070ee58（含 9.1–9.3） | 全章 | 修复覆盖原门禁，APPROVE 可保留；此处历史测试结果不等同于本轮验证。 |
| task 12：§1 隔离执行证据、§2 阅读范围、§3 AGENTS.md 三问 | 全章 | 声称的证据/代码范围完整；旧 F1 登记门禁与当前登记一致。 |
| task 12：§4 发现与裁定、§5 指令 §4 红线探针清单、§6 复核后处置建议 | 全章 | F2 重复成员对申报现已在构造 worksheet 前拒绝；F6 声称调用方金额须为可信派生事实，仍是对外 API 的输入信任边界。另发现遗漏见下。 |
| task 12：Re-verification after fixes 85115a7 + a1b8fc3（含 1–5） | 全章 | F1/F2/F3 的修复证据及状态闭合；F5 明示非强制的测试缺口仍非阻断。 |
| task 13：§一 隔离重跑结果、§二 AGENTS.md 三问 | 全章 | F1 持久底稿的实现及后续调用路径确认；发现修复顺序引入/保留跨层事务非原子性。 |
| task 13：§三 BusinessKind 标签迁移债、§四 计划验收项逐条核对 | 全章 | 报表不消费 `BusinessKind` 的推理与现代码一致；“closed/annual correction lifecycle”旧结论需由下列事务失败路径限定。 |
| task 13：§五 转为 APPROVE 的必要条件、§六 复核环境清理 | 全章 | F1–F3 修复链齐备；清理记录仅说明当时复核环境，不作为当前 worktree 状态证据。 |
| task 13：Re-verification after fixes ceadac7 + 52b0d16（F1/F2/F3、隔离重跑、其他核对） | 全章 | 重述后续期间利润表泄漏、简化登记、溢出静默默认均有修复；新增的原子性风险不在该修复的测试断言中。 |

## 旧发现与实现复核

| 任务 | 原文旧发现 | 本次逐点复核 |
|---|---|---|
| 11 | CAS 17 资本化实现与 test-locked blocked 注册矛盾 | 已由 070ee58 将版本化游戏假设登记入 fixture/docs，保留 CAS 17 原文未取得状态与免责声明；旧 REJECT 转 APPROVE 合理。 |
| 11 | 项目数上限、借款计息窗口、暂停/完工、预售/交付/尾款、存货减值、现金流分类、事件 id 与拒绝原子性 | 原报告给出直接函数/金样定位；当前 `company/real_estate` 的项目、合同、借款状态机仍有显式类型化守卫。任务 11 的逐笔结算结论不能自动外推到 ClosingEngine 的跨组件事务（见新发现）。没有找到可据当前 caller 认定的新增 A 股规则漂移。 |
| 12 | F1 六项合并口径未登记 | `a1b8fc3` 文档/fixture 登记后闭合；当前调用仍明确采用固定集团、单层、整 bp 等已登记简化。 |
| 12 | F2 同成员对重复申报曾静默双抵销 | `eliminate.rs` 当前先计数并拒绝第三笔/同向重复，再按反向申报配对；原触发形态不能再走旧双配路径。 |
| 12 | F3 测试文件纯行超限；F4 计数措辞；F5 五错误无独立用例；F6 申报金额不以账面余额为上界 | F3 登记已在原复核处置。F4/F5 仍是非阻断记录。F6 仍须由任务 13/26 的派生调用者担保；当前 `IntercompanyBalance` 是公开、可 serde 的调用输入，`build_worksheet` 仅校验成员/科目/现金标记/对手金额相等/科目要素，不校验申报余额上界。此处不把“传入与 Books 不符”直接判成可达的产品缺陷：任务 12 明确契约为调用方派生事实，需审查实际派生 caller 是否可产出超额值后才能升级。 |
| 13 | F1 更正分录泄漏到后续利润表 | 当前 `RestatementRegister` 按 Scope 持久累计；`correct`、`close_month`、`close_year`、`snapshot_interim` 的单体路径都传给 `generate_with`。`window.rs` 有效期间/实际现金分离与双向配平仍对应原修复。 |
| 13 | F2 报表与结账简化未登记 | 52b0d16 后 docs §2.7 与 fixture 条目覆盖 D1–D7；未发现当前所读登记仍缺该组简化。 |
| 13 | F3 `notes.rs` 用 `unwrap_or(zero)` 吞溢出 | 原报告已改为 `closing.sub(movement)?`；缺桶取零只是不存在的运动贡献，和错误吞噬不同。 |

## 新发现：更正的复合事务可部分提交

**候选级别：P1 / 需独立复核修复。** 当前 `ClosingEngine::correct` 在 `closing/mod.rs:269–290` 先调用 `books.post_batch(...)`，随后立即写入 `self.restatements`，之后才调用可失败的 `generate_with(...)`；只有成功生成后才 `store(set)`。因此任何 post 成功、报表生成失败的合法请求，都会返回 `Err`，却留下已过账调整分录和已登记重述映射、没有对应 correction 版本。重试同一请求会因 source 已使用而拒绝；下次普通封月又会把调整映射纳入报表，用户得到的状态与本次失败返回不一致。底座的 `post_batch` 只保证“过账批次自身”原子，不覆盖其后的重述登记/五表生成/版本发布。

**有效可达性依据（不依赖 industry/Books 错配）：** 可用受支持 chart 与正确对应的 `IndustryPresentation` 构造确定的溢出反例。用 `AccountChart::generic_v1()`（Industrial），在 2030-12 过账 `Dr 1001 h, Dr 1122 h, Cr 4001 2h`，其中 `h = floor(i128::MAX / 2)`；所有单科目余额/借贷合计可表示，资产和权益均为 `2h = i128::MAX−1`。成功 `close_year(2030)` 发布目标年报。之后在开放的 2031-01 过账平衡且有正现金余额的更正 `Dr 1001 h, Cr 6001 h`（`CashFlowClass::Operating`，正确会计含义为收款收入）。`post_batch` 逐科目试算通过：1001 借方合计到 `2h`、6001/1001 本次金额均在范围内、批内借贷相等、现金仍非负；其“试算”并不汇总跨科目总借贷数（这由封月的 `validate_trial_balance` 承担）。更正映射到 2030 后，重述余额表资产合计至少 `3h > i128::MAX`，`generate_report_set` 在五表计算中返回类型化溢出错误。于是 `correct` 返回 Err，但前述 `Books` 已记入调整，`RestatementRegister` 也已更新，版本列表未增加。这里没有 chart/industry 错配、负数金额或违反分录复式约束，是数值范围内逐科目合法、但派生报表超出表示上界的实际可达路径。建议修复时先在克隆/暂存 Books 与 register 上完成 post+生成+validate，再统一提交，或提供显式 rollback；增加拒绝后 Books、ClosingEngine bytes 均不变以及同请求可重试断言。

此外 `close_year` 在 `closing/mod.rs:197–206` 先调用成功且有副作用的 `close_month`（封 12 月并写月报版本），再生成年报；若年报专属全年窗口生成失败，会留下“函数整体失败但 12 月已封、月报已发布”的半完成状态。与 `correct` 已证实的同类事务缺口相比，这条额外路径的错误输入构造尚需专门 fixture 证明，本次列为次级边界候选，不能单凭月报/年报窗口不同就报为已证实故障。应与 correction 共用明确的复合事务策略；至少增加失败注入/极值回归覆盖，确认 API 失败时的公开契约。`close_month` 本身采用先生成、后封账、最后存版本，次序较安全；`snapshot_interim` 无 Books 写入，生成成功后才存版本。

## 抵销、报表和会计上界/原子性边界

- 合并抵销的纯函数路径先 `aggregate_balances` 得本地 map，再造 worksheet 并应用；Books 不可变引用，不回记成员账。抵销输出失败不会改变成员 Books。`apply_worksheet` 在聚合 map 内逐行 mutate，若溢出会留下局部部分 map，但 map 由当前 `consolidate` 栈拥有，错误返回即丢弃；这是安全的局部失败原子性。
- 合并金额仍是调用方派生事实，不可把“等额配对”误称“与两边账面余额核对”。代码没有证明内部应收/应付申报 `amount <= account balance`；类型注释要求金额恒正，但 `precheck_balance` 未验证 `amount.is_positive()`。负数/零值属于违反字段语义的输入，建议作为防御性校验跟进；本轮不将其提升为已证实的正常路径错报，因为需先证明生产派生 caller 能生成此输入。正数超额申报同理：需沿 task 13/26 的真实子账派生 caller 核实，不能仅以任意不匹配请求断言产品错误。
- 任务 13 单体报表由真实 journal entries 重建，`window` 的 correction 双口径配平由 `ReportSet::validate` 对间接法逐次校验；未发现新的现金重复或未实现的“差额 plug”。Notes 缺项取零、BS/Equity 缺余额取零是稀疏账本读模型的零余额语义，不是错误 fallback。
- 公司列报账户显式由 chart 版本映射；本次没有把传错 `IndustryPresentation` 当默认故障证据。检视公开生产调用 `session.rs` 的封年路径与 `information/prehistory.rs` 快照路径，输入行业值沿公司类型/历史构造给定；报告旧复核里行业列报依赖由 caller 保证的结论仍需保留为契约，而非本轮发现。

## 门禁结论

1. task 11–13 三份文档的每一章（包括历史拒绝、处置方案、复核结果、算术探针、清理记录）均已全文读取；旧结论的修复链未发现与当前代码矛盾。
2. 已确认一个跨过账/底稿/报表发布的部分提交缺口（`correct`），另有 `close_year` 两阶段发布失败的一致性边界。建议将这两项作为实施前置复核发现处理并由未实施者复核修复，不能把 task 13 的 APPROVE 直接解释为复合操作全路径原子。
3. 未将错配行业/账套、负数申报或超账面申报直接声称为正常可达产品缺陷；后两者取决于真实派生调用者，需由 task 13/26 caller 证据确定。
