# Accounting 工件 EOF 全文扫描

- 复核对象：`agents/oop-refactor-implementation/README.md`、`domain/accounting-result.md`、`domain/accounting-review.md`，均读至 EOF。
- 代码版本：HEAD `a7c7ce357bdc9f88c03633744b2d5815db49e9b2`，其提交信息为 merge `08e4fc7`；HEAD 的 accounting 与 Industrial 源码相对 `08e4fc7` 父提交无差异（针对两目录的 diff 为空）。因此本文按当前 merge 产品代码复核，不把早期签署自动当作当前证据。
- 范围：逐章清点、逐项追当前 owner/caller，并抽核会计语义及现行正式规则登记；未运行测试或编译。

## 行数与章节矩阵

| 工件 | 行数/章节 | EOF 扫描与对应当前证据 |
| --- | --- | --- |
| README.md | 7 行；标题、三段正文 | L3 汇总/统一状态链接：按当前工作树确认所链摘要和 plan 是审计工件，不替代代码检查。L5 分支/验证范围及语言约定：这是历史目录说明；当前 checkout 为 `docs/implementation-reaudit`，不据此声称当前分支名仍是 `refactor/oop-complete`。L7 最终验证/冻结/总审链接和不推送：属于历史协调流程，未发现会覆盖当前产品行为的后续承诺。 |
| accounting-result.md | 65 行；基线/范围和语义边界 L3–13；逐动作表 L15–27；验证 L29–45；源码清单 L47–65 | 表内 8 个动作全部逐行核对，具体见下表。L31–45 既声明实施者未跑测试，又称 root 后来短测通过；两个陈述分别限定主体/时点，并由 L37–42 的“建议 root 统一编译后”及 review L92–94 的静态审查限定，不是把实施者工作冒充测试。当前代码未重新验证测试。L49–65 清单中的 13 个源码文件与本次 accounting owner 变更点一致。 |
| accounting-review.md | 126 行；元数据 L3–7；依据范围 L9–28；语义 L30–44；最小范围 L46–57；逐项边界 L59–80；测试局限 L82–94；源码版本绑定 L96–126 | 逐项核对旧结论与当前实现；L98–123 的绑定基于旧工作树快照且明确自身限制，不当作当前 HEAD 的源码哈希证明。当前版本关系由本扫描的 HEAD 与 merge 父提交只读核查补足。旧结论的适用性及未决边界见下文。 |

## 动作与 caller 复核

| 动作/原结论位置 | 当前代码证据 | 复核判断与残余 |
| --- | --- | --- |
| N05（result L19；review L63） | `reports/consolidated_window.rs` 的 Builder 调 `Accumulator::add_current_worksheet`；`consolidated` 被 `generate_report_set` 的 consolidated source 使用。 | caller 与所有权仍吻合；worksheet 仅进当期窗口、累计器内部 checked 次序仍应以对应测试为行为证据。无新 A 股制度决策。 |
| N06（result L20；review L64、73–78） | `industrial/sales.rs` 调销项税、`industrial/purchasing.rs` 调进项拆分；`industrial/expenses.rs` 调 `IncomeTaxPolicy::compute`。源文件 free API 仍委托 policy。 | caller 真实且单一算法路径吻合。税率/CIT 法源及 CAS 18 原文仍由 `docs/company-accounting.md` 登记为 blocked，不能把半偶与亏损结转实现表述成现行税法或完整准则认证。 |
| R2-N01（result L21；review L65） | `closing/mod.rs` 的 `correct` 在 `Books::post_batch` 后登记；`closing/save.rs` 经 `save_rows/from_rows` 投影存档。 | 时序与旧结论吻合。保留“过账成功、随后报表生成失败时已写账及登记”的部分提交边界，是明确实现行为，需维持显式错误；本轮未见新静默回退。 |
| R2-N02（result L22；review L66、79） | `industrial/capex.rs` 仍按 preview/validate → `post_with_commit` → apply 调用资产账簿。 | caller/事务顺序吻合。折旧和减值的现实准则原文仍受阻，当前登记为简化；本轮不据游戏算法作法规判断。 |
| R2-N03（result L23；review L67、80） | `InventoryLedger` 转交 state；实际路径见 `industrial/config.rs` 开局 seed、`purchasing.rs` purchase、`sales.rs` sell、`production.rs` consume/receipt。 | 调用面吻合且不止销售路径；cost 溢出时 quantity 已先写的非原子顺序被记录为既有语义。未发现 caller 将失败吞掉的新增证据。移动加权平均继续是登记中的游戏假设。 |
| R2-N04（result L24；review L68） | 唯一生产 worksheet 路径在 `consolidation/eliminate.rs::build_worksheet` 对每笔 sale 校验后生成；组装继续由 `consolidate` 执行。 | 旧结论“一个生产 caller”成立；零利润/抵销工作底稿语义与 CAS 33 简化登记一致。与 A 股交易规则无直接变更。 |
| R2-N06（result L25；review L69） | `balance_sheet::generate/prior_lines` 调 `EquityPresentation`，少数股东比例运算仍在 consolidation/PriorSplit。 | 层间边界吻合；不得将合并列报的游戏口径扩称为完整 CAS 33 合规，正式文档明列单层、固定控制及其他简化。 |
| R2-N07（result L26；review L70） | `generate_report_set` 的 source 分支构造 `ReportClassification` 后供 income、balance sheet、notes 使用；公开 `merge_assignments` 仅由 compatibility/error 测试直接引用。 | 生产报表路径确实消费完整分类对象，兼容 API 保留判断成立。review 已注明实际 chart 跨层仍由行业 gold/公开财报测试覆盖；本轮未运行它们。 |
| R2-N08（result L27；review L71） | `consolidated_window.rs::consolidated` 建立 Builder，消费 consolidation 输出形成 `StatementWindows`，后续权益/附注消费者保留。 | caller 和 owner 仍匹配。当前缺陷/法规主张没有超出历史简化；不把静态等价复核当作新测试结果。 |

## 旧签署复核及候选反证

- 旧 review 的大 A 语义结论只对“本批重构没新建规则”成立，不是规则来源复核的替代。正式依据以当前 `docs/company-accounting.md`、`docs/trading-rules.md` 和相关 ADR 为准；其中 CAS 1/4/8/18/28、VAT/CIT 法源阻塞及多个游戏简化继续清楚标示，本批没有理由抹去这些限制。
- 旧 review L53 将 N05/N06、R2-N01/N04/N06/N08 称为可选内聚性改进，result 明列 owner、行为和 caller；当前源码 caller 仍存在，未发现动作已被后续产品变化废弃。基于明确限定的 OOP 目标，可接受其范围必要性判断。
- 候选反证一：README L5 说目录“对应 `refactor/oop-complete` 分支”，当前分支名不同。反证后判断为历史工作目录归属陈述（其余内容也叙述历史验证及本地协调），不构成产品或当前 Git 状态保证，无需按产品缺陷报告。
- 候选反证二：result L44 写 root 指定短测通过，而实施者 L31 写未运行。按原文语义分别是实施者与 root、实施阶段与最终阶段；review L92 明确复核者未执行测试且要求 root 记录结果。证据链内部自洽，但本次扫描没有独立重跑或验证该历史运行日志，故仅保留为历史结论，不升级为本次验证事实。
- 候选反证三：review L98–123 称源码哈希绑定此前工作树，却注明 SHA256 是最终快照、非首次审查时证据。此处限制已披露；本扫描仅以代码调用存在性、当前 merge 与其父提交无产品目录差异作新鲜性核对，不补造该历史哈希证明。
- 新发现：未发现需要立即修复的 caller 漏迁、跨层单位漂移或把 blocked 会计/税法规则误写成已核验的情况。仍未穷举的错误组合、serde 接受集和行业 chart 集成覆盖，review L87–90 已明确列为局限，属于后续验证事项而非本静态扫描能关闭的承诺。

本报告仅记录指定 EOF 工件的独立扫描；未修改产品源码，未执行 Git 写命令、cargo、测试或长验收。
