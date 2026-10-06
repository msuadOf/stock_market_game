# MarketSnap wire 消费端记录

## 范围

本次只更新 Web 运行协议 `parseMarket` 与存档 `parseSnapshot`，使其严格接收新增的
`cash_ex_reference_pending_trade`、`day_market_activity`、`last_cash_ex_reference`。
字段缺失时拒绝输入；可空除息锚点按 `ExReferencePrice` 校验日期和正 Money。

## 语义依据

三个字段来自 engine `MarketSnap`，分别表达除息后待首笔交易状态、当日市场活动事实、
最近除息参考价锚点。解析器只原样传输并验证类型/表示，不派生或修改交易行为；本改动
不改变 A 股交易规则。

## 验证

- 按 TDD 先增加短测，旧 parser 对新增字段的 exact shape 拒绝输入，测试按预期失败。
- `node scripts/run-with-deadline.mjs 10000 -- node --test --test-timeout=10000 --test-isolation=none apps/web/src/save/schema/money-wire.test.ts`：通过，7 cases。
- 既有 `.tmp/company-system/session-actions/web-tsc-bounded-build.log` 曾报告两个 parser 类型错误及 fixture 缺字段；parser 返回 shape 已补齐。尚未执行新的 TypeScript bounded build。

## 独立复核

初审发现存档锚点解析的 `civilDate` 年份域比 Rust `CivilDate` 与运行协议的
1900..=2199 范围宽。仅在新增的锚点 parser 中加入年份边界验证，并在短测覆盖
1800/2200 拒绝、1900/2199 接受；共享日期解析范围未更改。独立 reviewer 已完成最终
增量复核并通过；记录见 `agents/company-system/market-anchor-wire-consumers-review.md`。
