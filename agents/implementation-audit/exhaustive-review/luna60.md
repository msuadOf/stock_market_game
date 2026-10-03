# Luna60：frontend mobile / remote / request owners EOF 复核

复核对象为 `08e4fc7` 产品树；当前 checkout 中三份实施记录与源码一致。此记录仅是独立复核工作文件，不改产品代码、Git 状态或正式文档。只读检查至 EOF 的章节为 `frontend/mobile.md`（42 行）、`frontend/remote.md`（73 行）、`frontend/request-owners.md`（25 行）；另交叉检查真实 caller、相关 owner/test、ADR-0004/0009/0010/0014/0018 和 open questions。交易制度没有新增或被改写；涉及现行 A 股语义的代码只呈现已有价格/量单位与交易阶段，依据仍是项目登记规则及 ADR-0009、0014，未将本次工作冒充成重新查验官方规则。

## 章节 EOF 矩阵

| 记录 / 原行数 | 从实施记录追到真实生产 caller 与状态 | 复核结论 |
| --- | --- | --- |
| `mobile.md` 1–42 | `MobileStockDetail` → `IntradayPanel` / `KlinePanel` → 两个 `Mobile*Projection`；K 线的指标请求继续进入 `useIndicatorResults`。原型 HTML 的唯一交互 script 当场实例化 controller，后续脚本搬节点。 | 行情投影职责、借用输入、React viewport/effect 所有权与 prototype DOM 所有权一致。未见漏掉真实 caller。收盘竞价忽略、均价简化、5/20 游戏交易日聚合的限制都有明示；不等同真实成交均价、VWAP 或自然周/月。 |
| `remote.md` 1–73 | `createRemoteHost` 唯一建 `ReportQueryContext`、`RemoteCommandRegistry`、`RemotePublisherState`。逐一追了 start/connect/message/fail、`requestResync`、mode 切换、load、submitIntent、save/snapshot/tick/day、orders/diagnostics、report queries 和 dispose。 | 原字段及全部生产调用已迁移到窄 owner，socket/HTTP/callback/timer 编排仍留 facade；关键异步顺序与差异 guard 保持。ADR-0010 的 `CommandQueued` 只确认入队，不等于受理/成交，代码及测试一致。 |
| `request-owners.md` 1–25 | `useIndicatorResults` 的 effect capture/pending/microtask/resolve/reject/cleanup；`MobileStockDetail` 的 KDJ consumer；`CompanyQueryCoordinator` 对 page/by-ID 的 begin/finish/dispose/baseline caller。 | 请求身份 registry 未代替 React record / coordinator generation、disposed、cache 或 Redux authority。拒绝及 parse 错误保持可见 error state / failure action，旧请求不得覆盖新结果。测试 fixture 的证明范围已经明确限制。 |

## 原文与调用链核对

### Mobile（`mobile.md` 1–42）

- 原文声称 `MobileIntradayProjection` 是单次 render 只读 projection。源码 `market-model.ts:625–697` 对 minute/auction 点做既有 slice/filter，再统一 scale、progress、均价、trade/signature、坐标与量柱。真实 caller `MobileStockDetail.tsx:101–120` 每次 `IntradayPanel` render 通过 `fromInputs` 创建，`MobileStockDetail.tsx:184–235` 的选项分支负责切换 intraday/K 线；没有跨 tick 可变历史或副作用隐藏在 projection。
- 原文声称 `MobileKlineProjection` 统一窗口、MA、KDJ、量柱与 signature。源码 `market-model.ts:699–771` 借用 `allCandles`，以 `slice` 构造 visible window，MA 从全量 candles 算后切窗，KDJ 只读 `IndicatorResultState.ready` 中 Rust 结果；真实 caller `MobileStockDetail.tsx:71–99` 由 `KlinePanel` 消费，`:78` 的指标输入与 `useIndicatorResults` 仍在 React hook。`MobileStockDetail.tsx:231` 的 `key={code-period}`、viewport reducer 与响应迟到身份校验没有被 projection 吞并。
- 原型段声称 controller 持有 DOM refs。`design/ui/mobile/mobile-trading-concept.html:120–183` 可见构造和事件绑定，生产原型入口在同文件 `:183` 立即 `new MobileTradingConceptController(document).bindEvents()`；它操纵的仍是 DOM 唯一视图状态。现有 DOM 搬移后引用、重复 watch 按钮、程序化 click 等边界测试与实现记录对应。prototype 是静态概念原型，测试 VM 只模拟所用 DOM API，不是浏览器验收。
- 大 A 边界复核：源码的开盘 auction 与 continuous 分开取样/缩放，价格显示单位沿用分转元，量仍沿用股事实；原文说明算术均价仅展示值。ADR-0009 定义竞价轴、昨收对称比例及不展示 `PreOpen`；ADR-0014 要求 closing auction 不画进开盘槽位，记录说过滤由上游完成。没有由展示 projection 引入新的撮合、收盘竞价或上市板块假设。

### Remote（`remote.md` 1–73）

- `ReportQueryContext`：`remote-host.ts:46–48` 构造唯一实例；`queryPublicReports` `:277–285` capture epoch、完整 normalize 后 accept/register page；`publicReportById` `:286–294` 必须先从已登记 opaque ID 查 company，再验证返回 company/id/epoch。baseline `:79–84` 和 load `:269–273` 失效旧 epoch。无 `Number(id)`、未登记 ID 不猜公司。与 ADR-0010 opaque transport/权威合同相容。
- `RemoteCommandRegistry`：`submitIntent` `remote-host.ts:209–216` 在 socket/baseline 预检后 next → register → send，await 的 Promise 由 `queued` (`:99–102`) 或带 request id 的 gateway error (`:103–110`) 完成；`fail` `:54–62` 批拒所有登记 waiter。ADR-0010 `:60` 明言宿主确认要求，且 `CommandQueued` 不代表订单受理或成交。server/EngineHost 与 UI caller 未绕过这一层。
- `RemotePublisherState`：`remote-host.ts:65–73` `requestResync` 先置 awaiting、send，再装单槽 waiter；baseline `:79–84` 先 epoch/report invalidate/cache/clear-awaiting，再 callback，再 resolve waiter。`fail` `:54–62` 先 detach/take waiter、拒 waiter、close socket、reject commands、fatal callback。mode switch `:192–198` invalidate identity 后 close/detach 并按 running 重连；message/close identity guard，旧 onerror 仍直接 fail。load 只在原连接不存在时创建 5000ms timer（`:130–141`, `:267–273`）；其余 resync 无新 timer。queries 按源码确实存在不同 guard：save `:219–230` 检 generation/awaiting/disposed，orders `:232–245` 与 diagnostics `:298–310` 还检 epoch；未有统一增强。
- 状态/Promise/error/dispose 追踪：dispose `remote-host.ts:168–189` 拒绝 baseline waiter、detach/close socket、清 callback 并触发 DELETE；它没有 `commands.rejectAll`。命令 pending 在 dispose 后不会 settle，这是记录 `remote.md:27,64` 和旧 review “不视为已解决”所确认的遗留，不是本轮隐藏修复。`submitIntent` 的 synchronous `send` throw 也会 reject当前 Promise但留 registry entry，见记录 `:27,64` 与测试边界；未来异步 gateway 可能再命中同 request。单槽 waiter 覆盖不 settle 旧 waiter；fail 保留 cached baseline；旧 socket `onerror` 可 fail 当前 facade，均由旧记录逐项保留，没有被误报为已修复。
- ADR 确认：ADR-0010 明确 baseline 初始化/读档/显式重同步、结构化 fatal 与命令确认；ADR-0018 是 `proposed`，不能拿其提案当已接受规则。无新的 A 股规则变更，也不需用拟议长期时间线决策替代已接受 ADR。

### Request owners（`request-owners.md` 1–25）

- 指标真实状态链：`MobileStockDetail.tsx:78` 调 `useIndicatorResults`；`useIndicatorResults.ts:13–42` 每次 effect 创建 `IndicatorResultRequest`，同步置 pending、保留 Promise microtask，resolve/reject 仅在 `gate` ticket 与 calculator/input 身份匹配时写 React record，cleanup invalidate。`IndicatorResultRequest` 见 `indicator-results.ts:44–91`；组件在 `MobileStockDetail.tsx:88–97` 把 `pending/error/ready` 传给现有 KDJ 呈现。组件没有维护第二份 KDJ 算法/请求 authority。错误由 `rejectRecord` 变为可显示 error state；`enabled=false`/calculator 缺失对应既有 idle/unavailable 合同，而非吞错默认值。
- 公司请求真实状态链：`App.tsx`/协议 composition 组装 `CompanyQueryCoordinator`（本轮记录标其为 R2-N04）；coordinator `company-query-coordinator.ts:45–90` 唯一创建 registry，query/by-ID 在 await 前登记 ticket，`finally` 仅当前 ticket 可 finish；`installBaseline` 与 `dispose` 清 registry/cache 并递增/隔离 generation（`:69–82`）；所有错误转成 `recordCompanyQueryFailure` / unsupported action。`CompanyRequestRegistry` 不复制 coordinator 的 generation/cache/disposed 权威。检查到异常后会显式 dispatch 错误，不存在空 catch 或 return-null fallback。
- ADR-0004 的 Redux 仅作为 UI/序列化快照编排缓存，engine/host 是权威；R2-N01 hook 的 record 在 React 本地，R2-N04 registry 仅持请求序号/活动 ticket，符合该状态边界。没有交易制度或存档/API 语义变化。

## 旧发现与残余

- `frontend/review.md` 中 remote 独立复核明确把 dispose pending command、同步 send 留登记、单槽 waiter 覆盖、旧 `onerror` 及 fail 后 cache 保留当作既有残余；再次按当前 `remote-host.ts` 和 `remote-state-contract.test.ts` 核验，均仍真实存在，文档没有错报关闭。适用范围是 ownership refactor 行为保持，故这不是要求本批偷偷扩张的修复。dispose pending command 是最值得登记后续显式行为决策的一项：dispose 永久移除可接收确认的 socket/callback，promise 可能永不结束并令 await caller 持续等待；ADR-0010 要求有副作用命令等待 host acknowledgement，却未规定 dispose cancellation 时的 Promise 终态。建议另批定义为结构化 reject/cancel 并补测试，在该边界确定前不宣称已解决。同步 send 例外也可能遗留 registry 项，但立即拒绝当前 Promise；需决定是否清登记及迟到确认处理，避免增加隐式重试。
- `frontend/review.md` FR01（`PriceChartRuntime.removeSeries` 部分失败记账）标记为已逐项清 ref、补 retry 测试并关闭；不在本次 mobile/remote/request-owner 三章范围，本次没有反证。
- `request-owners.md` 中 hook fixture 用 React 私有 dispatcher，不是真实 DOM/StrictMode/E2E；原文明示，未发现把 fixture 证据夸大。mobile 原型 VM、移动组件 SSR 也仅证明声明范围。

## 新候选与反证

1. **候选：mobile projection 所谓只读并未深冻结所有派生对象。** 反证：它们在每次 render 由只读输入构造，没有跨 render/tick共享缓存或副作用；readonly 是 TS API 语义。`MobileKlineProjection.allCandles` 直接借用来源且测试明言 input 未变，旧复核确认借用 snapshot/frame 不冻结；新增 deep freeze 会越出需求且损害性能。无发现。
2. **候选：requestResync 在 send 后才登记 waiter，响应可否同步抵达造成丢失？** WebSocket message 事件异步投递，不会同步重入 `send`；同步 throw 时根本不建立 waiter，当前 callers catch 后走 `fail` 并显式报告，既有测试专门固定这个顺序。无新增竞态证据。注意这不消除前述 promise registry 残余。
3. **候选：历史报告 epoch 与 baseline generation 混用，晚到结果可能污染新时间线。** 反证：`ReportQueryContext.captureEpoch/acceptPage/validateReport` 对 load/baseline timeline invalidate，先验证整页再登记，id 查询还校验 company/id；epoch 是公开报告 query 失效身份，不是假装 server generation。实际 adapter late-response 测试覆盖。
4. **候选：projection 中算术均价、5/20 聚合或 prototype 数值违反真实 A 股。** 反证：三章明示为游戏旧显示/聚合约定，不是成交均价、日历周月或交易制度；ADR-0009/0014 阶段语义未改，故无需把 UI 重构升级为新规则实现。

## 最终判定

指定三章及真实 caller/状态路径均追至 EOF，前轮发现修复状态核验一致；未发现 ownership 提取引入的阻断问题或遗漏的交易语义变化。保留的 Remote pending/dispose 与同步 send 遗留应作为单独显式错误/取消语义决策跟进；本次只记录，没有修改产品代码。测试、build、E2E、tsc 与 Git 写操作均未执行。
