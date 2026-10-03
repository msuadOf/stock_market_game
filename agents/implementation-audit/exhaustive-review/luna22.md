# Luna22 全文独立复核记录

## 范围与完整性

- 目标基线：`.worktree/implementation-reaudit`；指定产品提交 `08e4fc75b52a71a3262a8a938c57b44f8b5b4960`。本工作树 HEAD 是 `a7c7ce357bdc9f88c03633744b2d5815db49e9b2`，其提交信息为合入 `08e4fc7`；HEAD 树与目标树不同，但 `git diff --name-status 08e4fc7 HEAD` 显示差异仅为既有 `agents/implementation-audit/` 记录文件，且本报告涉及的全部产品代码、fixture、测试、领域文档均与 `08e4fc7` 完全相同。目标提交的直接父提交是 `b76ece39b3a1635adde52da07375607f19b56ecc`。
- 三份目标材料连续全文读取至 EOF，命令输出含完整文件正文，未用片段代替阅读：`notepad-recovery.md` 63 行、`task-1-review.md` 27 行、`task-10-review.md` 253 行，共 343 行。
- 本轮无产品代码或 Git 操作、无测试/长测。唯一写入为本审计工作记录。

## 逐章核对矩阵

| 材料与章条 | 原文结论/承诺 | 当前代码或证据复核 | 状态 |
|---|---|---|---|
| `notepad-recovery.md` §Recovery method | 从 tracked `HEAD` blobs 恢复 1161/1140 行基底；记录基底 hash；损坏工作树仅剩 Task 32 尾部且未留损坏前 hash；历史乱码按字节保留 | 回执明确区分可验证基底与无法恢复的 post-HEAD 全文，不声称保存了覆盖前工作树 hash。此处是明确的证据限制，不得把恢复结果描述为逐字恢复全部后续 notepad。 | 符合，限制已诚实披露 |
| 同 §Appended sources | 列 Tasks 27–32 的证据来源及支持范围；Task 31 未宣称完成 | 条目明确指出 Task 31 为 unresolved，Task 32 保留既有尾部且避免重复；来源列是可追溯目录/文件名，但单凭回执不能证明每个摘要细节都由来源支持。 | 未发现新反证；摘要真伪仍以对应源证据为准 |
| 同 §Result | 报告恢复后行数、SHA-256、UTF-8、末尾换行、历史及追加标题顺序 | 原文数值完整且互相一致；当前只读取回执和正式仓库文件，未读取外部 notepad 工作副本计算现值 hash，故本复核不把旧 hash 当作当前文件 hash。 | 历史收据内部成立；当前再现性未验证 |
| 同 §Unresolved loss and incident / §Scope receipt | 承认 Tasks 27–31 post-HEAD prose 不可完整恢复；说明覆盖事故和禁止 whole-file write；声称未意图修改产品、测试、Git 等 | 事故处置说明范围明确、未伪造遗失内容；“未意图编辑”是执行者范围收据，不等价于独立证明仓库状态。 | 无新增问题 |
| `task-1-review.md` Gate 1 | 3 文件 add-only 基线测试；默认矩阵逐字段与 `defaults.ts` / Rust 语义对齐；价格限制、手数、T+1、费用、时钟等与既有语义一致 | 这是 2026-09-10 对 `0a77d8f` 的历史测试变更审查，不是现行规则来源。未发现其把基线 fixture 误报成业务实现。当前交易规则变更不在该历史文档证明范围内。 | 历史审查结论未被本轮反证；不替代现行规则复核 |
| 同 Gate 2 | 恰为计划要求的 3 文件、无产品行为/存档修改 | 原文明确 add-only 与文件数；与所述提交范围一致。 | PASS（历史范围） |
| 同 Gate 3 | Node 22 项、Rust 5 项；负例、进程边界、失败 manifest 等有断言；列出覆盖细节 | 证明当时测试覆盖与实现限制；原审自身标出 Invalid JSON stdout 仅邻近测试覆盖，属于测试缺口而非必然错误。 | PASS，保留的小项见下表 |
| 同 Gate 4 | `engine_error_events` 非零必须在日志及 manifest 可见；pnpm 不可用如实记录 | 该条防止失败静默丢失，符合工程原则。 | PASS |
| 同 Non-blocking observations | malformed timeout env 静默退默认、死 stub 分支、stderr 截断、Invalid JSON 直测缺口 | timeout 静默默认值得与现行错误处理原则复核；审查者明确判定当次不影响证据完整性，未声称已修。其余为低风险质量观察。 | 历史 nits 仍未由本轮证明已修；不升级为本批次阻断 |
| `task-10-review.md` §0 | 因兄弟工作区未在主树运行 cargo；在隔离 worktree 审查 | 这是 60c7c00 初审当时的执行收据；后续复审另有隔离收据。 | 历史可采；不是本轮执行证明 |
| 同 §1 | focused 17、engine 728、check、clippy 结果；8 条 warning 均定位为非本任务 | 初审之后新增上界边界测试，复审的套件计数增至 18、全量 729，前述旧数不应误当最新数。 | 由 §Re-verification 更新 |
| 同 §2 问1 | CAS 25 / CAS 30 锚点，计量、现金流分类、守恒、错误原子性、整数纪律、unsupported 路径等通过 | 原审将依据、逐笔分录和手算分别列出；代码路径与保险列报分类仍存在。这里仅接受其记载的复核结果，不重复把旧版本测试输出当作本轮重跑。 | APPROVE 证据保留 |
| 同 §2 问2 | 22 文件均属任务 10，最小范围 | 原文给出模块清单及纯行数。 | PASS（历史提交） |
| 同 §2 问3 | rate_bp 上界缺测、跨图表 1122/2501、`rhe_div` 孪生为轻微/后续事项 | 上界缺测已在修复后加测；跨行业代码问题应按后续报表/合并使用者重新核对，不能只沿用“注意点”结论。 | 上界关闭；发现一个当前跨层边界候选，见下节 |
| 同 §3 D1–D5 与原 REJECT | 初审以“未登记”为唯一阻断，拒绝 60c7c00；代码语义本身可接受 | §Re-verification 明确指出后续 `e8210bb` 增加 fixture game-assumption、三条 coverage `source_ids`、docs 行内标注与上界用例，且 engine 源码未改。现行 fixture/docs 确认 `game-assumption-insurance-gmm-simplifications` 与 D1–D5 在位。 | 初审 REJECT 已被后续复审正式取代，不得再报告为当前 verdict |
| 同 §4–5 | 通过项复用、证据位置与初审输出记录 | §Re-verification 说清楚哪些结论沿用、哪些增量重核；这避免把 docs-only 修复误说成重做引擎审查。 | PASS |
| 同 §Re-verification 隔离 / diff / 三问终评 | `e8210bb` 恰好 3 文件 +44/−7；D1–D5 文字与代码相符；补 rate_bp=10001；复跑 5/5、18/18、729/0/4、check 通过；最终 APPROVE | 当前 HEAD 的 policy fixture、`docs/company-accounting.md` §2.4、`guards.rs` 均可见相应登记与用例。结论与“旧 REJECT 已解决”一致。注意指定 `08e4fc7` 与 HEAD 树不相同，仍需按准确产品树重查相关文件是否一致。 | APPROVE（截至该修复组合）；当前目标树需做提交锚定复核 |

## 当前调用者与跨行业分类复核

1. `insurance_presentation_lines(&Ledger)` 是 insurance 纯读分类层，只从本行业 `Ledger` 的 1002/1122/2501/2502/4001/4103/6051/6451/6541 取数，代码映射由 `insurance::codes` 与 insurance chart 共用。它本身没有把别的行业账簿合并进来。
2. 完整报表调用者 `generate_report_set` 对 `ReportSource::Consolidated` 会从每个成员的 chart version 解析 `IndustryPresentation`，先生成 consolidated windows，再把所有行业分类表合并：`packages/engine/src/accounting/reports/mod.rs:202`、`:208-217`。分类表 `merge_into` 对同一科目码映射至不同主表行会返回 `DuplicateClassification`：`packages/engine/src/accounting/reports/notes.rs:73-90`。
3. consolidation 聚合层注释称混合行业集团合法（工业+银行为例）；其 `ChartConflict` 只比较 `element/is_cash/is_contra`，名称差异允许：`packages/engine/src/accounting/consolidation/aggregate.rs:3-6`、`:114-130`。当前 mixed-industry consolidation 金样只用工业+银行：`packages/engine/tests/consolidation/gold.rs:34-38,112-116`；完整报告金样也使用工业+工业：`packages/engine/tests/industry_reports/consolidated_gold.rs:1,39`。未见工业+保险完整报告用例。
4. **新候选（待编写者确认产品范围，不判成静默错误）**：consolidation 所称“混合行业合法”覆盖面比完整报表合并分类更宽。工业 `1122` 是 Receivables、工业 `2501` 是 LongTermBorrowings；保险 `1122` 是 InsuranceReceivables、保险 `2501` 是 InsuranceContractLiabilities。若两者同时在 consolidated report 中，`from_industries` 对这些共有码遇不同 `NoteTarget` 会类型化报错 `DuplicateClassification`。这保证不会把应收保费/保险合同负债误呈为工业应收/长期借款，但有意或无意地将可合并的工业+保险集团限制在无法生成五表。应明确此组合是否“不支持”，或按成员行业分类后再映射到合并可兼容的报表行；并以工业+保险真实账簿加一条 consolidate 成功、report 失败/成功边界用例固定契约。当前代码是 fail-closed，故这是能力/契约不一致候选，不是已经证实的错误列报。
5. 任务 10 原审 §2 问3 对 `1122/2501` 的“独立 AccountChart 命名空间、不交叉污染”只足以支持单体分类；其后续提示“任务 13/33 报表生成与合并必须经各行业 presentation_lines 分类层取数”没有覆盖当前 consolidate 后按 code 合并及 multi-industry assignments union 的语义。应把此候选交给完整差异复核，不能仅凭单体 presentation 通过就关闭。

## 最终判定与局限

- 三份目标材料结构、历史状态流转和 `task-10` 原拒绝的后续修复均可对账；当前 D1–D5 已见 fixture/docs 登记，`10_001` guard 测试已存在。task-10 结论应读作 **初审 REJECT → 修复复审 APPROVE**。
- task-1 的 PASS 适用于当时仅新增基线固定测试的差异，不是对 2026 当前 A 股规范、也不是产品行为正确性的独立新审查。
- 未发现 notepad receipt 虚构被遗失的 post-HEAD 文本；它明确承认不可恢复和覆盖前 hash 缺失。恢复文件当前 hash 未在此轮重算。
- 首要新复核候选是**工业+保险 consolidated report 的行业科目冲突边界**：当前路径显式拒绝而非错误归类；产品是否承诺支持需依据计划/ADR/调用者契约进一步定性。不要以普通 mixed-industry consolidation 通过替代报表消费者验证。
- 产品目标提交锚定：HEAD tree `181a29ba4f78156afeac540954c0c4e67003cec5` 与 `08e4fc7` tree `045d132799ad31be7f0daf1c966fd00fa6c62090` 不同，但逐路径 diff 已确认差异只在审计工作记录，所审产品路径与 08e4fc7 一致；当前混合行业候选可归到指定产品版本。
