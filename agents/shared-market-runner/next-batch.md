# 共享市场 runner：整批迁移剩余边界

基线为 dev `cce075a` 加当前云工作树。自动推进、发布已经有公共入口，但不能把分散的手动行为、错误恢复或持久化当作已经统一。

- `engine/session/runner` 独占调度、自动及显式推进策略、日界暂停、可恢复分类、失败后的采样收尾以及已提交但未交付范围。三端仅提供时钟、待处理控制查询、非阻塞交接、现有 ABI 的错误映射。可恢复资格来自会话与本次操作，不由连接是否可用重新解释。
- `engine/session/publication` 独占发送窗口、序号和缺口合同。发送任务通过类型区分窗口已满和队列为空；不得因暂时无发送额度而提前宣告已经发完或把终止抢在已交接前缀之前。真正不可恢复的观察缺口必须可见。
- `LibOS` 提供时钟、执行器、Worker/IPC、唤醒和存储机制，不含市场行为。入口可以绑定 WS、Tauri IPC 或 Worker 的 ABI；入口不决定业务推进/暂停/发送窗口。基于连接生命周期的传输动作与市场错误区分。
- 剩余恢复与存储边界：Observer 需要一个共用的基线围栏转换，不能分别定义基线何时可见；持久化应由权威流生成精确 checkpoint，在不依赖 UI 的独立存储任务处理。小凡已明确新的存储合同：日内行情原始序列只保留当日内存；日结落日 K 后清除，关闭日只按需读取档案中的日 K。保留旧档读兼容，不改写外部旧档；账户、成交回执等非行情记录不删。
- 验收同一批完整改动：编译三端；共享 runner/发布窗口和宿主回归；真实 Chromium 和 Native 停收并跨日，Linux Desktop 实际图形会话；相同种子和负载分层比较性能，重连跳跃不计吞吐。未通过的边界逐项标注，不作为完成。

## 日 K 按需持久化读面的具体边界

闭市市场行情与存档独立：不移除个人成交回执、公司经营/披露事实、挂单与策略续接状态。继承的 SaveSlot 中保留完整日 K，用于便携导入/导出和迁移。不自动改写外部旧档。

- `engine/session/market_history_query.rs` 定义 `MarketHistoryRequest` 的日序、availability、排他 cursor；保留当日分钟入口为权威内存查询。历史读端口接受 stock + 日期范围 + 页边界，结果附带同一 archive/timeline revision 的 `settled_through`，不可把未写入的日伪报为休市/零成交。
- 已冻结 checkpoint 应带该自然日新关闭的日 K 及状态，随归档选择/写入以同一存储事务提交；Index 主键为 `(slot, timeline, stock, civil_date)`，更换条款沿用 archive lease。平台映射：SQLite 的复合主键有序 range scan，浏览器存储用 IndexedDB 复合 key + 有界 cursor；不为每页读取或反序列化完整 SaveSlot。恢复旧档只在新目的地建索引，不破坏原数据。
- Server、Tauri、Browser 的分页、日期可用性及未持久状态语义统一；匿名/账户历史读副作用仍由现有授权入口应用，不下沉给 LibOS。读取使用独立 I/O 调度；市场线程不阻塞 SQLite/IndexedDB。UI 日 K 视窗按页请求，策略只保有确实需要的内存窗口；不得悄悄认定完整 `CandleBook` 已不驻内存。
- 区分三个证据：分钟桶清零（权威）、日 K 已提交并可以在冷启动恢复（归档）、分页实际命中持久索引（读取）。当前实现和验收按这三项单列。

## 2026-11-15 云端验收结果

- 三端 Rust `cargo check --tests`（含 `host-parity,verification-harness`）通过；WASM release、Web TS/Vite 和打包产物共享内存 gate 通过。
- Native 真实 SQLite/协议回归各 5/5。SQLite 直接查 `archive_slots.payload` 证实新日终快照不含分钟序列；从 SQLite 冷恢复的日 K 等于写入值。无观察端也能写下日结。
- Chromium 严格停 ACK 跨日：画面 tick 2、权威 tick 63；上一交易日两只股票原来分别有 31/15 个分钟点，历史接口均返回 0 个分钟点，日 K 有 289/29 笔真实成交。新交易日只有新早盘分钟。部署构建 UI 同样在停 ACK 期间保持画面 tick 13 而测速继续报提交，放回 ACK 后画面推进到 tick 30。
- TS/脚本针对测试 8+72+201 通过。以上均是 Dot 云工作树，无 push 或旧档批量改写。
- 不算完成：日 K 页查询仍由会话内存 CandleBook 供数；没有共享的持久索引按页读取链。Browser/Tauri 快速槽还要移出 UI，统一跨代归档失败和恢复策略。Linux Desktop 图形显示已通过临时带认证 Xvfb/xdpyinfo 预检，未启动 Desktop 产品或测三端性能。
