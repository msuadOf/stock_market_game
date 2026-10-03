# 隐藏扫描批次 147（owner 2）

## 来源与完整性

- 按 caller 的 `scan-plan.json`，本批 owner=2，来源基线 `43b1aa5`。三份材料均从主工作区首行连续读取至 EOF；实测路径、行数与 SHA-256 符合计划，详见配套 `batch-147.json`。
- 已核对根 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`、`docs/architecture.md`，以及 implementation audit 总账、pipeline caller/owner/consumer 复核与 ADR-0017/0018/0019。目标代码树为 `.worktree/implementation-reaudit` 的 `43b1aa5` 基线。未运行测试或构建，未修改产品代码、Git 状态或 G/Q 台账。

## 来源结论与当前核对

- `engine-pipeline-06.md` 是其所审 OOP 清单及源码的历史独立复核，主张 JSON/模块说明与 8 个分配文件的职责、测试覆盖及候选边界一致。其自身声明没有重新跑测试或构建；不能据此推断当前流水线已完成运行时验收。当前 caller 复核确认 `GameSession::step` 进入正式连续事务，P0/P1 资源快照、P3/P4 受理、候选失败隔离及 P9 单点提交由生产链消费，见 `reaudit-pipeline-contracts.md`。这只说明当前已接线契约，不等于本批历史 review 验证过当前代码。
- `engine-pipeline-07.md` 记录两项历史审计文档问题：模块把 manifest 的 15 个文件统计为 4 个生产模块、11 个测试模块，而 review 判断应为 7/8；JSON 对 `decision_snapshot_tests.rs` 漏列 `retail_account_input`。这是历史核销材料内部的清单/符号记录问题，不是交易行为缺陷。现有后续批次对重构后的 pipeline 07 另有复核，但不能据此确认本份历史工件的修订字节或消除其记录的问题；作为文档证据边界保留，不升级 G/Q。
- `engine-pipeline-07.md` 的交易边界结论强调 `Envelope::apply` 先在副本计算校验再替换、股票内受理顺序不由身份排序决定、worker 失败后丢弃 tick candidate。它与 ADR-0017 接受的局部实际冲突顺序、资源分账及原子提交方向一致；不把输出身份或数组顺序解释成跨账户优先级。
- `engine-pipeline-08.md` 明确审查对象是候选设计且未实施，并保留 `EnvelopeLedger`、受理账本和 NPC 决策现有 owner；存档形状变更需单独授权。其将费用封顶标为游戏简化，未宣称交易所清算规则。ADR-0018 仍为 proposed，ADR-0019 的当前范围也不允许以任意请求配额替代性能工作；没有从候选文本推导实现承诺。

## G/Q 与证据边界

- 当前总账保留 G01–G68 的生产缺口并强调局部实现不能核销整模块；本批三篇旧 review 没有提供新的、经当前 caller 证实的已批准承诺遗漏，也没有证明现行 G/Q 应关闭或重开。不改动 G/Q。
- 三篇来源无新的 A 股规则主张。对交易语义仅沿用 ADR-0017、现行交易规则文档中 T+1、资源守恒、局部撮合顺序及费用简化的边界；本批未联网复核交易所规则。
- 结论限于这三篇历史 review 及当前静态 caller/owner/consumer 对照。历史材料声称的文件与测试清单核验不等同于本轮测试通过，也不代表全仓或相关模块完整验收。
