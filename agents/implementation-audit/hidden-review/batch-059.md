# 批次 059 独立复核

## 阅读完整性

| 基线来源 | 行数 | SHA-256 | EOF |
|---|---:|---|---|
| `agents/oop-refactor-audit/exhaustive/reviews/engine-pipeline-08-caller-final.md` | 39 | `61a5542829df667b0e06c161676efa386ef768f84f5c0787191db5bbb8bc9419` | 是，全文至末行 |
| `agents/oop-refactor-audit/exhaustive/reviews/engine-pipeline-08-final.md` | 36 | `59a7d8d0582da27c3568a9e3dbf774b9771c72c7ad69ed8faf21f8d71ade5c75` | 是，全文至末行 |
| `agents/oop-refactor-audit/exhaustive/reviews/engine-pipeline-09-final.md` | 42 | `7172967a5b767fec75613527f12a9f5b11a714dbc1e917aacc1b4a924050531c` | 是，全文至末行 |

三份指定来源均直接读取完整文本至 EOF，并与计划中的 SHA-256、行数逐项相符。审查基线为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`，当前 HEAD 与之相同。已阅读仓库 `AGENTS.md`、`docs/principles.md`、现行 `docs/decisions/` 与 `docs/open-questions.md`。进一步对照 `ready_ingress.rs`、`authoritative_tick.rs`、`pre_open_transaction.rs` 和 engine pipeline 当前调查记录。未修改产品源码或 Git；未运行测试、构建或官方规则查询。

## 结论

三份历史复核的关键界限仍成立。`AccountReceipts` 的 `observe` 局部 mutation-before-Err 不应被单个 helper 的实现推断为跨 tick 污染；现行默认 ingress 创建 transaction-local receipts，admission 错误由 `?` 上传，phase dispatcher 只在 prepare 成功后进入 commit。该证据支持当前生产 phase 的候选生命周期判断，不覆盖未来新增 caller 或独立重试边界。

`EnvelopeLedger` 公共 clone/validate/swap 路径与 private mutation-before-Err 路径须继续区分；不能把对象存在或 OOP 归属视为所有 private caller 的丢弃证明。`PreOpen` 的 take 后失败可能消耗本地 plan，权威 session 只通过准备成功后的 P9 commit 安装；不得描述为可就地重试或由本函数单独保证 authority 回滚。

09-final 对 NPC lifecycle projection 测试数为 7 的修正仍与对应历史源码审查一致。它是静态源码计数，不能写成测试执行结果。未发现这三份来源中的历史结论被当前 caller 证据反证，也没有足够证据要求扩大实现范围。

## 现行调用链与语义边界

| 项目 | 当前证据 | 判断 |
|---|---|---|
| Admission receipts owner | `ready_ingress.rs` 中 `ReadyIngress` 持有 `AccountReceipts`；正常 `capture_roots` 创建新默认值，`into_parts` 将其交付给当前 phase transaction。`validate_available_ready` 对 `admit_ready_batch(ready, receipts)?` 传播错误。 | 支持 caller-final 对当前 transaction 候选边界的有限结论；未覆盖未来 caller。 |
| Phase error/commit | `authoritative_tick.rs` 各 phase 先 prepare，失败经 `?`/`map_err` 返回；仅 prepare 成功后 precommit 校验并消费 prepared commit。 | 当前 dispatcher 不显示失败后复用 receipts 的路径。 |
| PreOpen candidate | `prepare_pre_open_tick_with_evidence` 从 authority 建 `TickShadowPlan`，执行 shadow transaction，验证后生成 P9 commit token；`PreparedPreOpenTick::commit` 消费 token。 | 历史报告的局部 take/失败与 authority 发布边界表述准确；候选失败丢弃依赖外层调用方。 |
| 交易语义 | `AccountReceipts` 与账户现金/股份 lane 是游戏内资源受理和 receipt 排序；不等价于交易所申报合法性、证券优先级或主动买卖方向。 | 本批仅审计对象所有权与生命周期，没有改变沪深 A 股规则或单位，无需新增法源主张。 |

## 决策及总账对照

ADR-0017 当前为 accepted，提供 P0–P9、计划依赖、收据和提交契约。ADR-0018 整体仍为 proposed；不能把其长期 timeline、WAL、恢复、完整观察模型等提案写成现行约束。仅适用被单独明确决定并由现行记录承接的子项：2026-09-24 的局部冲突受理规则，以及 2026-09-25 的玩家/NPC 跨 tick 入队时点与撤单反馈；它们在 ADR-0017 的修订记录中有明确交叉引用。`ready_ingress.rs` 的实际生产路径仍须按代码核验，不能因 ADR-0018 提案而假定实现。ADR-0019 的范围/容量原则不允许以任意输入配额掩盖计算边界。后续 ADR-0023–0028 中与持久化、经验、部署及发布相关的决定没有为本批 pipeline 失败路径增加新承诺。决策与 sources 的有限结论不冲突。

G16 维持当前总账状态：已接受的局部历史计划复制目标独立于 ADR-0018 整体提案；该目标尚未因本批 caller/OOP 复核而核销。参照 `agents/implementation-audit/implementation-audit-2026-10-02.md` 与 `reaudit-engine.md`，不把整体 proposed 状态或本批结果用于更改 G16。

按 implementation audit 的 G01–G68 与 Q 总账状态保持所有编号；本批没有足以核销或改写其中任何条目的生产行为证据，也未将历史 OOP 通过、对象/测试存在或静态计数作为相关需求完成证明。尤其既有长期性能、验收工具比较、产品行为和未决口径继续由各自条目负责。本批没有引入领域变更，因此 A 股规则复核范围为确认概念边界与现行 ADR 一致，不声称重新核对交易所官方规则。

## 三项门禁

1. **大 A 语义：** 未发现来源把账户内资源 receipts 冒充交易所优先级的现行实现证据；交易阶段和 P9 单次发布描述符合 accepted ADR-0017。仅 ADR-0018 中被单独明确决定的局部冲突等子项可适用，不能将该 proposed ADR 整体当作现行规则。无规则改动，本次未查询官方规则。
2. **必要性与最小范围：** 来源结论限定于归属/失败生命周期审查；现行 caller 支持其范围。没有证据需要新增 wrapper、改变失败语义或扩大到无关交易逻辑。
3. **边界与跨层漂移：** admission `Err` 的当前 caller 丢弃边界得到 dispatcher 补强；private ledger 的所有其他调用点、未来 caller、独立重试和长验收仍不由此覆盖。测试数 7 为静态计数，不是运行结果。无新增已确认遗漏。

## 未执行项

未运行测试、构建、性能验收、GUI/跨宿主验收或官方规则查询；未检查所有 G/Q 对应产品源码。本批仅基于三份完整来源、其关键实际 owner/caller/consumer、现行决策与总账状态复核，不据此宣称完整产品或长验收通过。
