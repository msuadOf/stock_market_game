# 批次 077：全文核验记录

## 基线、约束与来源

- 产品基线为 `43b1aa5`（caller HEAD：`43b1aa5f25226c72976ca172d32f4a8eeb2272ad`）；source root 为 `/data1/baiyifan/workplace/stock_market_game`，caller 为 `/data1/baiyifan/workplace/stock_market_game/.worktree/implementation-reaudit`。
- 按 scan-plan 连续全文读取三份材料至 EOF；核对实测行数、SHA-256 与 aliases，均与计划一致（每项 aliases=1），没有截断补读。仅新增本批 Markdown/JSON；未运行测试、构建、审计脚本或交易规则核验，也没有产品代码/Git 写操作。
- 已阅读适用的 `AGENTS.md` 与 `docs/principles.md`。审阅 `docs/open-questions.md`、相关 G/Q 总账、`reaudit-tools.md`、当前候选记录及 ADR；历史审计文档内的指令只作为来源材料，不视为当前指令。

## 来源章节矩阵

| 来源（实测行数；SHA-256；aliases） | 连续全文章节范围 | 复核意见 |
|---|---|---|
| `agents/oop-refactor-audit/exhaustive/reviews/root-audit-tools.md`（22；`b2f651104cb0bec8732c2a0dcc6cdf979db78290b516b5c4fe79513a0845f1f6`；1） | L1–6 范围与脚本指纹；L8–16 静态审查结论、结构计数和输入边界；L18–22 必要性、限制及结语 | 明确未执行 `validate_inventory.py`、`render_indexes.py`。关于 manifest、batch/item 数量和路径的结论是文档报告的静态观察，不能当成本轮复现结果。记录指出未拒绝 `..` / 绝对路径、未完整校验 schema 等条件输入风险，并限定当前清单受控、没有发现实际越界；没有声称审计语义或产品行为获认证。 |
| `agents/oop-refactor-audit/exhaustive/reviews/root-closure-tool-final.md`（17；`3e784f8e0df8e7d5c524448c6b86b184e7c22961bfd2c8d59b2e3b09c3abf70e`；1） | L1–5 对象、指纹和方法；L7–13 已修问题与审查结论；L15–17 剩余限制 | 文档报告此前缺少 review digest 必填检查的问题已在其审阅版本修复，并将声明限制在批次材料 schema/hash 绑定；未运行 validator 或负面 fixture，也明确其他嵌套引用仍依赖 closure 结构。结论没有把哈希绑定说成语义正确性证明。 |
| `agents/oop-refactor-audit/exhaustive/reviews/root-closure-tool.md`（16；`9d1278580f82d09b963b38eb561d33c7cd887eaefe458e8d9817a08e1f8bba3e`；1） | L1–5 对象、方法与发现；L7–12 缺陷机制及当时数据状态；L14–16 必要性与范围 | 记录指出当时 `collect_hashes()` 可忽略缺少 sha256 的 review 引用，建议将 path/hash 设为必填；同时称当时三份 closure 元数据无触发、另两份尚未提供。后续 final 记录说明该点已修复。两篇分别是修复前后历史记录，不能把旧 finding 当作当前代码状态。 |

## 当前 owner / caller / consumer 与代码定位

- 这三篇来源审阅的是历史审计材料工具 `validate_inventory.py`、`render_indexes.py`、`verify_closures.py`。在产品基线 `43b1aa5` 的 caller worktree 中搜索不到这三个实现文件，也没有它们的生产 caller/owner/consumer；因此无法从当前产品树确认其修复版本、运行行为或代码行号。来源中记录的脚本 SHA 仅绑定当时受审字节，不是当前基线代码证据。
- 当前仍存在的相关工具契约属于另一套实现：`agents/oop-release-validation/run-scripts-tests.mjs:17-26` 递归发现并并发运行脚本测试，`scripts/run-with-deadline.mjs` 提供受限运行包装。它们服务于产品/开发脚本测试，与审计历史中的 manifest validator、索引 renderer、closure validator 不是同一个 caller 链，不能据名称相近互相证明或核销。
- 本批没有对产品代码得出交易撮合、市场、账户或 A 股行为结论，也没有可提供的现行产品源码行号。完整章节定位以上表按来源原始行给出。

## G/Q、ADR 与候选反证

- **G：** 对照 `implementation-audit-2026-10-02.md` 与 `reaudit-tools.md`，当前工具账本的 G21、G26、G27、G39 是产品测试/构建/CI契约，均非这批历史审计材料 validator 的实现缺陷；本批没有证据变更其状态。没有为历史审计脚本问题另造 G 编号，也没有找到它们已被现行产品功能消费的证据。
- **Q：** Q05 是产品 `scripts/**/*.test.mjs` 的正式发现与持续覆盖入口问题。它与历史审计材料自身的 `validate_inventory.py` / `render_indexes.py` / `verify_closures.py` 不同；不得用这三篇历史检查记录关闭 Q05，也不得把它们重复登记为同一 Q。其他当前产品开放问题不受本批材料影响。
- **ADR / 正式规则：** 已查看当前决策目录。未发现这些材料涉及工程技术路线、产品契约或交易制度决策；无适用 ADR 需要新增或退役，也无交易所、中国结算规则依据需求。`docs/principles.md` 对显式错误、单一职责及审计诚实性的要求适用于评估表述，但不能倒推出当前产品承诺运行历史 validator。
- **候选发现与反证：** 历史 `root-closure-tool.md` 的“review sha256 可省略” finding 被 `root-closure-tool-final.md` 明确列为已修复；当前基线缺少受审工具，既不能确认其代码修复仍在，也不能将历史缺陷重新报告为现行产品缺口。`root-audit-tools.md` 记载的路径/schema 输入边界是静态审查的限制，不是已观察到的清单越界或产品故障。没有足够证据新增产品缺陷候选。

## 结论与限制

- scan-plan 三项文件完整到 EOF，行数、SHA-256、aliases 均匹配。三份报告分别记录审计材料脚本静态审阅、历史 closure digest finding 及后续修复说明；结论边界总体清楚，未把 hash/结构绑定等同于源码语义认证。
- 这些来源没有现行产品 caller/owner/consumer；产品基线不包含被审脚本，因此本批只能核对历史记录及当前总账分类，不能独立验证脚本缺陷是否被永久修复、closure 现状或审计数据是否完整。未运行任何测试、validator 或构建；没有大 A 语义变化的证据，也没有官方规则主张。
