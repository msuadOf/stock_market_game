# 永久分钟历史 Core 独立复核

## 范围与证据

复核者未参与实现。已完整阅读根 AGENTS、principles、architecture、open-questions、ADR-0034、core-contract、新 retained_history.rs，以及 session.rs、hash.rs、saved_runtime.rs、candidate_commit.rs、protocol/civil/session.rs 的当前完整 diff；另外核对 candles.rs、observation_clock.rs、AppendOnlyHistory 和既有存档日期校验。共享文件中多人账户、月报、更正事务等其他作者的改动不在本次完成声明范围内。

本次已直接阅读 `.tmp/retained-history/host41-*.log`：9 个基础 case 通过，竞价配置恢复 case 在业务断言处真实失败，未把编译或 fixture 阻挡当真红。随后亲读 `.tmp/retained-history/host42-*.log`：11 个 case 各实际执行 1 项、全部通过，最长 1.17 秒。root 统一 fresh 编译，作者确认 binary 为 `engine-de08d335cdbb058d` 且 list 包含全部 11 项；批次整命令 10000ms、child 9000ms、xargs 8、Rayon 4，实际 1.38 秒。最后亲读 `host44-nullable-red.log` 和 `host45-core-final-green.log`：缺失 after 的真实断言先红，新增 required_nullable 后全部 12 项实际执行通过，测试本体 1.18 秒。host45 的 fresh binary 由 root 构建、作者 list 确认，8 测试线程、Rayon 4、整命令 10000ms 监督。本审查未另启 Cargo 或完整回归。

## 领域语义与必要性

实现使用真实 Event::Trade 聚合稀疏分钟量价，不把 AuctionTick 指示量、无成交分钟或休市日期伪造为成交。开盘 09:25、收盘 15:00 单独标记，与 Continuous 自然分钟分离；该时段语义与项目现行 A 股规则一致，不引入交易制度变化。证券开市状态取各自交易所日历。公开资料不带 maker、taker 或 order identity。u128 分成交额、u64 股数和笔数使用规范字符串，符合单位契约。

永久历史事实、严格存档字段与分页契约是 ADR-0034 必要范围。已归档日期通过 Arc 与 AppendOnlyHistory 共享，tick shadow 不复制旧日 bars；活动分钟只在内存 checkpoint 中恢复，公开 Protocol 日终恢复显式拒绝非空 active_minute_history。没有新增 schema 版本、迁移或旧字段默认补齐。

## 有效发现

1. **已修复并复核：恢复未交叉证券价格粒度及竞价配置。** 原 validate_bar 仅验证通用时段和 OHLCV；可以构造保持日 K 总量与 OHLC 一致、但违反 stock.tick 的分钟价格，也可以在 auction_ticks=0 时塞入 OpenAuction、closing_auction_ticks=0 时塞入 ClosingAuction，或启用收盘竞价时塞入 14:57–14:59 Continuous。host41 真实业务红证实竞价缺口。新 validate_stock_bars 已共用于活动恢复、永久恢复及日终转移，四个 OHLC 均按本股 tick 校验、禁未启用竞价、配置收盘竞价时 Continuous 严格止于 14:57 前；新增真实档篡改和纯函数正负控保持断言，静态重新审查及 host42 两项短测通过。
2. **真实发现已静态修复：内部 tuple map 不能 JSON/hash 序列化。** 作者运行真实成交路径后发现 active 的 `(minute, phase)` key 不可 JSON 编码。新自定义 Serialize 保留全部 days，并通过规范 saved_active 的 Vec 顺序序列化全部活动分钟，不丢事实、不建立兼容字段；与 SaveSlot 的实际形状一致，host41 基础真实执行、hash、恢复路径绿测已覆盖这项修复。
3. **文档契约漂移已修复并复核：nullable 字段必须明确提供。** 普通 Serde Option 曾接受缺失 after 为 None，与文档的全字段必填承诺冲突。host44 取得缺 after 真红；当前 after、daily_candle、next_cursor、settled_through 均通过 deserialize_with=required_nullable 接受明确值或 null、拒绝缺失，未加入 default 或兼容逻辑。完整读增量 helper 和测试，host45 四字段缺失拒绝及明确 null 控制通过，工作契约原章节已准确融合证据。

## 原子性与边界

record 先复制受影响证券的活动分钟，全部事件成功才 extend；candidate 提交前调用，失败不发布权威分钟。日终 finish 在经营、披露和日结后调用，任何失败由完整 Committable checkpoint 恢复；历史包含在 clone/commit/hash 中。分页自然日期递增，after 排他且范围显式校验，BeforeStart、NotEnded、Closed、NoTrades 分开。query_market_history_for 只为成功请求的本人记阅读，market_history_page 本身不制造阅读经历。

现有新测试覆盖逐笔量价、无私有身份、u128 超 u64、错误成交原子、竞价指示量、严格 wire、真实 checkpoint/EOD restore、失败 tick/day-end、八自然日分页、Arc 共享及缺失/重复/篡改档。新证券粒度与竞价配置测试已在 host42 fresh batch 通过；失败 tick 现先安排第二笔真实买卖再注入提交前失败，开局股份由 float_shares=1000 的真实分配产生，不调用 grant。跨宿主 SQLite/IndexedDB/Remote、生成绑定和真实 WASM 不由这些 Core 测试证明。

长期查询目前每个日期按 AppendOnlyHistory.get 寻址并倒序寻找日 K，单页成本随历史长度增长；这不丢数据且 tick 不拷全史，但后续性能证据应明确查询成本，不应把稳定分页描述为已完成长期索引优化。

## 当前结论

**本批 Core 范围 PASS。** 完整 diff、统一 guard、required nullable 增量和测试已由非作者核对，最终 12 个 fresh binary 短测结果已亲读，发现均已修复并再次复核，工作契约描述与本批代码一致。该结论只覆盖永久分钟事实、Core 查询及其 GameSession/Protocol 接线，不扩大为跨宿主持久化、绑定、WASM、UI 或完整 checklist 已完成。
