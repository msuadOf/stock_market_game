# 批次 12：完整性领域复核记录再审

## 范围与全文身份

依据 `scan-plan.json` 批次 12，主仓来源根为 `/data1/baiyifan/workplace/stock_market_game`，现行产品基线按任务指定从 `.worktree/implementation-reaudit` 读取，标识 `43b1aa5`。逐份从首行读取至 EOF；实际行数与计划值相符，SHA-256 与清单相符，无截断或未读尾部。详情见同目录 `batch-012.json`。

| 来源 | 行数 / SHA-256 | 全文覆盖矩阵及结论 |
|---|---:|---|
| `agents/oop-refactor-audit/completeness-2026-10-03/domain/review-b.md` | 34 / `b9bba5fbbdaf00ebf9d97ac0075c279249648e7e22c8a990414a6a4c2cfd06b2` | 结论、A股语义、必要性最小范围、边界测试与复杂度、复核范围：全读。N02 要保留 Order 错误优先级及非原子撮合写入次序；N03 要限价状态写口、保留恢复/竞价语义并迁移非法状态测试。属于静态候选复核，不是实现或测试通过记录。 |
| `agents/oop-refactor-audit/completeness-2026-10-03/domain/review-c.md` | 26 / `9840e0d7dd16dbfccc5618a4f4c123c25538a30787b3780572e4ef36becc26b5` | 三门结论、N04 逐项核对、审计映射元数据阻断项：全读。N04 是三行业构造期开局子账种子守卫内聚候选，明确仍为行业各自黑名单而非统一白名单；原记录还指出 actions anchor 与哈希更正状态的问题。 |
| `agents/oop-refactor-audit/completeness-2026-10-03/domain/review-closure.md` | 25 / `0a1cd6bdfe61ea115738ffd77192cc5d2e0ba113064d18d340249ab46015245d` | 资料与修订闭合、三门结论：全读。报告记录 147 个全读路径、34,143 行、动作映射及两个哈希更正状态已闭合；明确候选未实施、未运行测试/构建、未重取官方法源。 |

## 现行代码与调用链核对

- **N02 / Order 进度：** 当前 `OrderBook::place_inner` 先拒重复 ID，再在 `orderbook.rs:356` 调 `order.validate_progress()`，之后才校验价格（`orderbook.rs:351-368`）。撮合仍以 maker 价、盘口价时顺序处理（`orderbook.rs:370-424`）。逐笔字段更新顺序在 `orderbook.rs:428-438`：先计算 maker 成交额，更新 taker 成交额与数量，再应用 maker 盘口变化；不应把普通值对象提取描述成自动具备失败回滚。当前 owner/caller 仍是 OrderBook/Market 与 session 撮合路径；不因方法搬迁或 OOP 归属主张撮合语义改变。review-b 要求的具体错误顺序条件应保留为实施约束，但其本批证据不构成新的 G/Q。
- **N03 / Market 写入口：** `Market` 价格状态恢复口 `restore_prices` 是 `pub(crate)`（`market.rs:258-262`），竞价更新口 `apply_auction_price` 是 `pub(crate)`（`market.rs:264-267`）；随意设置价格的 fixture 仅 `cfg(test)`（`market.rs:269-277`）。现行 caller 是 `GameSession::restore`（`session.rs:2762`）和竞价收盘路径（`session/pipeline/auction_day_end.rs:1424`）。`Market::place_inner` 仍先检查涨跌停再进簿，超限拒单不改簿；成交后才写 last price（`market.rs:295-319`）。这是窄化 owner 写入口且保留真实生产 caller 的现行形状。N03 旧 review 特别指出的 integration fixture `i64::MAX` 问题，现可由私有测试 fixture `fixture_set_last_close`（`market.rs:274-277`）和同模块测试调用（`:533,548,569`）表达，无须公开任意 setter。
- **N04 / 三行业 Config：** `BankConfig::check_opening_lines`、`InsuranceConfig::check_opening_lines`、`RealEstateConfig::check_opening_lines` 均由相应 `Books::new` 唯一调用，分别在 `bank/mod.rs:109-119`、`insurance/mod.rs:104-114`、`real_estate/mod.rs:82-97`；实现位于各自 `config.rs`（银行 `:26-51`，保险 `:45-65`，地产 `:55-81`）。保留各自 10/6/11 项禁止科目、按输入顺序返回首个命中及构造器既有校验先后。配置仍公开持有 `opening_lines`，不是持久化形状变化。候选是可选内聚改善，不是运行时缺陷，不登记 G/Q。

## 复核与判断

1. **大 A 语义：** 三份材料没有提出改变 A 股撮合、交易价格或数量单位的候选。现行代码仍采用价时优先、maker 成交价、涨跌停前置拒单；本次为静态复核，没有重查交易所法源，不对最新有效性作额外背书。会计 N04 是构造校验范围，不冒充交易制度结论。
2. **必要性与范围：** N02/N03/N04 是对象职责与入口收口建议，已有真实 owner 和生产 caller。当前基线已经体现候选迁移形态；不应把既有 refactor 当成新产品缺口，亦不要求恢复 OOP 前公开写口。
3. **遗漏、反证与剩余发现：** closure 已明确报告映射、coverage 与哈希更正阻断项闭合；批次 12 中没有可复核的新增 G/Q。反证是现行生产调用链实际使用受限恢复/竞价入口，配置各守卫仍在各行业构造路径执行，领域候选本身未显示跨层漂移。发现一项不升级为 G/Q 的源码文案准确性问题：三份 config 字段注释声称“只允许现金 + 权益侧科目”或“唯一合法落点”（`bank/config.rs:18-20`、`insurance/config.rs:37-39`、`real_estate/config.rs:43-45,56-59`），但对应函数实施的是行业各自黑名单（后续其他科目仍由 `Books::post_batch`/Chart 验证），并非这些函数本身执行白名单。建议后续将注释改成准确描述黑名单与下游验证，不能在本次审计中假定接受集合已是白名单。

## 限制与 EOF 证明

本批只读审计材料与现行代码，没有运行 Git、产品测试或构建，也没有改产品文件。三份全文分别由 `cat` 连续读出；`wc -l`、`sha256sum` 与 plan 中行数和摘要一致，完整文本输出均包含末尾章节/最后一行，证明抵达 EOF。没有靠搜索结果替代全文；`rg` 仅用于定位当前 caller 和核对行号。未核实交易所规则现行效力；静态代码行不能证明运行时测试通过。
