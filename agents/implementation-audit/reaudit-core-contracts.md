# 核心账务与撮合契约复核

## 范围与口径

- 目标范围：指定代码更新 `b89afb3..8cf34a1` 涉及的 `Account`、`Position`、`OrderBook`、`Market` 及生产调用链；依照 [R07](coverage/r07.md)、[R08](coverage/r08.md)、[R09](coverage/r09.md) 已建立的需求文档到生产代码映射复核守恒、T+1、价时、拒绝和原子边界。
- 已用只读 `git diff` / `git show` 核对该提交区间的生产文件差异，并回看 `b89afb3` 中的撮合循环及原始订单簿 plan/spec。已先读本 worktree 的 `AGENTS.md` 与 `docs/principles.md`，并完整阅读 R07、R08、R09。此次只做静态审阅；未运行测试、构建或重回归，未执行 Git 写操作，也未改生产代码。
- R07–R09 已有的 OOP/类型组织结论不作为本篇证明。以下以真实 caller、实现和消费链为证据；测试源码只证明覆盖存在，不代表运行通过。
- A 股语义依据限于仓库当前 [交易规则文档](../../docs/trading-rules.md) 与其中记录的交易所条款/适用日期；本次未联网重查官方材料，因此不对规则的最新有效性作额外背书。

后续 `8cf34a1..dddcc31` 的 BookState diff 只把测试module移到文件末尾，生产簿写入顺序未变；私有异常fixture仍明确不是合法生产数量进度。session符号价测试调整实际受理先后并新增相反先后负控，不改变OrderBook/Market的撮合算法。已核对差异，`ebfb68b`未新增产品代码，因此以下核心结论继续适用；未运行Rust测试。

## 复核结果

### Account / Position

- **未见本范围新增的账务或 T+1 漏接。** `Account::apply_settlement` 对正数量/金额和非负费用先校验，再按方向进行结算（`packages/engine/src/account.rs:493-515`）。买入在写状态前计算总支出、现金、持仓数量、锁定数量和累计成交成本；卖出先验证可卖量，再计算费用后净现金、累计回收额与剩余数量，最后一次提交账户状态（`account.rs:326-382, 435-490`）。这是本地单账户更新的原子边界。
- `Position::sellable` 以 `qty - t1_locked` 得可卖数；日终生产入口调用 `AccountBook::unlock_t1_positions`（`packages/engine/src/session/pipeline/auction_day_end.rs:2157`），符合当日买入锁定、交易日界解锁的仓位状态契约。持仓成本与已收回额仅累计成交额、不含费用，未发现费额流入成本的路径（`account.rs:277-286, 385-397`）。
- 撮合回执按账户/股票汇总实收费用，并在账户 shadow 全部准备成功后才扩入账本（`packages/engine/src/session/pipeline/settlement.rs:76-144`）。账户间并行准备的错误在提交前传播；没有看到部分账户先写、后续失败仍保留的生产路径。
- `grant_position` 接受零股数及负成本（`account.rs:258-274`），但当前生产使用点是游戏初始仓位/存档恢复辅助流程，存档整体先校验，原则文档也明确允许编辑存档事实后重建派生数据（`docs/principles.md:28`）。这不构成本轮发现的新缺口；若该公开 API 将来变成外部未校验输入入口，应另行核定其合法域。

### OrderBook / Market

- **价时优先与 maker 定价仍在实际撮合链上。** `continuous_matching` 将已验证请求送入 `Market::place_recording`，取回逐笔成交和 maker 原状态，随后生成双方回执、订单事实和账务输入（`packages/engine/src/session/pipeline/continuous_matching.rs:478-537, 997-1070`）。订单簿依最优价和序号逐档撮合，成交价取 maker 价（`packages/engine/src/orderbook.rs:377-449`）；无新发现显示订单 ID、来源或账户类别替代簿内价时优先。
- `Market` 在调用订单簿前校验日涨跌停范围；越界以 `LimitExceeded` 返回，不进入簿（`packages/engine/src/market.rs:279-314`）。session caller 将此业务拒绝转换为 Reject receipt；订单簿、金额或投影错误则作为 invariant/fatal 传播（`continuous_matching.rs:478-503`）。没有静默改价或把系统错误伪装成业务拒单的证据。
- **撮合中途溢出后局部簿已变化是既有边界，不是本区间新引入的缺口。** 在 `b89afb3` 旧实现，撮合循环已先扣除/移除当前 maker，再进入下一档（只读 `git show b89afb3:packages/engine/src/orderbook.rs`，约 392–452 行）；当前代码将盘口写入抽到 `BookState::apply_maker_fill`，但保留同一先写后处理顺序。若后续档金额累计溢出，公开 `OrderBook::place` 仍可能返回 `Err` 而先前档已变化；`Market::place` 也只在簿操作成功后更新 `last_price`，所以直接调用者可能观察到簿/价不一致。
- 历史 plan/spec 要求 `place` 显式拒绝非法价格/数量、按价时顺序撮合并报告错误；没有承诺任意撮合内部错误都须在模块边界回滚。Market 的明确拒单原子承诺是 `LimitExceeded` 在调用订单簿前返回且不改变簿；该路径当前仍成立。故此处不登记为 `b89afb3..8cf34a1` 新缺口或违反既有原子承诺。真实 session caller 另有 private tick candidate：除 `LimitExceeded` 的业务拒单外，其他撮合错误作为 invariant/fatal 返回（`continuous_matching.rs:478-503`），tick 事务只在完整操作成功后恢复候选状态（`continuous_tick_transaction.rs:167-181`）。若今后希望公共撮合 API 具备任意错误强原子性，应作为独立契约决策与测试处理；本次不扩张旧需求。

## 旧边界与待定项

- R08 已登记的“订单价零值与旧计划不一致”仍是旧文档/规则待收口问题，不据此另报本轮新增缺口。现实现拒绝 `price <= 0`（`orderbook.rs:356-368`）；本次未找到可将该点归类为已确认交易规则的新依据。
- 账户守恒中手续费累计、卖方按成交回执计费封顶等已在 `docs/trading-rules.md` 标作游戏简化，本篇不将其误报成现实清算语义错误。
- 对其余已在 R07–R09 记为已实现或被新决策取代的承诺，本轮未发现新的 caller 断链；`Account` 的 COW 改造保留了先完整计算、成功才写字段的原子结算顺序，`Position` 不变量和 T+1 锁定方式未变。以上“未发现”仅限静态检查范围，不代表测试验收完成。
