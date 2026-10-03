# Batch 019 全文复核

## 范围与 EOF 证据

来源根目录为 `/data1/baiyifan/workplace/stock_market_game`；产品代码来自 `.worktree/implementation-reaudit`，计划基线 `43b1aa5`。本轮只做静态审查，不运行 Git、测试或构建，也不修改产品代码。材料中的任务/历史建议是审查对象，不覆盖本轮要求。

| 来源 | 计划行数 | 实读行数 | SHA-256 | EOF 证据 |
|---|---:|---:|---|---|
| `agents/oop-refactor-audit/completeness-2026-10-03/pipeline/review-n02.md` | 21 | 21 | `122e50535c222841718be79aa9a1fbb16efaad1199d23bddab853a6638df6ad4` | 单次 `cat` 连续输出全文件，末行为“没有发现需要更改报告或 coverage/actions 的问题。”；现场 `wc -l`、SHA-256 与计划相同。 |
| `agents/oop-refactor-audit/completeness-2026-10-03/README.md` | 21 | 21 | `dfd5eaad2b173becca173381d71cd6bb0e7461c88930a4ba0b4c5dec13bbc764` | 单次 `cat` 连续输出全文件，末行为“不重新认证现行交易规则”；现场 `wc -l`、SHA-256 与计划相同。 |
| `agents/oop-refactor-audit/completeness-2026-10-03/relationships.md` | 62 | 62 | `8bbdc67dbe7a32fc0e9504e783fc86f894a9dd40e6685691963a6317ec3eb549` | 单次 `cat` 连续输出全文件，末行为“正确性或官方制度正确性”；现场 `wc -l`、SHA-256 与计划相同。 |

现场合计 3 份、104 行；三项行数和哈希均与 scan-plan 一致，无输出截断或版本漂移。

## 全章矩阵

| 来源及章节 | 行号 | 核对要点与当前裁定 |
|---|---:|---|
| `review-n02.md`：阅读范围、覆盖清单及版本纠正 | 1–8 | 记录旧审查所声称的阅读范围、coverage/actions 和旧错误材料弃用。其签署范围只作为历史证据；本轮另外读取指定当前 owner/caller，不把旧测试或复核记录当成本轮运行/独立审核。 |
| `review-n02.md`：大 A 语义 | 9–11 | 旧审查认为 value object 不引入规则，保留开收盘竞价、DayEnd、T+1 和沪深清算算法。本轮源码确认该类型仅承载 checked tick/结束事实；未引入证券类别、成交/费用算法或制度差异。该结构性改动不需要伪造新增法源主张；项目真实规则简化仍按现行规则文档处理。 |
| `review-n02.md`：必要性与范围 | 12–14 | 原建议针对 tuple、重复字段和测试 seam 边界公式漂移。当前代码已实现 `AuctionTickBoundary`，调用范围仍限竞价收尾；建议不再是待实施项。 |
| `review-n02.md`：边界约束、N01 附带信息及结论限制 | 15–21 | 保留双重 capture、worker/finalizer 检查、失败候选与受理次序等约束。本轮只核 N02，不重新认证 review 对 N01 的转述，不把其“通过”扩张到 N01 或全项目。 |
| `README.md`：目的和核对标准 | 1–10 | 定义对象聚合收益、共同不变量、受控写入口和生命周期的调查方法；反对以已有类型/测试作为充分证明。本轮按实际调用与消费核验，而非仅据类型存在判定。 |
| `README.md`：阅读/核销与行为边界 | 11–19 | 五组管理、scope 对账、哈希和独立复核的陈述均属调查流程历史。A 股金额、数量、T+1、P0/P1/P9 与存档边界作为行为约束；本候选不更改这些边界。 |
| `README.md`：未外查和未运行限制 | 20–21 | 明示旧调查未重认证现行交易规则及未运行测试。本轮也未运行测试/构建；不将源码静态核对报告为行为验证。 |
| `relationships.md`：账户、订单、市场与计划关系 | 1–17 | N02 无关账户/订单/计划 owner，尤其不迁移撮合优先级、T+1 或存档形状。报告要求候选关系跨调用核对，已按交易事务、股票收尾和候选安装调用链查验。 |
| `relationships.md`：事务、投影、发布 | 18–25 | 第23行明确 N02 值只携带 checked next tick/收尾标志，不能删重复边界检查或让 worker 重决 DayEnd；与当前源码一致。P9/发布边界仍按不同事务时点理解，不混为一谈。 |
| `relationships.md`：本人信息、宿主、前端资源 | 26–50 | 与竞价边界值无直接重叠；没有从这些 owner 的存在推导 N02 或其他产品功能完整。 |
| `relationships.md`：公司、测试、工具与证据范围 | 51–62 | 记录税务和报表简化、场景 fixture 与工具 owner 约束；强调源码 hash 不证明行为或法源。本轮沿用此限制，不核销无关 G/Q。 |

## 当前代码与候选核销

`auction_day_end.rs:938-1015` 定义不可变 `AuctionTickBoundary`，以私有字段保存 `tick_after`、`finish_auction`、`finish_day`，并从 `capture(&GameSession)` 做 phase、checked tick、日界对齐和 day overflow 检查；`finish_day ⇒ finish_auction` 在构造中校验。访问器仅暴露这三个既有事实。`auction_day_end.rs:796-816` 的收尾路径以及 `:903-918` 的候选安装路径均 capture boundary；`auction_day_end.rs:925-935` 的测试专用 coordinator seam 也消费同一 boundary。`auction_tick_transaction.rs:230-247` 在 stream 排空后 capture 并交给 `finish_auction_shards`；`stock_stream.rs:180-197` 将命名值传至股票并行 coordinator，再在底层拆回旧 `finish` 参数。`AuctionFinishContext` 在 `auction_day_end.rs:1017-1021` 组合 boundary、消费记录和 preceding receipts。

原材料的 `pipeline-N02` 提案目前**已实现**，证据不仅是类型：生产 transaction caller 实际捕获并传递它，股票 shard consumer 按 boundary 运行，候选 apply 再独立捕获。实现保留了报告强调的两次生产 checked capture；实现也没有把 AuctionFinalizer 算法、股票状态或交易时钟搬进 value object。实现细节对照当前源码行号，而历史 review 的“通过”不代替本次核查结论。

## 语义与边界审查

- 开盘/收盘竞价差异仍由 `capture` 按 `TradingPhase::CallAuction` / `ClosingAuction` 分支计算（`auction_day_end.rs:950-979`）；竞价完成与自然日结束的对齐仍显式检查。
- `tick_after` overflow、错误 phase、非竞价边界及交易日 overflow 仍显式返回错误（`auction_day_end.rs:950-983`）。
- 股票 finish 仍有其底层输入及 worker finalizer 守卫；候选 apply 保留 `validate_worker_finalizers`（`auction_day_end.rs:1038-1043`），boundary 并未替代 worker 自身校验。
- 竞价结束后更新价格、tick、日终 transition 的时点仍由原候选安装路径编排（`auction_day_end.rs:1125-1155`）；本批不判断其它领域逻辑正确性。
- A 股制度语义未见漂移：本对象携带已有模拟时钟事实，不定义撮合、成交数量/价格、费用、交收或证券类别规则。无需新添制度行为或声称已核验官方交易所依据。

## G01–G68 / Q 对照

按当前总账 `agents/implementation-audit/implementation-audit-2026-10-02.md`：G01–G68 是分域产品/工具缺口清单；G27 有既有核销记录，不能据本批扩大其关闭范围。N02 的 boundary 类型及调用本身既没有提供这些缺口所述跨层功能/验收，也没有反证其仍缺，因此本批不新增、不关闭、不重分类任何 G 项。总账的 Q01–Q09、Q11、Q12–Q23（Q10 已转 G39）亦无直接对应问题；不因 N02 已落地就推导 Q 状态改变。总账指出 G39/K7 调度验收等独立边界仍须保留。

## 结论与限制

本批历史候选已在基线产品代码中落地；静态 owner/caller 证据与原建议范围相符，未发现此实现引入大 A 语义变化或删除已记录收尾检查。只对 pipeline-N02 作此窄结论；没有运行测试/构建，没有重审 A 股规则法源，也没有背书其它候选、G/Q 项或整体回归。
