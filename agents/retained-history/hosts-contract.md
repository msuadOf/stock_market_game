# 永久量价历史宿主接线

本批将 ADR-0034 的 `MarketHistoryRequest/Page` 接到三种 Rust Host、WASM Worker 和统一 `EngineHost.queryMarketHistory`，不修改 UI 或存档 schema，也不手写 generated。公开分钟资料仅包含真实分钟 OHLCV 与成交额，不返回账户、订单或成交对手；字段使用 ISO 自然日期、Money 十进制分、规范 u64 股数和笔数、u128 成交额。

Server 的 `/api/market-history` 要求认证和当前 generation，账户从认证主体的市场 membership 解析，不能由客户端指定。真实成员经 `ProtocolSession.query_market_history_for` 记录本人读取；未入场的公开访问者、经济账户不在当前档中的 controller 走纯 `market_history_page`，不伪造 AccountId(0) 或授予交易能力。Desktop 和浏览器本地的实际本人均为 AccountId(0)。查询不推进 tick、不读盘恢复市场。

Worker 请求/响应绑定 generation 和 requestId，安装前额外检查 baseline epoch；Remote 和 Tauri 使用宿主 query cursor 拒绝换档迟到结果，Remote 同时拒绝本人 membership 身份变化。Web parser 拒绝缺字段、额外字段、账户注入、非规范或超范围金额/数量、无效自然日期、错误证券、乱序分页、伪成交与不相容的成交阶段。09:25 和 15:00 分别是实际开盘与收盘竞价成交，不将指示价或虚拟撮合量当成交。

## 短测试证据与边界

parser 首轮三个 case 中一项真实正向业务红、两项拒绝负控绿；最初正向 fixture 的 u128 金额与 full-u64 股数不自洽，修为实际 `100 × 股数` 的金额，不放宽生产 guard。最终六项定向短 case 通过：严格 parser 三项、Worker generation 一项、Tauri 显式命令一项、Remote 未入场公开读取及过期 generation 一项；使用 10000ms 外部进程树 deadline、case timeout 10000ms、test-concurrency=4，整批实际0.42秒。没有运行复杂回归。

Server 新增 `retained_market_history_auth_generation_and_public_nonmember_query`，验证真实路由认证、未入场公开访问、当前 generation、账户注入和 `after` 缺失。root 的 host44 实际编译成功后，使用其真实 Server lib 产物执行该单个 case，10000ms 外部 deadline、test-threads=4，实际0.26秒：认证、本人／未入场公开访问、过期 generation 和账户注入断言通过，缺 `after` 的最终断言真实红（HTTP200，预期400），日志 `.tmp/retained-history/hosts-server-short.log`。Core owner 取得同源 serde 红后增加必填 nullable guard，没有删改或弱化 HTTP 断言；root 的 fresh host45 编译成功后，以相同外部 deadline 和并行参数重跑同一 exact case，实际1项通过、37项过滤、零失败，测试报告0.19秒，日志 `.tmp/retained-history/hosts-server-host45-short.log`。没有运行复杂回归。

正规 generated 首次重新生成遗漏 `TS_RS_LARGE_INT=number` 环境，导致 DailyCandle.time 变成 bigint；实际 tsc 短命令8.11秒失败。root 恢复完整生成环境后，实际 `tsc --noEmit -p apps/web/tsconfig.app.json` 在10000ms外部 deadline 内6.99秒通过；没有手改 generated 或类型 cast 隐藏问题。补充的空页／遗漏自然日期／提前丢失 next_cursor 负例先取得真实红，再增加完整自然日覆盖 guard，六项定向短测仍全部通过。

完整独立复核记录见 `hosts-independent-review.md`，非作者复核未发现阻塞问题。root 的首次宿主编译发现新 Server 分支错误复用接受 MembershipError 的 typed reject closure，而历史查询返回 SessionError；已改为显式 SendCommandError 文本映射，非作者再次确认两类错误的类型边界和错误详情均正确。保存到 `.tmp/retained-history/hosts-web-short.log` 的最终六项短测实际0.47秒全绿，`.tmp/retained-history/hosts-web-typecheck.log` 的最终类型检查实际8.25秒通过。host45 Rust 编译及新增 route exact case 均通过；最终签核由非作者亲读红绿日志确认，不以 Web 通过冒充 Rust 通过，也不将这些短测描述为完整平台验收。
