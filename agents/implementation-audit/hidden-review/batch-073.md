# Batch 073：全文核验记录

## 基线、约束与来源

- 基线 `43b1aa5`；源树 `/data1/baiyifan/workplace/stock_market_game`，caller `/data1/baiyifan/workplace/stock_market_game/.worktree/implementation-reaudit`。
- 已阅读 caller `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`、相关 ADR-0005、ADR-0019、ADR-0025、当前 `agents/implementation-audit/implementation-audit-2026-10-02.md` 与 `reaudit-engine.md`。本批历史记录讨论的是 engine 测试审计描述及其边界，不提出改变沪深 A 股交易语义的承诺；相关交易规则文档仍是正式语义来源。
- 三份 source 均连续全文读取至 EOF；行数与 SHA-256 均匹配 scan-plan，无截断补读；每项 aliases=1。章节/内容范围逐项记于 JSON。

## 当前 caller 与交叉核对

- `packages/engine/src/session.rs:2716-2720` 的 `GameSession::restore` 接收 `&SaveSlot`，先校验 urgency policy、调用 `validate_save_slot`，再局部创建新 `GameSession`。`packages/engine/src/session/persistence.rs:258` 是存档校验入口。caller 中没有证据支持“恢复失败时一个既有 running session 被原子回滚”的承诺；engine-tests-09 历史记录明确指出相应测试只证明非法输入返回 `InvalidSave`，不证明既有会话失败前后不变。该旧措辞已在历史修订记录中纠正，不构成当前新增缺口。
- engine-tests-06 的历史发现限于测试说明不应将通用 `Market` fixture 概括为完整现行 A 股规则验证。复核记录称修订后已限定为 fixture 参数下的行为，明确未覆盖交易所、板块、证券类别差异。该限制符合 AGENTS 与 `docs/trading-rules.md` 的语义门禁，不能据此要求测试 fixture 代表全部市场类别，也不能核销真实规则覆盖承诺。
- engine-tests-08 的历史发现是 `RealEstateBooks` 值相等曾被误称为序列化字节相等，以及存档 fixture rationale 错配；来源记录称两者均修订并复核。它们是审计文档准确性问题，不是当前 caller 的产品实现错误。来源未提供当前测试清单/模块文档内容用于重新验证该修订绑定，故仅接受其为历史核销证据，不扩张成当前源码通过结论。
- 当前 `reaudit-engine.md` 登记 G06–G09、G16、G28、G35–G38 等实际生产缺口；三份来源主要核验测试审计文档，没有证据显示这些缺口已修复或能由测试审计核销。未发现来源揭示了超出现行 G/Q 的、具备当前生产 caller 证据的新遗漏。

## G/Q、候选与反证

- **G/Q 对照：** 无新增 G/Q，也不更改既有 G 状态。相关 G06–G09 等仍按 `reaudit-engine.md` 原结论处理；测试审计条目不能代替生产路径证据。Q02/Q11 仍按开放问题和 ADR-0016/当前说明处理，本批不触及其决策。
- **候选：** 无。engine-tests-06 的完整 A 股规则覆盖、engine-tests-08 的字节原子性、engine-tests-09 的既有 session 失败原子性，均不能作为当前产品遗漏：前两项来源报告称已纠正描述；最后一项已明确降界为非法 SaveSlot 拒绝校验。没有运行证据可支持升级为运行缺陷。
- **边界：** 历史审阅记录本身不等同于现行产品验证。本批未运行测试或构建，未查官方规则来源，不把任何测试说成通过；不据未来建议、历史表述或未运行测试创建产品候选。未修改产品代码或 G/Q 台账。
