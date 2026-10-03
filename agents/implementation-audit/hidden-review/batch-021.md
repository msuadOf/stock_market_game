# 批次 021 独立复核

## 阅读完整性

| 文件 | 实读行数 | SHA-256 | EOF |
|---|---:|---|---|
| `/data1/baiyifan/workplace/stock_market_game/agents/oop-refactor-audit/completeness-2026-10-03/session/independent-review.md` | 19 | `f487aaa8851a60a303a5fe8b526998df44321808144159329679828271daf559` | 是，连续全文至末尾 |
| `/data1/baiyifan/workplace/stock_market_game/agents/oop-refactor-audit/completeness-2026-10-03/session/parts/01.md` | 15 | `51a1977419282669b3cf9ba661527f7e399cd56cc2727ad88d046d8c61c13cb4` | 是，连续全文至末尾 |
| `/data1/baiyifan/workplace/stock_market_game/agents/oop-refactor-audit/completeness-2026-10-03/session/parts/02.md` | 9 | `0cf63ea0dfa9d8b61400dcf06a5e7131204ec8596cc999aef656b66fbd8b257d` | 是，连续全文至末尾 |

已读根 `AGENTS.md`、`docs/principles.md`，核对 ADR-0015、ADR-0017、ADR-0018、ADR-0025、OOP 汇总及实现审计 G01–G68/Q 总账。三篇源文件位于主工作区 `/data1/baiyifan/workplace/stock_market_game`；指定目标 worktree 中没有这些源文件，故本记录保留主工作区绝对源路径。指定审查 HEAD 为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。未执行 Git 写操作、测试、构建或规则查询；仅新增本批记录。

## 结论

原 review 对 session-N01/N02 的审查结论是对调查材料和当时候选的历史签署，并非产品验收。当前指定 HEAD 已有两候选对应结构及真实生产调用。N01 在 `ParentOrderPlan` 内实现连续与竞价各自契约的 child 局部转换；N02 用 `ProtocolState` 统一 Session/Checkpoint 配对状态。静态结构核对支持“已实现于当前树”，不证明全部旧候选测试边界已运行或所有事务行为验收通过。

parts/01 的遗漏意见指出 `NpcAttentionState` 与候选堆需要作为调度聚合一起操作。当前 `CommittableSessionState` 确实同时纳入 `npc_attention` 与 `attention_scheduler`，存档恢复也从事实表重建索引；但状态更新、入队和调度仍由 `GameSession`/流水线分散编排，`NpcAttentionScheduler` 本身只拥有堆。因此这是可选的内部内聚增强，尚不能说已完全实现该意见；也没有证据把它升级成产品缺陷或新的权威状态需求。建议保留为原有 CommittableSessionState 范围内的候选增强，不重复计数或新建第二权威状态。

## 候选及章节族状态

| 章节/候选 | 当前状态与路径证据 |
|---|---|
| Session `session-N01` | 已落地。`ParentOrderPlan` 的受理、真实成交、匹配 child 清除、checked 竞价转换、同向限价修订及续发计算见 `packages/engine/src/session/execution.rs:136`、`:163`、`:180`、`:217`、`:225`、`:241`、`:268`、`:283`、`packages/engine/src/session/execution/orders.rs:82`、`:198`。生产 caller 仍由 Session 适配器编排；`execution/records.rs:35`、`:88`、`:115` 和 `pipeline/auction_day_end.rs:1847`、`:2076` 使用这些转换。连续路径保留 assert/expect 与写入时点，竞价路径保留 typed error 和先校验后安装的差别。金额/数量及撮合权威没有转移。 |
| Session `session-N02` | 已落地。私有 `ProtocolState` 聚合 `GameSession`、历史、`PublicationFactCursor`、日终档和两类发布缓存，显式 checkpoint clone 与单次 state rollback 见 `packages/engine/src/session/protocol/civil/session.rs:8`、`:18`、`:59`、`:67`、`:71`。未把事务承诺扩展到多个 facade 调用；宿主组合批次仍依赖外层 checkpoint。 |
| Session 注意力调度增强（parts/01） | 部分落地/增强仍可考虑。权威状态表和堆都在同一可提交 owner：`packages/engine/src/session.rs:1147`、`:1160`；初始化分别写入两者见 `:1937`、`:1945`，恢复由状态表重建堆见 `:2859`，提交复制二者见 `:4099` 后的 shadow/commit 配对。scheduler 自身只管 heap 与 stale-key 核对，见 `packages/engine/src/session/attention.rs:263`、`:294`、`:321`；snapshot capture 在成功更新个人状态后再 enqueue，见 `packages/engine/src/session/pipeline/decision_snapshot_capture.rs:103`、`:113`、`:228`。没有改动理由要求把持久化事实放入 scheduler，也不应改变 `AccountId` 去重/排序语义。 |
| AccountBook（parts/02） | retain 再证。其分页账户目录、访问/写入、COW、验证缓存与 T+1 解锁仍是明确 owner；本批未见新调用链反证，也未发现账户数据复制出第二权威目录。该篇针对 335 行旧源码的历史 SHA 只作为历史阅读记录，不把旧指纹冒充当前源码指纹。 |

## 总账与语义边界

- 实现审计 G01–G68/Q 中没有条目可由 `ParentOrderPlan`/`ProtocolState`/OOP 抽取本身核销；G16 所述长历史复制仍是不同性能/所有权问题，OOP 类型存在不能证明副本消除。Q 项继续按总账状态处理，本批不重新裁决。
- ADR-0015 保留真实母单 lifecycle、成交/撤单须依真实订单事实回写等语义；其早期策略限定已由后续决策替代。ADR-0018 对跨来源受理顺序的后续修订优先于 ADR-0017 的历史全局来源顺序描述。对象提取不得把对象/账户排序变成交易优先级。
- ADR-0025 将公共存档限定为完整日终档；`ProtocolState` 的内存 checkpoint 和母单临时运行态聚合不授权公共日内快照/活动委托存档。母单金额为分、数量为股；整手、T+1、费用及撮合继续由既有交易层负责。本轮没有查官方交易规则，不宣称重新认证规则基线。
- 原始 parts 中属于历史探索意见的内容不自动变成当前动作；候选状态以当前代码、正式总账与后续 ADR 为准。本批静态阅读没有要求修改产品代码。

未运行产品测试或构建。当前结论仅核对指定源码路径中的类型、方法与调用归属，不替代实施后的独立 diff 审查或测试验收。
