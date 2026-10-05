# 永久历史存档边界验证

本批落实 ADR-0034 的当前契约，不接受旧字段缺失、不迁移或新增 schema 版本。`retained_market_history` 与 `runtime_state.active_minute_history` 均为必填。Web 严格解析规范 Money/u64/u128 字符串、完整自然日期与证券集合、稀疏成交 OHLC、量额笔数、阶段及本股 tick，归档事实与日 K 逐项互证。交易所日历的权威恢复检查仍由 Rust `ProtocolSession.restore` 完成，未在 Web 复制农历或政策算法。

低层活动分钟可以被严格解析，以保留内存 checkpoint 的真实事实；公共浏览器日终入口拒绝任何活动分钟集合，不删除事实伪装日终。Native 的 capture、save、load、copy 原本均经过同一 `ProtocolSession.restore`，复用 Core 的公共日终 guard，不增加重复业务条件。

## 短测试

三个独立 parser 用例以临时空 stub 取得真实断言失败，之后替换为实现并通过。第一次模块不存在的加载错误未登记为业务红。追加日 K 互证、公共日终输入不变及零成交与闰日跨月用例后，六个用例通过；命令使用 `scripts/run-with-deadline.mjs 10000`、Node `--test-concurrency=4`，case timeout 为10000ms，实际约265ms。闰日用例只测试日期覆盖及零成交 wire，不声明该 fixture 日期是现实开市日。未运行复杂回归。

Native 新增 `archive_load_and_copy_reject_active_minutes_without_rewriting_facts`：真实成功周末日终档经 SQL 注入非空活动分钟后，load 与 copy 必须拒绝、不得创建复制槽，原 payload 不被重写。root 统一构建 `host44` 后，从实际 `archives-71aa422541e00218` 产物以 exact selector、`--test-threads=4` 和10000ms外部进程树 deadline 执行，一项通过、零项失败，实际约1.6秒；日志 `.tmp/checklist-wave4/retained-history-native-guard-host44.log`。未用零 case 或静态检查代替成功。

`tsc --noEmit` 在约8.7秒返回失败，错误均位于并行实施的 Host/UI 新接口与尚未生成的 `MarketHistoryRequest`、`MarketHistoryPage`、`MinuteBar` bindings；本批 schema 文件未报告类型错误。这不代表整个 Web 编译通过。真实存档 JSON 由 root 以当前 Engine 正规生成，未手补新字段，也不以旧 fixture 作为当前 schema 的正控。

独立复核记录见 `save-contract-independent-review.md`，须修复其中有效发现后才能宣布本批完成。

`day-end-archive.test.ts` 的局部 synthetic helper 将开局空档的日期改成完成2030-01-01，却没有加入该自然日的归档事实，导致六个原拒绝用例全部停在 schema 前置覆盖检查。该失败属于测试 helper 契约过时，不是日终领域 guard 的业务红；真实日志 `.tmp/checklist-wave4/retained-day-end-fixture-red.log` 保留。只在此 helper 明示2030-01-01、证券600101的 `Closed` 空分钟归档，没有改生产代码或真实 JSON fixture，没有删除或减弱活动委托、资源冻结、待受理和 parent order 的任何原断言。新增空历史及缺证券覆盖拒绝正反控后，与活动分钟入口用例共八项短测通过，实际293ms，concurrency4、命令及新增 case deadline10000ms，日志 `.tmp/checklist-wave4/retained-day-end-fixture-green.log`。
