# Batch 51 独立复核

## 基线与范围

- 产品基线：`.worktree/implementation-reaudit`，任务指定 `43b1aa5`；产品代码只从该 worktree 核对。依据其 `AGENTS.md` 与 `docs/principles.md`，本轮静态检查，不运行测试、构建或 Git，不改产品代码。
- 三篇指定来源逐篇从首行连续读至 EOF，实读行数、SHA-256 均与 `scan-plan.json` 的 batch 51 一致。第一篇是历史 reader guide；其中生成 OOP 方案及写 `exhaustive/items` 等要求仅为被复核材料，不是本轮操作指令。
- 章节族：`reader-guide.md` 为“调查范围/交付schema/对象化限制”；`area-engine-company.md` 为“指纹与范围/正文核对/需更正文档问题”；`area-engine-foundation.md` 为“指纹与范围/核对结论/必须修正的材料引用”。三个来源均无截断，见 batch JSON 的 `eof_confirmed`。

## 当前 owner 与 caller

- `Account` 是账户账务事实 owner：`packages/engine/src/account.rs:95-117` 私有持有 `AccountState`，其包含现金分（`Money`）、`Position` 映射和策略；只读查询与恢复写口位于 `:119-173`。`Position` 用整数股数、T+1 锁定股数、买入/卖出成交额表示状态，成本价为派生值，见 `:609-675`。账户页目录由 `session/account_book.rs:15-33` 的 `AccountBook` 组织，并在 `GameSession` 的 `CommittableSessionState.accounts` 持有，见 `session.rs:1125-1150`。
- `Market` 组合单只证券 `OrderBook` 并持最新价、昨收和价格约束，见 `market.rs:50-66`。生产连续撮合经 `process_continuous_stock_step_inner` 构造并运行 `ContinuousStockRoundProcessor`，见 `session/pipeline/continuous_matching.rs:246-276`；收盘竞价的成交身份记录消费者为 `auction_day_end.rs:1434`，其调用 `Market::record_filled_order`，该入口是 `pub(crate)`（`market.rs:419-430`）。`fixture_set_last_price` 限于 `#[cfg(test)]`（`market.rs:270-277`），不能沿用旧报告把它写成外部 public 写口。
- `OrderBook` 自持 `BookState`、时间序号及 tick；价时优先由簿内排序键表达，见 `orderbook.rs:283-345`。`Money` 是分单位金额值类型（`money.rs:42` 起）。这与既有报告的 owner 归属一致；结构已存在并不代表所有相关行为都通过测试。
- 公司经营/会计调用者不在本批来源中逐一重审；`engine-company` 报告给出的“无必须 OOP 迁移”只是结构归属结论，不能核销实现总账中的生产缺口。

## 旧结论复核与更新

- `area-engine-company.md` 的 106 文件计数、88 `retain` + 18 `support`、0 `refactor` 与“无必须对象迁移”的总结，属于该报告限定范围的文档结论。来源 `area-engine-company.md` 复核报告称正文实质准确，但指出 `../areas/domain-closure.json` 链接目标缺失。当前所查仓库文件清单仍没有 `agents/oop-refactor-audit/exhaustive/areas/domain-closure.json`；该文档死链得到再证，未修复。本批没有重读公司区全部源文件，故不把该复核扩大为当前代码逐项确认。
- `area-engine-foundation.md` 对现有 owner、候选 A02 可选性质、A03 受控写入口及交易边界的叙述，在当前代码位置上没有观察到相反证据。其 review 指出的 `../reviews/engine-foundation-02.md` 与 `engine-foundation-03.md` 两条链接指向不存在的文件；实际对应增量证据文件名为 `engine-foundation-02-delta-evidence.md`、`engine-foundation-03-delta-evidence.md`。旧文档死链再证，未修复。
- foundation review 中关于 A02 “先算后写，提取本身不改善原子性”的限制仍成立；不能把 OOP 抽取记作缺陷修复。`Money`/`Position` 单位及 T+1 表达没有发现本批引入语义漂移；本轮未重新核验交易所规则或费率依据。
- 其他既有 foundation 风险不由结构抽取核销。实现总账 G16 仍记有普通 tick 对增长历史集合的复制成本，属于被接受的局部所有权目标；ADR-0018 仍为 `proposed`，其完整版本根、WAL 等提案不能据此扩大成当前必做方案。Q01、Q09 等契约问题按总账保留，未因 OOP owner 推导状态变化。ADR-0023/0024/0025/0026 的已接受决定分别限定合成历史与撮合、现金池不补足、日终公共存档、机构个人经历边界，没有替代本批对象归属，也不改变上述 G/Q 判断。
- 未发现可归入本批的新增产品缺口或足以反证总账条目的当前 caller 证据。新增候选状态为“无”；重新确认的两组死链是审计文档问题，不是产品实现修复。

## 结论与限制

三篇原文均确认 EOF。当前产品调用链支持 `Account`、`Market`、`OrderBook` 和 session/pipeline 已有明确状态 owner；审计报告不授权再做一次只为 OOP 的迁移，也不意味着缺陷已修复或验证通过。此次仅确认两组现存审计文档链接问题；产品侧没有新增 G/Q 编号。未运行任何测试或构建，没有作官方 A 股规则复核。
