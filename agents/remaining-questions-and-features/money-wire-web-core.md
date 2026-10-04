# Q01：Web Money 基础工具与严格解析

## 范围与规则

- 用户选定 Money 跨边界使用十进制整数分字符串。Web 运行态同样保留字符串，账务算术使用 BigInt，结果显式检查 i64 范围。
- 只接受 `0`、无前导零的正整数、无前导零且非零的负整数；拒绝 JSON number、负零、正号、空白、小数、指数和溢出。没有版本字段、兼容 alias 或迁移。
- `utils/money.ts` 提供严格解析、加减、股数乘法、比较、银行家成本除法、无浮点的元输入/输出。`moneyToChartNumber` 与 `ratioMoney` 只供图表与比例呈现，不输出交易金额。
- `utils/format.ts` 的 `yuan` / `yuanSigned` 精确显示两位元；`formatCentsAmount` 精确处理有符号 Money 中文数量级；原 `formatDecimalCentsAsYuan` 继续处理 u64 成交额，不把它缩为 i64。
- `trade-input.ts` 的价格输入和涨跌停计算改用字符串与 BigInt，保留原 A 股正数四舍五入、一价位保护、板块差异和股数规则；不是新的交易制度。
- 存档 `market` / `orders` / `save-snapshot` / `runtime-state` / personal schemas 和宿主 protocol 共用严格 Money 边界。现金、冻结资金、成本、费用的领域非负约束使用 BigInt 比较，不作字符串字典序比较。
- 股数、基点、日期、序号、数量和比例不改为 Money。AccountingAmount 的两位小数元字符串与 Money 分字符串保持独立。

## 短测证据

- Money 工具红阶段是缺少模块；金额显示红阶段是缺少新导出。它们是接口红，不冒称已有行为断言失败。
- 更新价格输入、涨跌停及固定限价测试后，旧 number 实现发生行为失败；实现改为字符串后通过。
- 定向执行 23 个 case 全部通过：四个 utils suites 与 `save/schema/money-wire.test.ts`；其中计划资源覆盖 `9` / `10` 的字典序反例和负冻结资金，运行快照 PositionSnap 成本覆盖旧数字拒绝及大额字符串接受。后一项是对首轮已落地生产解析的补测，不冒称有新的生产行为红阶段。使用 `--test-isolation=none --test-concurrency=4 --test-timeout=10000`，外部 `timeout --signal=TERM --kill-after=1 9` 保证十秒以内终止；这些小 fixture 无需完整回归。
- 独立审查由主任务统一安排；本记录不冒称已完成独立复核。后续补查价格错误上下文，先以 `invalid` 输入观察“金额输入”缺少价格语义的红测，再保留原错误 cause 并加上价格上下文。
