# 实时分钟查询 Core 独立复核

## 范围与依据

复核者未实施本批代码。已完整阅读根 `AGENTS.md`、`docs/principles.md`、ADR-0034、`core-contract.md`、`live-hosts-contract.md` 与新 `session/live_minute_history.rs`，并核对 `session.rs` 的 module/export、`protocol/civil/session.rs` 的新增 wrapper、`retained_history.rs` 的唯一新增 `current_bars` getter，以及实际 `phase`、`observation_civil_instant`、本股日历与本人历史阅读入口。

本批只公开当前内存分钟事实，不核销 Host、UI、SQLite／IndexedDB、全部保留历史或其他原有任务。分钟标签、竞价成交归属及金额规范复用已独立复核的 retained-history accumulator；没有新增交易制度或修改交易所差异。

## 静态结论

- `bars` 只从真实成交 accumulator 读取，不取 indicative price、不从日 K 反推；休市证券返回 `Closed` 和空分钟，正常零成交开市日仍为 `Trading`。
- `AfterClose` 由本股当日真实已完成日 K 与日界 tick 联合识别，不将新日 tick 0 误判为昨日收盘；成功自然日日结的既有 accumulator 转移／清空负责跨日界限。
- `live:true` 表示当前内存事实，并非“仍在交易”。日期与观察时刻使用真实自然日；所有交易所休市才取当日00:00，另所开市时本股仍可保持 `Closed`。
- 纯 `current_minute_history` 不修改状态。本人 wrapper 在成功查询后只登记传入且确实存在的 account；失败证券、失败账户不登记，公共账户缺席场景无需伪造 `AccountId(0)`。
- Request 只有 `code`，拒绝缺字段与账户注入；DTO 复用 `MinuteBar` 的 Money/u64/u128 字符串，不改变 `NotEnded` 分页语义、不增加存档字段、数据库写入、兼容或版本方案。新增独立入口为实时1／5／15／30／60／120分钟视图提供必要事实层，范围合理。

暂未发现需修复的业务代码缺陷。原五项测试覆盖真实成交、只读状态不变、本人读取隔离、休市／零成交、收盘至自然日日结及严格 Request。作者已补两项直接测试并由复核者完整阅读：真实12tick推进验证 `CallAuction→PreOpen→Continuous→ClosingAuction→AfterClose`，无成交时不把竞价指示量当分钟；双交易所合成日历差异 fixture 在真实一步推进后，休市证券为 `Closed`／空分钟但保留与开市证券相同的公共观察时刻。合成日历只测试已明确 Q13 分歧，不声称现行沪深某实际日期开闭不同。

## 验证状态

亲读 `.tmp/retained-history/host46-live-initial-red.log`：五项实际执行，三个真实行为红、两个控制绿。作者随后实现真实 getter 和本人记账；亲读 fresh `host47-live-first-green.log`，原五项全部通过，测试本体0.36秒。root 报告整命令10000ms监督、4测试线程／Rayon4；此证据不覆盖后来新增的两项。

最终亲读 fresh `host48-live-case-list.log` 和 `host48-live-final-green.log`，确认 root 实际 `engine-de08d335cdbb058d` 中七项 case 均存在并全部通过，本体0.79秒；root 报告6测试线程／Rayon4、10000ms进程树监督含监督0.885秒。再次核对新增两项测试源未漂移。综合完整新 scope 静态审查、有效红绿与边界补测，**本批 Live Core 限定 PASS**；不将本结论扩展为 Host／UI 全链路、1／5／15／30／60／120分钟图表、永久历史整体、数据库或完整回归已经完成。未执行完整回归。
