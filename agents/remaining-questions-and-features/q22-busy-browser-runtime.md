# Q22 忙碌 production step Browser 验证

## 范围与状态

本项补充 ADR-0032 对 Browser 真实 production step 忙碌期间、NPC 完成登记与 cutoff 的运行证据。
验证脚本与 feature-gated bridge 已接线，两条真实 Browser 短验收已通过，最终非作者完整增量
复核 PASS。本记录只核销本批忙碌 Browser 证据，不把此前外层 owner gate 验证改写为当时
已有真实 NPC 忙碌证据，也不扩展为整项目／全部交易规则验收。

仅使用 `verification-harness` 控制真实 Rust NPC `before_decide` 与 cutoff 冻结后握手。
控制不写现金、股份、订单簿、Strategy 或 receipt，不以 JS 时间戳、外层 JS gate 或返回后补盖
receipt 代替真实登记。正常生产 WASM 不包含这些阻塞控制。

## 隔离接线

- 特制产物位于 `apps/web/wasm-verification-pkg/`，不覆盖正式 `wasm-pkg/`。
- standalone owner Worker 直接调用正式 `create_session`、`step`、`player_working_orders`。
- Intake 使用原生产 `wasm-ingress-worker.ts`，验证服务的 Vite alias 将其 bindings import
  解析到同一个特制 binary 的 JS bindings，避免不同 binary 的函数索引混用。
- Observer 使用同一个 compiled `WebAssembly.Module`、shared `WebAssembly.Memory` 经异步初始化建立独立
  instance，只按 ingress token 访问 feature-gated 控制与真实只读 trace，不依赖 owner thread-local registry。
- fixture 仅一只沪市 MainBoard 股票与一名 retail，保留分报价、100 股整手、资源冻结与生产
  受理规则；4 tick 日内与零竞价窗口仅为已有测试压缩时间，不冒称真实交易所日程。
- 实际请求使用 100 股与 200 股的不同完整 payload，验证当前/下一 tick 归属及 exactly-once。
  retail 的真实个人风格与关注节奏可能使当 tick 不出单，不能将“worker 已决策”冒充“NPC 已登记”。
- `npcCompetitionRuntimeVerified` 必须同时满足真实 Player 先于 NPC 的股票 receipt、下一 tick
  两者各自唯一对应的生产 `OrderAccepted`、原真实订单簿 `Order.seq` 保留同方向入口次序，
  而非仅观察 trace。此标签不宣称同价 price-time
  胜者、同账户资源胜者或成交结果；这些领域规则仍由已有专门 Engine 测试验证。
- 验证专用 owner 只读 export 使用既有低层 `game().save()` 内存投影，只返回指定存在账户与
  股票的原 `Order`，未知账户／股票及缺失订单簿 key 均明确报错；不写日内公共存档。

## 命令与已有证据

```bash
node scripts/run-with-deadline.mjs 10000 -- node agents/remaining-questions-and-features/q22-busy-browser-runtime.mjs
node scripts/run-with-deadline.mjs 10000 -- node agents/remaining-questions-and-features/q22-busy-browser-runtime.mjs 2 --same-price
```

第二条为独立十秒同价 witness，不合并挤占第一条 deadline。它保留 total shares、初始现金、
真实 seed 2 Factory 与 Strategy，仅将流通盘置零并使用现有可选简化 `price_cage_enabled: false`，
Player 使用 `Highest`。必须在真实受理结果中确认 Player/NPC 均为 Buy、价格严格相等，且原
Book `seq` 保留更早 Player receipt。零流通盘不是现实上市 A 股配置，关闭 price cage 不是
声称现实放开申报约束；这个隔离 fixture 仅证明同价时间序，不能替代默认 cage 主用例，
也不宣称已有真实撮合成交胜者。日志显式输出 `priceCageEnabled`、`floatShares` 和独立标志
`samePriceTimePriorityRuntimeVerified`，主用例该标志保持 false。

脚本显式选择 `min(4, availableParallelism())` 个 Rayon threads，至少 2 个；Engine 与 Intake
各一个 Worker，Observer 一个 instance。外部 supervisor 同时约束执行与进程树清理为十秒，
case timeout 声明为 10000ms；握手等待为 3000ms，失败 finally 释放当前 Rust gate 并终止两个 Worker。

首次命令误用 `--timeout-ms` 参数，仅打印 usage 并退出，不属于业务红灯。随后上述正确命令真实
退出 1，耗时 0.61 秒：独立目录缺 `web_wasm.js`，报 ENOENT。这是产物缺失红灯，不是已有控制
违反行为断言的红灯。`node --check` 已通过。未运行 Cargo、未操作 index 或 commit。

父任务已报告 root 统一执行两项 Native 行为红灯：实际 production step 成功执行后，没有进入
配置的 NPC gate（0.23 秒）与 cutoff gate（0.40 秒），断言失败；不是编译错误。收到该先红门禁后，
本子任务才加入 WASM 三个控制 exports；当时编译和绿色运行仍待 root 统一验证。

父任务随后使用实际 build 6 Engine rlib 生产者，在外部十秒 deadline 下以 8 个 OS 线程、
Rayon 32 对 8 个 seed 与两种流通股 fixture 探查。真实原流通盘／现金、1 retail、1 stock、
4 tick、arrival rate 1 的 seed 2 在 tick 1 产生唯一真实 `Buy 300 Highest`，没有初始挂单占用
receipt；故 Browser 默认改用 seed 2，保留原流通盘与 `price_cage_enabled: true`。结果为
`.tmp/checklist-wave4/q22-fixture-probe-results.json`，由父任务提供，不是本子任务独立执行。
该 Native fixture 证据不代替 Browser 运行通过。

root 首次隔离 WASM 构建因 `wasm-pack --features` 之后的参数会透传 Cargo，将 `--out-dir`
放在其后而报工具参数错误；不是业务红灯。随后按下列参数顺序重试，正式 `wasm-pkg/` 未覆盖：

```bash
wasm-pack build apps/web-wasm --target web --release --out-dir /data1/baiyifan/workplace/stock_market_game/apps/web/wasm-verification-pkg --features verification-harness -Z build-std=panic_abort,std
```

该构建由 root 在统一工具链／多核环境与外部长验收 deadline 下执行，日志为
`.tmp/checklist-wave4/wasm-verification-build-8-retry.log`。父任务报告实际 Engine 编译达到
229% CPU、11 threads；此处只记录来源，不冒称本子任务自行观察或构建通过。

该次隔离编译随后真实报 E0277：新增只读 export 将 `GameSession::save()` 的 `StepFatal`
直接交给仅接受 `SessionError` 的 helper。已最小修复为 `SessionError::from(fatal)` 后再经
原错误 DTO 序列化，保留 fatal 类型与详情，不使用默认值或吞错。这是 feature export 编译
失败，不是 Browser runtime 行为红；此前未启用该 Web feature 的 Native 构建不证明此 export
通过 typecheck；该时点的绿色重建仍待 root 验证，后续实际结果见下一节。

## 验收边界

- Engine verification API 已由父任务落位，真实生产 hook 由父任务在行为红灯后接线。
  `web-wasm` 已新增 `verification-harness` 到 Engine 的同名 feature 映射、三个控制 exports
  及经 root 批准的一个 owner 只读订单簿 export。
- 脚本首先检查正式 bindings 不存在 `verification_` exports，再检查隔离产物的四个验证 exports。
- 最终非实施者复核已包含下面记录的初始化／清理修复、完整 payload 与 Book 时间序断言。
- 不扩展为同账户资源胜者、真实成交赢家、默认 cage 的同价成交、完整回归或全规则验收。

## 当前源码产物与 Browser 实测

父任务确认 root 的独立 `verification-harness` WASM build 9 成功，59.67 秒；随后相同当前源码
正常 WASM build 9 成功，约 1 分 01 秒，日志为 `.tmp/checklist-wave4/wasm-production-build-9.log`。
收到正常产物 ready 授权后，本子任务才运行下面两条短命令，正式 bindings 的
`verification_` exports absence 断言均通过；不以旧生产包缺少 exports 冒称当前源码隔离成功。

初轮两命令并行，真实失败分别为 8.33 秒和 9.09 秒：Chromium 对主线程同步实例化超过
8MB 的 WASM 有限制，而 Observer 尚未完成初始化时 finally 的 release 又覆盖了原错误。
两份原始日志保留为 `q22-busy-browser-default-runtime.log` 与
`q22-busy-browser-same-price-runtime.log`；后者还实际耗尽执行预算并由外部 supervisor
终止进程树，不记为通过。Vite 同时执行不必要的依赖扫描／bundling，增加了短用例启动成本。

最小修复先关闭这个独立验证服务的 dependency discovery（未修改正式 Vite 配置），并只有
Observer 初始化成功才尝试 release。默认用例 retry 1 实际失败 4.87 秒，原始日志
`q22-busy-browser-default-runtime-retry-1.log` 清晰显示主线程 `WebAssembly.Instance` 的
8MB 限制。随后改用 bindings 异步默认初始化，仍使用同一 compiled Module/shared Memory；
没有 Chrome 特例 flag、外层 JS gate、timestamp 排序或替换生产 Intake。

两条 retry 2 命令再次并行、独立 session/Browser/十秒 supervisor，均 exit 0：

- 默认 cage 主用例 **6.038 秒**，原流通盘 3,571,428,571 股、`price_cage_enabled: true`。
  原始日志 [`q22-busy-browser-default-runtime-retry-2.log`](q22-busy-browser-default-runtime-retry-2.log)。
- 独立同价 witness **6.044 秒**，零流通盘、`price_cage_enabled: false`。
  原始日志 [`q22-busy-browser-same-price-runtime-retry-2.log`](q22-busy-browser-same-price-runtime-retry-2.log)。

两者实际使用 Rayon 4 threads、Engine/Intake Worker 各 1、Observer instance 1。NPC 真正处于
Rust `npc_decision` gate 时，Intake 接收 Player 100 股并分配真实 receipt；释放后真实 Factory
Retail 决策登记 Buy 300，Player 股票 ordinal 更早。tick 1 没有 Player 活动委托；tick 2 cutoff
已冻结 100 股时真实接收的 200 股只进入 tick 3，两次 cutoff 数组严格保存各自原 receipt，
account/stock ordinal 严格递增，未重复消费。

默认用例生产受理与实际 Book 为 Player Buy 100 股／1008 分／`seq: 0`，NPC Buy 300 股／
1028 分／`seq: 1`，不假称这两个不同价格是同价 witness。同价用例两者均为 1232 分、Book
时间序仍为 0／1。owner/account/code、qty、side、实际 price 与 `OrderAccepted` 严格对应，
`OrderId` 仅用于关联原委托而非排序。tick 3 唯一迟到 Player 为 200 股，旧 100 股仍恰一次。

日志中的 deprecated initialization warning 来自既有 Rayon bootstrap，port occupied 警告来自
两个独立 Vite 服务自动选取可用端口；它们没有吞错或改变 gate/receipt 证明。本批不修改正常
生产初始化。本子任务未运行 Cargo、未操作 index 或 commit；正式构建与整体复核由 root 收口。

## 独立复核

非实施者 `review_shared_ingress_core` 完整增量复核 PASS，无剩余有效发现。复核覆盖 feature
控制与四个 exports 的隔离、真实 gate/receipt/cutoff、两份当前 Browser 日志、异步 Observer、
失败清理、完整 payload 与实际 Book 时间序及工作记录 chronology；没有改变 Strategy、资金、
股份或生产 schema。reviewer 未独立重跑 Browser/Cargo，绿色运行依据上述作者/root 实际工具
证据；Q08/Q13 并行改动不在该复核结论范围内。
