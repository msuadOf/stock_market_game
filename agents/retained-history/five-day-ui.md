# 五日与日期分页生产消费者

`RetainedHistoryPanel` 通过统一 `EngineHost.queryMarketHistory` 查询，不接触 IndexedDB、SQLite 或存档恢复。Desktop 提供独立五日图表 tab 和长期日期入口；Mobile 的五日 tab 使用同一消费者，长期日期入口独立于当前图表。当前分时仍展示正在进行的一天，归档接口只展示已结束日期。

五日窗口从当前自然日的前一天向前分批查询，跳过 `Closed`、`NotEnded` 与 `BeforeStart`，保留 `NoTrades` 开市日；达到开局边界时明确显示不足五日，不从虚拟前史日K补造分钟。长期入口按自然日正向分页，保留五种 availability；返回上一页使用已读取的页面，不重新查询或恢复市场。五日窗口不删除 Engine 的长期历史。

每个真实分钟提供收盘价图形及完整 OHLCV、成交额、笔数表格。原始分金额、股数、成交额保持十进制字符串，只有图形坐标转换为近似 Number。集合竞价只标示真实成交的阶段，不宣称提供 B05 的独立指示价曲线。

Provider 绑定当前 Host、generation 和本人 account；组件另绑定证券、日期筛选和请求序号，切换或卸载后丢弃迟到成功与错误，并终止五日后续批读取。公开查看者没有账户时仍可读取公开历史，不为其伪造账户。Q02 的主动阅读记账由宿主承担。

验证先以未实现窗口 stub 执行三个真实失败断言，再实现窗口算法。六项短单测通过，外部整命令10000ms监督、Node test concurrency=2、每case10000ms，实际0.62秒；覆盖休市/零成交、开局不足五日、游标、失败显式传播、前页缓存及迟到结果隔离。未执行复杂回归。非作者完整指定范围复核通过，见 `five-day-ui-review.md`。全项目 TypeScript 首次检查因历史 generated 类型缺失失败；root 正规 typegen 后第二次检查实际8.45秒，本模块没有类型错误，但全项目仍因 `DailyCandle.time` 生成 bigint 与既有 number 契约冲突及另一个 owner 的 readonly 差异失败。不把此结果记为全项目编译通过，生成契约修复由 root 统一处理。
