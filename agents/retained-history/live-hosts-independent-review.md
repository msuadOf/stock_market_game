# 当前分钟 live 宿主接线独立复核

## 范围与结论

本次只审查新增的 CurrentMinuteHistory 跨层接线，不复用此前 UI merge／历史恢复宿主复核结论。范围包括 Engine DTO 与访问层、Server actor/route、Desktop actor/Tauri command、WASM FFI、Web `EngineHost`/Worker/WASM worker/Remote/Tauri 查询、严格 wire normalizer、`MarketRuntimeProvider`、测试 fixture 与相关测试。结论为 **有条件通过**：已读代码路径未发现本人身份、generation 隔离或 A 股分钟事实语义的明显漂移；Web 短测 5/5 与 Server 新 route adapter/auth/generation/public-viewer 短测 1/1 通过。此结果只签核当前新增接线，不替代 Core 非空成交的 live 事实测试或全宿主矩阵验收。

## 语义与接线

- Engine `CurrentMinuteHistoryRequest` 使用 `deny_unknown_fields` 且只有证券代码，不能由调用者注入 account。响应显式标 `live: true`，含自然日、`CivilInstant observed_at`、该证券状态/交易阶段及真实 `MinuteBar`。交易日当前分钟仅从 `retained_market_history.current_bars` 取事实；休市返回空 bars；查询不调用归档写入。日终前查询不会改变 save，CivilDay 结束才按原有机制归档。
- 本人记账由 Engine 的 `query_current_minute_history_for(account, ...)` 执行；非法账号/证券拒绝，不先记账。Server actor 仅在已解析出会员 account 时走该方法；未入场或等待重新确认的 public viewer 调用只读 `current_minute_history`，没有伪造 `AccountId(0)`。Desktop 与本地 WASM 的 `AccountId(0)` 是其单一玩家语义，调用本人记账入口，与 Remote guest 路径不同。
- Server route 要求授权、规范 generation，body 禁止未知字段，过期 generation 返回 conflict；Tauri/Worker 也携带当前 generation 并校验响应 generation。Remote 额外校验请求期间 member account 未变；Provider 在 await 前后校验 host、市场 generation 和玩家 account，防止迟到响应安装到已切换身份/会话。
- 前端 normalizer 对请求/响应使用 exact keys，检查六位证券代码、civil date/observed date 一致、second-of-day 范围、live 标志、Trading/Closed 与 phase 一致、bar 严格递增且不晚于 observed minute，并复用 `normalizeMinuteBar` 校验阶段、价格、成交量/笔数/金额的字符串与 u64 边界。Rust DTO 用整数类型及 serde 拒绝未知字段；金额保持 decimal string，成交量/笔数保持 u64 wire string。未发现 `NotEnded` 历史归档接口被复用或日内存档行为新增。
- 没有发现交易撮合、结算、板块规则或真实交易所制度实现改变；该接口读取引擎已发生的分钟事实，无需新增交易所规则依据。

## 验证与限制

- `.tmp/retained-history/live-hosts-web-short.log`：5 个 Web case 全部通过，覆盖 strict live response、Remote public query/generation、Tauri 与 Worker 隔离。实现者报告 parser 首轮真实红测试在 thread 中记录，未提供可复核日志，本记录不将其作为独立验证证据。
- `.tmp/retained-history/live-hosts-server-host47-short.log`：精确 Server route case 1/1 通过（38 filtered out，case 0.20s）；覆盖无授权拒绝、member/public viewer 响应相同、stale generation conflict 与账户注入拒绝。该 adapter fixture 响应 bars 为空，仅验证 route/auth/generation/public-viewer 路径，不代替 Engine/Core 的非空真实成交断言。
- 已额外亲读独立 Core 复核 `agents/retained-history/live-core-review.md` 与 `.tmp/retained-history/host48-live-final-green.log`：Core live 实际非空成交、阶段转换、休市证券共享观察时刻等 7 项通过；这补强 live source-of-truth 证据，但不等同于所有 Browser/Remote/Tauri/Server host 的非空成交端到端矩阵。
- root 报告 host47 五 crates compilation 成功；本复核只亲读并确认上述短测日志，不将其扩大为全量 Rust 回归或全宿主矩阵验收。没有运行 Cargo 全量回归。
- 未运行全量回归；本结论不代表未列明的历史存档/市场历史功能或完整应用验收完成。
