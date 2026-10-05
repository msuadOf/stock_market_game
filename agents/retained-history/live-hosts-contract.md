# 当前真实分钟历史宿主契约

本批为实时分钟 K 提供独立 `EngineHost.queryCurrentMinuteHistory`，不改变 ADR-0034 的已结束日期分页，也不把 `NotEnded` 返回值改为盘中档案。请求只有 `code`；响应为 `code/date/observed_at/live/status/phase/bars`，所有字段必填。`live:true` 标记当前内存事实，不表示市场仍在交易；`AfterClose` 保留当前日真实成交直到成功自然日日结，次日必须读取新的空或已发生分钟，不复用旧日存档。休市证券没有分钟成交。

Server 新增认证 POST `/api/current-minute-history`；真实 membership 主体经 `ProtocolSession.query_current_minute_history_for` 记录本人 Q02 读取，未入场公开访问者或当前档缺经济账户的 controller 使用纯 `current_minute_history`，不伪造 AccountId(0)。Desktop 和浏览器本地仅访问实际本人 AccountId(0)。三 Rust Host、WASM Worker、Remote/Tauri 适配器与 Provider action 已接实际方法，不提供 optional 空实现或偷偷回退至已归档页。

严格 parser 共用已复核的 `MinuteBar` 规范校验，保留 Money/u64/u128 十进制字符串，不将大数转换为 Number。`observed_at` 使用既有 CivilInstant：ISO 自然日期与响应日期相同、当日秒0–86399；分钟成交不得晚于已提交观察时刻，不接受重复或乱序分钟。`Closed` 必须同时是 `Closed` phase、空 bars。宿主和 Provider 共同拒绝旧 generation、baseline epoch、本人身份或替换 Host 的迟到响应。历史查询不推进 tick、不恢复存档、不写日内数据库。

## 短测试与复核边界

独立 parser 的首次两项短测一项正向业务红、一项拒绝负控绿；实现后 parser 两项与三种 Web Host 请求／过期 generation／未入场公开读取共五项通过，10000ms 进程树 deadline、默认 case timeout 10000ms、test-concurrency=4，日志 `.tmp/retained-history/live-hosts-web-short.log`，实际0.47秒。没有运行复杂回归。

Server 新增 `current_minute_history_is_authenticated_public_and_generation_scoped`，验证认证、正常本人／未入场公开响应、generation 和客户端账户注入；root 的 fresh host47 五 crate 编译实际成功后，获授权执行真实 Server lib 的该 exact case，10000ms外部 deadline、test-threads=4，实际1项通过、38项过滤、零失败，测试报告0.20秒，完整日志 `.tmp/retained-history/live-hosts-server-host47-short.log`。Core owner 的实际分钟与本人阅读测试另行负责，本记录不以适配器空成交 fixture 证明 Core 真实成交已实现。

root 从真实 Engine test 产物按完整 ts-rs 环境生成 CurrentMinuteHistory 类型，未手写 generated。实际类型检查在10000ms外部 deadline 内8.38秒结束，新 Live 文件无错误，但全 app 仍有七项 merge 后既存类型诊断：`protocol/effects.ts` 及对应测试把 string AccountId 当 number，另 `mobile/market-model.test.ts` 的调用参数数量不符；已交 root 分配原 owner 修复，不声称该次整体类型检查通过。完整非作者审查见 `live-hosts-independent-review.md`，最终 fresh Rust 日志由非作者亲读后签核；不扩大为完整回归或所有平台 runtime 已验证。
