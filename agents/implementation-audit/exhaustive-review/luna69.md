# Pipeline 三份审查记录全文 EOF 独立复核

复核基线产品提交：`08e4fc75b52a71a3262a8a938c57b44f8b5b4960`；当前 checkout `a7c7ce357bdc9f88c03633744b2d5815db49e9b2` 包含该提交。工作树原有其他审计文件，本记录为本任务唯一新增文件。只做静态阅读，无产品/Git 写操作，无测试、编译或长测。

## 全文行数与章节矩阵

| 文件 | EOF 行数 | 全文覆盖章节 | 重点重核 |
|---|---:|---|---|
| `agents/oop-refactor-implementation/pipeline/review-auction.md` | 80 | 批次/范围；新增源码 EOF；六文件内容绑定；结论；大 A 语义与依据；必要性与范围；错误顺序、部分写入与边界；测试审查与限制 | Auction owner/caller、P9、拒绝/receipt、DayEnd 局部失败和外层 candidate 原子性、旧函数接线 |
| `agents/oop-refactor-implementation/pipeline/review-continuous-core.md` | 78 | 范围；结论；有效发现与关闭；大 A 语义和依据；必要性/范围/测试边界；文件绑定；fixture 收口复核；manifest | Continuous receiver 与 finalizer caller、P0/P1/P4 receipt 顺序、consuming failure、NPC lifecycle、DayEnd |
| `agents/oop-refactor-implementation/pipeline/review-resources-projections.md` | 51 | 范围与依据；结论；测试与证据边界；最终 SHA256 门禁 | P1 immutable snapshot、P4→P5→Settlement、Retail projection、SelfView、producer/validator 独立性 |

三份记录均从首行读到文件 EOF。其“完整 diff/manifest/hash 已由先前 reviewer 核对”的历史证据只记录为旧审查主张；本次没有重算全部历史 diff 或代签 SHA，也不把源码复读称作编译/测试证据。工作区 HEAD 是产品提交的后代，不据此宣称三份历史 manifest 覆盖当前所有提交。

## 当前执行链与 P9 对照

当前 phase enum 明确 rank 0–9：`ExpiryShadow`, `SealAllocationSnapshot`, `DecisionShadow`, `AccountValidation`, `StockProcessing`, `ReceiptAggregation`, `SettlementShadow`, `DerivationAudit`, `PreCommitValidation`, `CommitTick`。因此文中“P9”若指 TickPhase rank 9，对应最终 `CommitTick`，并非 `Settlement` 或 receipt application；`packages/engine/src/session/pipeline/phase.rs` 没有名为 `P9` 的 variant。交易侧 P0/P1 则是各旧审查沿 ADR/领域管线使用的阶段称呼，不能与 enum rank 混为一谈。

Auction 外层的真实 owner 是 `prepare_auction_tick` 创建 tick shadow、调用 `apply_tick_shadow_auction_transaction`，成功后 `prepare_tick_shadow_plan_commit_with_evidence` 创建 prepared commit，再由 `PreparedAuctionTick::commit` 安装。内层先 `take()` decision snapshot 和 private session candidate；只有 `apply_session_auction_transaction` 成功才 `restore_success(candidate)`，随后扩展 receipts/events/finalizers。错误路径会让整个 shadow 失败，不代表其中的 receiver 都具备自身 rollback。

Continuous 外层相同：`prepare_continuous_tick` → `apply_tick_shadow_continuous_transaction` → private session candidate；成功才 `restore_success` 并追加 output receipts/events。真实生产 finalizer caller 位于 `continuous_tick_transaction.rs`，在 `finish_continuous_shards` 后调用 `finalize_continuous_tick`；Auction 完成 caller 是 `stock_stream.rs::finish_auction_shards`，将 boundary 原字段传入每个 shard 的 consuming `finish`。这重证了历史接线主张，不以测试 seam 冒充生产调用。

## Auction 记录重核

旧文主张 receiver 拥有 auction shadow apply、projector 缓冲区，coordinator 仍做分股/worker/安装；当前 Auction tick transaction 仍调用同一 `apply_session_auction_transaction` 路径，`finish_auction_shards` 仍对每个 coordinator 调 consuming finish，`apply_finished_candidate` 汇入 unified ReceiptAggregation/Settlement/lifecycle。交易语义复核仍依赖仓库登记的沪深规则与游戏简化；本次未联网复核官方材料，不能将旧文的制度日期/费率依据升级为本次再次核验。

```rust
let resources = plan.decision_resources.take().ok_or_else(|| {
    AuctionTransactionError::Preparation(invariant("P1 decision resource snapshot is absent"))
})?;
let mut candidate = plan.state.take_session()?;
let output = apply_session_auction_transaction(&mut candidate, resources, roots_override, &preceding_receipts)?;
plan.state.restore_success(candidate)?;
```

上例来自 `auction_tick_transaction.rs` 的当前代码（为可读性折行）。说明失败可留下已消耗的临时 plan 部分状态，但 tick authority 在 prepared commit 前未安装；不可描述为 receiver 局部事务回滚。集合竞价拒绝/P4、OrderId 消耗与 P3 snapshot 不回补的主张属于旧文已标出的游戏语义；本轮核对到本批 owner/caller 没把资源入口改为 settlement/P4 release。具体沪深规则仍应以 `trading-rules.md` 登记的“简化/不支持”为准。

DayEnd 原子边界也仍须精确限定：历史文已承认计划同步失败可能在 candidate 中发生 T+1 unlock、`mem::take` plans 后失败；day overflow 在当日日 K 提交后检测。其安全边界是外层丢弃 private candidate，不是 `TradingDayEndTransition` 自己 rollback。当前 Continuous finalizer 仍导入并调用 `TradingDayEndTransition`；限定搜索 `auction_day_end.rs` 与 `continuous_tick_finalizer.rs` 未见已删除旧自由函数的生产引用。由此复证“旧调用已改接 transition”，不扩大为所有 DayEnd/交易流程无遗漏。

旧文关于局部顺序的核对点保留有效：shadow receiver 内校验、created envelope、价格/业务拒绝、receipt、lifecycle 的部分推进可随该 shadow 丢弃；finish 是 consuming；跨股票成功结果安装前完成错误收敛。当前同一调用结构支持这些限定，未找到把 rejected receipt 当成交回补预算的接线。审查记录虽声称相应边界测试覆盖，但本次未执行，不能说通过。

## Continuous、receipt 与事实消费重核

旧文列出的 state caller 遗漏已更正（`game.state.last_retail_decisions` 与 `last_retail_order_events`）；当前代码结构中 state 收敛在 `CommittableSessionState`，旧的 `game.last_*` 访问不应继续作为生产接线。NPC lifecycle 的注册/移除、`PlanChainFactConsumption` 的 prepare/commit 与 Contains-only finalizer 路径仍需按 owner 边界理解：生产 finalizer 在连续 transaction 内，receipt 统一完成后才 Settlement，lifecycle/DayEnd 在该 candidate 上继续投影。旧文的错误优先序是静态实现结论，不是任意失败时全局无副作用的承诺。

```rust
let receipts = apply_session_receipt_transaction(session, created, receipt_batches, terminals)
    .map_err(AuctionDayEndError::ReceiptAggregation)?;
let _settlement = apply_session_settlement_transaction(session, &settlement_receipts)
    .map_err(AuctionDayEndError::Settlement)?;
```

当前集中收据点为 `auction_day_end.rs::apply_finished_candidate` / Continuous finalizer 共同所用的 `receipt_aggregation::apply_session_receipt_transaction`；该函数先校验 session 与 ledger cursor 相等，再在 private ledger candidate 安装 created、应用 receipt、复核完整证据和 cursor，成功才替换 ledger 并推进 session cursor。Settlement 只读已聚合 receipts 的 charged 实收，不重新计算名义撮合费用；业务拒绝/Release 不作为 Fill 结算。

旧文所述 incoming ordinal、maker 顺序、买方后卖方 receipt、P4 fee cap、释放不回补、P0 后 P1 snapshot 的方向符合当前“single receipt aggregation → settlement”的所有权关系。计数、费用 cap 与具体字段算法仍需以代码/ledger validator 核对；历史文独立 validator 覆盖不表示所有 fee 分项已有统一参数化等价测试，旧文也把 N07 的对照留作补充证据。

Continuous 的局部失败边界：某个 account/shard/lifecycle projection 失败时可有临时 candidate 已被部分改写，真正丢弃边界是 tick shadow；`finalize_continuous_tick` 的直接 `&mut GameSession` 本身不等于 clone-and-commit。DayEnd 的行情/深度冻结、receipt/Settlement 时钟和之后 T+1 解锁顺序是旧记录主张；当前 finalizer 函数签名明确接收 disposable candidate，交易路径从 continuous transaction 调用。测试和 historical manifest 不是本轮 runtime 证明。

## Resources/projections 重核与候选反证

当前 `DecisionResourceSnapshot` 文档与私有字段仍写明 Post-P0 snapshot 供 P2 观察、P3 预留；`AccountBudget` 从 snapshot 读取 cash/share lane，`adopt_*` 只更新同一资源维度。当前代码没有由这个提取自动引入 P4 release 回灌 P1 的路径。此结论仅核对资源 owner 与接线，不代表现实交易中资金可用性的完整建模，也不扩大到 A 股规则认证。

`ReceiptSettlementPlan` 按 receipts 建立临时 totals，先准备 shadow accounts，caller 在准备成功后扩展权威 AccountBook；`RetailProjection` 当前仍并行计算全部账户 jobs、按 account 顺序 collect 首个处理错误，之后才扫描 final-position reconciliation error。这支持旧文特别记录的错误优先级，不把其写成任意跨层错误顺序。

```rust
let prepared: Vec<AccountProjection> = prepared.into_iter().collect::<Result<_, _>>()?;
for projected in prepared {
    if let Some(error) = projected.final_position_error {
        return Err(error);
    }
}
```

SelfView 的 `raw_cash - reserved + replaceable` 是观察层可用现金；旧记录正确限定其不进入 P1 执行预算。Settlement 的零量 Fill 局部 skip 也不能解释为生产接纳：旧记录已注明 upstream ledger audit 会约束累计 movement。producer/validator 的独立实现有利于避免同一算法自证，但“独立”不自动证明两者每个边界已统一参数化比较。

### 可疑旧结论与反证

| 候选 | 支持旧文/当前代码的证据 | 反证与结论 |
|---|---|---|
| 日终函数接线陈旧 | Auction 文声称删旧自由函数、Continuous 改用 transition；当前 `continuous_tick_finalizer.rs` 导入 `TradingDayEndTransition` | 当前调用点仍在 finalizer，限定生产文件搜索无旧名字引用；暂未证实接线回退。此结论仅限该路径。 |
| settlement/receipt 局部原子性被夸大 | Receipt aggregation 内有 private ledger candidate；Settlement 准备 private account copies | 相邻 projection 或 DayEnd 可在 enclosing candidate 上部分推进；外层 tick shadow 才是 authority 边界。旧文已披露 DayEnd 局部部分写入，未发现把整个 receiver 宣称自回滚的依据。 |
| 零量 Fill skip 代表合法收据 | Settlement 对 qty=0 不做账户结算 | 上游 ledger `validate_audit` 负责累计证据合法性；局部 skip 不能替代 upstream admission。旧文已明确此分层，候选不成立。 |
| “P9”代表 Settlement/收据 | 项目有 P0/P1/P4 等领域阶段说法 | `phase.rs` 明确 rank 9 是 `CommitTick`；不得把它和 ReceiptAggregation(rank 5)、SettlementShadow(rank 6) 混淆。 |
| 三份旧 manifest 证明当前完整树 | 记录登记 SHA / manifest 和各自审查范围 | manifest 绑定的是其列明内容与基线；当前 checkout 是基线后代。本次只重查需要的 owner/caller 与边界，不重新计算全部 manifest，不能把旧记录覆盖推广到其它后续变化。 |

## 复核结论与限制

逐章复核没有发现能由本轮证据支持的新增有效交易语义缺陷，也没有找到旧文所称旧 DayEnd 接线在当前生产路径复活的证据。保留三项限制：未运行测试/编译；未重算三份完整历史 diff、全部文件 SHA 或重新核官方来源；本静态复核只对本文逐项追过的 pipeline owner、真实 caller、收据与 candidate 原子边界作判断。不得把旧文结论扩写为全批 build/runtime 通过、TDD 红绿历史已证或 A 股规则本次已在线复核。
