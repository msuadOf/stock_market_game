# Luna16：订单簿计划与交易规则独立复核

## 范围与方法

- 目标 worktree：`.worktree/implementation-reaudit`，产品提交 `08e4fc7`，当前 `HEAD a7c7ce3`。
- 完整连续读取至 EOF：`docs/superpowers/plans/2026-06-29-orderbook.md`（868 行）、`docs/superpowers/specs/2026-06-29-orderbook-design.md`（141 行）、`docs/trading-rules.md`（123 行），合计 1132 行；并读取仓库 `AGENTS.md`、`docs/principles.md`。
- 逐任务/章节沿 `OrderBook` → `Market` → account/P3 校验、价格解析、continuous/auction caller、SaveSlot 校验与恢复追踪；对照当前大 A 规则登记及获批的符号限价、价格笼子、资源 envelope 与集合竞价简化。本记录是静态复核，未运行测试、未改产品代码、未执行 Git 写操作。

## 计划任务矩阵

| 文档任务 | 需求/边界 | 当前代码与测试证据 | 复核 |
|---|---|---|---|
| Plan Task 1，`:35–128`；Spec §3/§5，`:25–57,90–98` | Side、OrderId、OrderError；非法输入显式错误 | `orderbook.rs:23–132`；`tests/orderbook.rs:1–40` | 有实现。当前错误类型已增补进度、已成交、序号溢出、Money 错误，符合显式错误方向。 |
| Plan Task 2，`:132–233`；Spec §3，`:25–57` | Order/Trade/MatchResult 与可序列化事实 | `orderbook.rs:152–255`；`tests/orderbook.rs:42–105` | 类型扩展包含原始/已成交量、累计成交额及 maker/taker 委托 ID，支持后续费用与回执对账；未见偏离价量单位。 |
| Plan Task 3，`:237–323`；Spec §3，`:59–76` | tick 正数、价时盘口、best quote | `orderbook.rs:272–330`、`market.rs:93–124,315–330` | 买盘 `Reverse(price), seq`、卖盘 `price, seq` 仍直接决定优先级；无来源或 OrderId 优先级替代。 |
| Plan Task 4–5，`:327–638`；Spec §4，`:78–88` | 价格/数量校验、交叉、部分成交、maker 价 | `orderbook.rs:332–474`；`tests/orderbook.rs:107–315` | 撮合按最优档循环、maker 报价成交、先挂同价先成交；新余量才分配本簿 seq。当前 `sell` 交叉为 `new.price <= best_bid`（`:389–395`），语义正确。 |
| Plan Task 6，`:640–714`；Spec §3/§7，`:71–76,107–120` | 撤单返回余量、未找到报错 | `orderbook/book_state.rs:81–95`；`orderbook.rs:484–491`；`continuous_matching.rs:1316–1369`；`tests/orderbook.rs:317–380` | 簿身份索引只用于定位，交易优先仍由价时键决定。continuous caller 校验股票、所有权、live envelope 后撤单并释放对应余量；全成、他人单、未知单分别显式拒绝。Auction 路径依阶段和所有权拒绝/撤销（`stock_auction.rs:161–220`）。 |
| Plan Task 7，`:716–845`；Spec §3/§7/§8，`:59–76,107–132` | 深度只读、crate 导出、测试覆盖 | `orderbook.rs:632–691`；`market.rs:322–345,453–458`；`lib.rs` 导出；`tests/orderbook.rs:382–470` | 深度按价聚合 `u64`，买卖方向正确；盘口不暴露可变容器。行情投影读取 best/depth，不改变撮合事实。 |
| Spec §6，`:100–105`；Trading rules `:16–50` | 资金/T+1/涨跌停/笼子/费用由上层负责，市价为获批简化 | `account_validation.rs:705–767`；`market.rs:279–319`；`continuous_matching.rs:450–503,510–539`；`price_resolution.rs:9–74` | 分层仍成立：P3 校验资源/板块数量；Market 检查日涨跌停；P4 执行订单簿；市价按保护价逐档即时成交并取消余量。符号 `Highest/Lowest` 受理时解析一次，已接纳挂单保存实际价，不追价（ADR-0022 `:56–73`）。 |
| Trading rules `:24–35,77–95` | 集合竞价阶段、撤单窗口、深沪清算差异、同股真实受理时序 | `stock_auction.rs:125–220`；`stock_auction_adapter.rs:175–234,322–353`；Trading rules `:30–35,87–95` | Auction 独立清算；continuous OrderBook 不被误用作集合竞价算法。来源块/订单 ID 不定义交易优先；股票入口接收顺序与簿内 seq 决定顺序。深市昨收参照及完全同距低价规则在规则文档明确标为游戏简化。 |
| Spec §7/§9，`:107–141`；Trading rules 存档 `:112–117` | 边界测试、挂单与序号恢复，不重新编号 | `orderbook.rs:539–578`；`session.rs:2594–2627,2765–2793`；`persistence.rs:285–315,1666–1749`；`tests/session.rs:3841–3988` | 恢复保留每股 seq 与独立游标，空簿也保留游标；股票键集、订单身份、序号唯一性/小于游标、挂单交叉、价格/数量进度和 reservation 均在恢复前验证，候选簿成功后整体替换。无遗漏证据。 |

## 旧结论复核

- `reaudit-core-contracts.md:21–30` 的核心结论继续成立：价时优先、maker 定价在实际 Market/session caller 上；LimitExceeded 在簿操作前明确拒绝；OrderBook 的索引不充当价格时间顺序。
- 旧报告指出的“撮合多档过程中后续溢出可能使此前 maker 已变更”仍成立（`orderbook.rs:424–449`，`Market::place_inner` 只在 book 成功后更新 `last_price`：`market.rs:310–319`）。它不是新引入的语义变化，且 caller 使用 private tick candidate、fatal 使本 tick 候选丢弃（`continuous_matching.rs:478–503`；ADR-0017 `:32–45`）；依旧不把它误报成权威 tick 部分提交。公开 `OrderBook::place` 的通用强原子性不是原计划承诺，本轮不扩张该契约。
- 旧“零价与旧计划 ≥0 不一致”记录需按新决定优先级改写：大 A 连续规则和当前 `OrderBook` 明确拒绝 `price <= 0`（`orderbook.rs:332–368`）；`docs/trading-rules.md:18–21` 登记 0.01 元最小价位，现规则/实现没有把零价当有效委托。不能把仅旧设计稿中的 `>=0` 当现行要求。
- 新订单 ID 来自 Session 分配，OrderBook 接受调用者 ID（`continuous_matching.rs:480–490`）；与当前全 tick ID/序号分配契约（ADR-0017 `:39–40`）一致，不恢复原 Spec §2 “模块内部自增 ID”旧文字。
- 交易规则官方来源/适用日期在 `docs/trading-rules.md:97–110` 留有 2026-09-22 核对和费用表 HTTP 404 的边界声明。本次只核实现接线，不冒称重新访问或验证官方现行材料；没有发现要求改动规则本身的新证据。

## 新候选

- **固定零价/负价进入 caller 时，拒绝原因被映射成日涨跌停超限。** P3 `validate_place` 仅校验数量、资源和 lot（`packages/engine/src/session/pipeline/account_validation.rs:705–767`），未在固定价格处拒绝非正数；Fixed 价格解析原样传递（`price_resolution.rs:16–24`）。`Market::place_inner` 先比较日涨跌停并返回 `MarketError::LimitExceeded`，之后才调用订单簿（`packages/engine/src/market.rs:295–314`）；continuous caller 把此错误映射成 `RejectionReason::LimitExceeded`（`continuous_matching.rs:480–503`）。因此经公共受理路径提交 0/负价会得到“超日涨跌停”拒绝，掩盖了更准确的“非正委托价”错误原因。OrderBook 直接测试已覆盖 0 价 `InvalidPrice`（`tests/orderbook.rs:132–145`），但本次扫描未找到 Market/session 路径对该边界及其业务拒绝原因的覆盖。
- 这是拒绝归因/反馈候选，不是错误接受或静默吞错；建议在 P3 或 Market 入口显式做正价校验并使用 InvalidPrice/InvalidOrderPrice 拒绝原因，补短 caller 测试后再定级。若团队决定“所有低于跌停价均统一报 LimitExceeded”，需在规则/UI 契约明确该映射，而不能将其表述成通用 InvalidPrice 已在公共调用路径生效。

## 结论

除上述非正价拒绝原因候选外，本范围未发现价时优先、成交价、订单簿撤单、盘口只读、已批准简化、排队价解析或序号恢复的当前实现偏离。此结论为静态源码复核，不代表运行测试结果或重新完成交易所规则查证。
