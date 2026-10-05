# 永久量价历史 Core 契约

本批对应 ADR-0034；全部已结束自然日、每个证券的历史永久留存，五日仅是 UI 查询窗口。Engine 不依赖 SQL、IndexedDB、网络或文件。

## 数据形状

- `SaveSlot.retained_market_history` 为必填 `RetainedHistoryDay[]`，按自然日期严格递增且覆盖开局到最后成功日结。每项 `securities` 精确覆盖本局证券。
- `MinuteHistorySession.status` 区分 `Trading` 与 `Closed`。`Trading` 可以拥有空 `bars`，表示正常零成交；`Closed` 必须没有 bars。
- `MinuteBar` 是真实成交产生的稀疏分钟 OHLCV，而不是无成交分钟填充：`minute_of_day` 是上海交易时间当天分钟标签；`phase` 显式为 `OpenAuction`、`Continuous`、`ClosingAuction`。开盘成交标为 09:25，收盘成交标为 15:00；连续竞价按实际模拟 tick 的自然日观察时刻归属，不把开盘虚拟撮合量计入成交。
- `open/high/low/close` 使用 `Money` 十进制分字符串；`volume_shares`、`trade_count` 使用规范 u64 十进制字符串；`turnover_cents` 使用规范 u128 十进制分字符串。公共分钟资料不包含账户、成交对手、委托或 receipt identity。
- 当前日分钟 accumulator 只在内存；只在完整经营、披露及交易收尾的成功日结后成为不可变历史。不持久化日内活动委托。`SavedRuntimeState.active_minute_history` 为必填 `Record<StockCode, MinuteBar[]>`，使低层 `GameSession` 的内存验证 checkpoint 可以完整恢复；公共 `ProtocolSession.restore`、SQLite 和 IndexedDB 日终写入入口必须拒绝任何非空活动分钟事实。正常日结真正转移并清空 accumulator，不通过投影删除来伪装日终。

## 分页

`MarketHistoryRequest { code, date_from, date_to, after, page_size }` 全字段必填；日期是 `CivilDate` 既有规范 `YYYY-MM-DD` 字符串，日期两端包含，`after` 为日期或显式 `null`，是同一范围中上一页最后返回日期的排他游标，`page_size` 为正 u32 整数。每页按自然日期稳定递增，保留休市日期状态，不通过无解释截断丢事实。

`MarketHistoryPage { code, entries, next_cursor, settled_through }`；entry 为 `{ date, availability, bars, daily_candle }`。`availability` 明确区分 `Traded`、`NoTrades`、`Closed`、`BeforeStart`、`NotEnded`。开局前可以有明确生成的日 K，但不会生成不存在的实时分钟事实；未结束日不会以部分成交冒充完整已归档日。

公共内部缓存生成是纯读取；`query_market_history_for(account, request)` 为实际账户主动阅读入口，在请求成功后记该账户自己的历史阅读，不替其他账户制造经历。`ProtocolSession` 提供相同 wrapper；没有经济账户的已授权公共阅览由 `market_history_page(request)` 纯只读入口处理，不创建 `AccountId(0)` 或他人的阅读经历。分页 generation 隔离由宿主契约负责，不借历史读取恢复当前市场。

## 验证计划

短 fixture 验证：真实逐笔 OHLCV 与精确成交额、竞价时段分离、错误输入原子拒绝、旧历史 Arc 共享、跨日永久留存及稳定分页、休市与零成交区分、成功日终 restore 深等值、遗漏/重复/乱序/错误证券/不合法金额的严格拒绝、失败交易和失败日结不追加历史。Cargo、bindings 和真实 fixture 由 root 统一生成；本记录不是已经通过的验证宣告。

首轮 `host39` 从 root 实际产物 `engine-fd39ddbb13be65af` 用 `--list` 确认全部五项测试存在；四项真实 `record` 行为断言失败，一项严格 wire 正负控通过。整批使用 10000ms 进程树监督、每 child 9000ms、并行4及 Rayon4，实际0.28秒，日志位于 `.tmp/retained-history/red-*.log`。

接生产后的 `host40`（root 实际 `engine-48220fcc6e1a5073`）十项短测中五项通过，五项失败，不能称为全部通过。错误输入原子性测试暴露内部 `(minute, phase)` map 不能直接序列化为 JSON；这个问题同样影响真实 `business_state_hash`，现已改为通过规范 `MinuteBar[]` 序列化。四个真实执行场景因测试开局 `float_shares=0` 没有实际卖方股份而停在 setup 断言，已在真实初始化前设为1000股分给 NPC，未补造持仓。恢复阶段配置的新增拒绝测试还没有取得有效业务红；没有把 setup 失败登记为领域 guard 失败。日志 `.tmp/retained-history/host40-*.log` 保留。

相邻 `ProtocolSession` 的低层 verification checkpoint 不创建公共档、休市成功日结候选可以恢复两项 `host40` 短测通过，实际0.87秒；这些证据不替代真实有成交日的最终验收。

修正序列化和真实开局 fixture 后，`host41` 实际十项测试中基础九项通过；独立静态复核发现的竞价配置缺口，在真实有成交日终档上获得“关闭的竞价阶段仍被接受”业务红。统一 `validate_stock_bars` 现在同时用于归档、完整已结束历史恢复和活动内存 checkpoint 恢复，交叉本股 `stock.tick`、开盘及收盘竞价是否启用，以及启用收盘竞价时连续竞价不得落在14:57之后。未修改其他作者的金额 helper。

`host42` root 实际新构建 `engine-de08d335cdbb058d` 通过 `--list` 确认十一项 case 全部存在；整批 10000ms 进程树监督、每 child 9000ms、并行8及 Rayon4，实际1.38秒，十一项全绿。新增证券 tick 正负控也通过；真实执行 checkpoint、日终深等值恢复、八自然日分页及状态区分、失败交易和未完成日结不追加事实、Arc 共享与篡改拒绝均为短 fixture。日志 `.tmp/retained-history/host42-*.log`。

最后的文档复核发现 Serde 的普通 `Option` 会将缺失 `after` 视为 `None`，与必选 nullable 契约不一致。`host44` 新 `nullable_history_fields_require_an_explicit_null_or_value` 在缺失 `after` 上取得真实业务红，十秒监督下实际0.157秒；同批 Server 缺字段请求也是真实 HTTP 200≠400 红，而其他权限与 generation 控制已通过。已按项目既有 `required_nullable` 模式给本批四个 nullable 字段配置明确反序列化入口，不使用 default 或兼容补齐。`host45` root 新构建 `engine-48220fcc6e1a5073` 用 `--list` 确认十二项实际存在；本模块十二项短测以8测试线程、Rayon4、整命令10000ms进程树监督全部通过，测试本体1.18秒、含监督1.27秒。日志 `.tmp/retained-history/host45-core-final-green.log`；四种缺字段拒绝及显式 null 控制均已真正执行。

本批范围仅为 Engine Core、严格状态与 `ProtocolSession` wrapper。Server／Tauri／WASM、Web 严格存档 parser 和 UI 查询消费者由独立任务配套；SQLite／IndexedDB 历史索引与五日视图不能凭本批短测核销。没有运行复杂回归、多平台矩阵或长仿真；完整冷热组织和大规模历史查询性能仍须另有证据。

## 当前内存分钟查询

`CurrentMinuteHistoryRequest { code }` 独立于已结束日分页，拒绝额外 `account` 字段。`CurrentMinuteHistoryResponse { code, date, observed_at, live, status, phase, bars }` 全字段必填，`date` 为规范公历字符串，`observed_at` 使用既有 `CivilInstant` 的公历日期与当天秒数，秒数范围0至86399。`live=true` 只表示本次查询来自当前内存会话，不表示市场仍在交易。

`status` 按本人查询证券的交易所日历判断，`phase` 显式区分 `CallAuction`、`PreOpen`、`Continuous`、`ClosingAuction`、`AfterClose` 和 `Closed`。某股休市但另一所开市时，该股仍为 `Closed`、空 bars，观察时刻可以是共享市场的实际时刻；所有交易所休市的自然日稳定起点为00:00。无真实成交时返回空 bars，不用竞价指示量或昨收补假分钟。

`current_minute_history` 仅复制请求证券的真实活动分钟事实，不复制全部证券的历史。`query_current_minute_history_for` 在查询成功后只给实际读者登记 Q02 访问；公开缓存或没有经济账户的公共阅览不创建 `AccountId(0)` 的经历。当前交易日收盘但尚未完成自然日日结时保留当天真实 bars；日结后进入下一日期，当前 API 不从昨天归档补成今天的事实。已结束日接口的 `NotEnded` 行为保持不变；本 API 不写存档，不新增日内持久化。

`host46` 部分 Engine 新产物在全宿主批因 Desktop 测试签名未适配而失败后，按 root 授权执行五项 Live 短测，取得实时真实成交、收盘后真实分钟和本人阅读未记账三项业务红、两项严格请求及休市／零成交控制绿。实现读取唯一活动 book 并记本人访问后，`host47` 五项全绿。独立复核要求补齐阶段切换与双交易所休市边界，已用真实12 tick和既有混合日历短 fixture 补入。

`host48` root 新产物 `engine-de08d335cdbb058d` 的 `--list` 确认七项 case 全部存在，六测试线程、Rayon4、整命令10000ms外部监督，本体0.79秒、含监督0.885秒，七项全绿。证据 `.tmp/retained-history/host48-live-case-list.log` 与 `.tmp/retained-history/host48-live-final-green.log`。这只验证 Live Core，不能代替 UI 分钟聚合、三宿主消费或完整 checklist。
