# Q01：Web 金额消费接线

本批仅把现行 Money 编码切换为规范十进制整数分字符串，不改变 A 股交易制度、初始现金数额或多人市场规则；不增加版本标记、旧 Number 接受路径或迁移。

## 实施范围

- `position-valuation` 使用 BigInt 计算累计净投入、bankers 舍入成本、市值和浮盈；结果显式检查 i64，超过 JavaScript 安全整数但仍在 i64 内的金额保持精确。
- `LocalRefreshViews` 的可用现金、持仓汇总、总资产及盈亏使用 Money 工具；快捷仓位按 BigInt 整手与整数分母计算，不经浮点比例推导委托数量。
- `AutoOrderManager` 比较真实整数分值，不按字符串字典序触发条件单。
- Redux 的 `AutoOrderUI.triggerPrice` 与执行器共享 Cents 字符串类型，消除展示状态仍写 Number 的跨层漂移。
- 玩家活动委托 DTO 严格检查 Money 字符串，拒绝 Number；订单输入消费字符串价格。
- 默认 SessionSetup 只改变金额编码，保持原来的现金、手续费和股票初价。
- 行情列表用原始分字符串、精确价格文本和数值比较器；涨跌幅只在末端转换为显示比例。
- 图表坐标、MA 与指标保持末端呈现 Number，不用于账务或订单；原始当日 OHLC 分值随 Kline 投影保留，报价摘要不从 Number 反算。投影相等判断同时比较原始值，避免相差一分被 Number 投影抹去。

## 验证

新增 `money-wire-ui.test.ts` 四个短行为测试：超安全整数持仓精度、跨数字长度条件单触发、活动委托拒绝 Number 且保留大金额，以及相同 Number 坐标不能吞掉原始 OHLC 一分变化。用 `timeout -k 1 10 node --experimental-strip-types apps/web/src/app/money-wire-ui.test.ts` 实测全部通过，四个 case 均设置 10000ms timeout。

初始红测用 Node 子进程 test runner 执行，仅得到 file-level 失败，无可用行为诊断，因此不把该次失败宣称为完整行为红证据。后续直接执行测试文件得到具体行为通过结果。生成 Money 类型尚未刷新时的 tsc 出现预期跨层类型错误；最终类型检查及独立完整 diff 复核由协调任务在类型和 fixture 接线后执行。未运行完整回归。
