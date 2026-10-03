# Batch 011：domain OOP 实施状态独立复核

基线：`43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。本批仅核对指定 action-index/domain report/domain review-a 三份来源所覆盖动作，在基线产品实现中的 owner 与 caller，以及 G01–G68/Q 和现行 ADR 的交叉边界。未改产品文件、未运行测试或构建、未写 Git。

## 阅读完整性

三份源文件均已连续全文读取至 EOF，实测行数与 SHA-256 如下：

| 源文件 | 行数 | SHA-256 | EOF |
|---|---:|---|---|
| `agents/oop-refactor-audit/completeness-2026-10-03/action-index.md` | 2067 | `70b1050b78346fb48b63b29e663b15a98a657dbd950d3bc9da8fb90811bf059b` | 是，读取 `1–2067` |
| `agents/oop-refactor-audit/completeness-2026-10-03/domain/report.md` | 265 | `f0b4c38abc5ab7e98be2e11053c6f8100b5a1876a4705b2b5fe9f5a9e49d13fc` | 是，读取 `1–265` |
| `agents/oop-refactor-audit/completeness-2026-10-03/domain/review-a.md` | 23 | `bcb189719b42430ea7974585412712f4bd4f2dfd771c2726b3c95a31ee2af283` | 是，读取 `1–23` |

## 动作与实现核对

来源中“尚未实施”是历史调查快照，不是基线当前状态。基线 `43b1aa5` 已包含 `8cf34a1 refactor(engine): 完成全仓 OOP 状态与行为聚合`；实施台账 `agents/oop-refactor-implementation/domain/accounting-result.md`、`orderbook-result.md`、`experience-result.md` 及 `summary.md` 与产品源码相互印证。所审 `engine-foundation-01-A03`、`domain-N01` 至 `domain-N07` 对应 owner/caller 已存在：

| 动作/领域 | 当前实现证据 | 复核结果 |
|---|---|---|
| A03 账户与持仓封装 | `packages/engine/src` 账户字段私有、只读 getter、受控恢复、测试专用 fixture setters；`Position` 私有字段与恢复构造保留既有 serde 接受集 | 已实施；未见把行为校验或交易语义作为重构副作用 |
| KDJ 累计状态 | `packages/engine/src/indicators.rs:179-285` 的 `KdjAccumulator` 及双 caller | owner 与生产接线已存在 |
| 订单进度 | `packages/engine/src/orderbook.rs:179-215,346-368` | owner 被订单簿流程使用 |
| Market 写入口/竞价 caller | `packages/engine/src/market.rs:256-277,411-426`；`packages/engine/src/session/pipeline/auction_day_end.rs:1422-1435` | crate 内有限写入口与生产 caller 已存在 |
| 三行业 Config 守卫 | `packages/engine/src/company/{bank,insurance,real_estate}/config.rs` 及各 `mod.rs` Books 构造 caller | 对应配置守卫已接线，行业差异仍分开建模 |
| worksheet 累计 | `packages/engine/src/accounting/reports/consolidated_window.rs:142-149` | `Accumulator::add_current_worksheet` 由报表流程调用 |
| VAT/CIT policy | `packages/engine/src/accounting/tax.rs:74-158` | policy 方法与兼容 facade 均存在 |
| 机构观察/持仓清理 | `packages/engine/src/session/institutional_behavior.rs:16-29`；`packages/engine/src/session.rs:1547-1583` | 经 `RetailExperienceState` 方法接入；不是散户 dated experience 链 |

必要性与最小范围：以上源码证实这些抽取并非仅有孤立类型，而在相应生产路径中使用。A03 保持字段封装和受控恢复，但没有额外引入订单合法性校验、交易制度改变或新的权威存档事实。三行业配置仍显式区分，没有以共用默认规则代替行业差异。

## G/Q 与语义边界

这些 OOP 动作不等于现行功能缺口已关闭。逐项交叉时仍保留以下未核销项：

| 现行项目 | 不由本批动作核销的理由 |
|---|---|
| G17 | Rayon batch 未生产接线；KDJ 状态 owner 不代表批处理已部署。 |
| G08 | G08 专指 retail dated experience 链；机构观察状态不能替代该链。 |
| G35 / Q23 | 经营税务调用时机与 policy 方法化属于不同问题。 |
| Q17 | ClosingEngine 整体失败边界并未由局部 owner 抽取解决。 |

未发现本批动作引入新的 G/Q。与 ADR-0026 对照，机构经验是游戏策略假设，不是交易所规则。源码核对未发现改变 A 股金额分、股数、T+1 或价格时间优先的证据；这只是本批局部语义检查，不是现行官方规则重新取证或全局合规认证。测试文件/fixture 仅作静态检查，未运行测试。

## 初审问题闭合

`review-a.md` 曾提出 `review.md` 链接及 hash 转抄问题。后续 canonical `agents/oop-refactor-audit/completeness-2026-10-03/domain/review.md` 记录两处复读已闭合、三门通过；其中核对的 report hash 与本批指定源文件一致。故该历史初审意见不是当前阻断，不重复登记为新发现。

## 结论

所审 A03 与 domain-N01–N07 在基线中均已实施并由对应路径使用；来源中的“未实施”需按历史快照理解。未发现需要新增 G/Q，也未发现所审 owner 聚合模糊或改变 A 股交易语义。未运行测试，亦未声称测试通过；结论限于本批指定来源和上述源码/总账交叉项，不替代完整 OOP 或 G01–G68 验收。
