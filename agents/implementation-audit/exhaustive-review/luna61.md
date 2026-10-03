# Luna61：frontend review / verification / WASM 旧审计全文 EOF 复核

## 范围、基线与方法

- 逐行覆盖 `agents/oop-refactor-implementation/frontend/review.md`（230 行）、`verification.md`（36 行）、`wasm.md`（77 行），共 343 行，均读到 EOF；本记录只写审计工作文件。
- 当前基线 HEAD `a7c7ce357bdc9f88c03633744b2d5815db49e9b2`；产品提交 `08e4fc75b52a71a3262a8a938c57b44f8b5b4960` 的父为 `b76ece39b3a1635adde52da07375607f19b56ecc`，当前 HEAD 为该产品提交之后的 merge。
- 对照正式决策 ADR-0002、ADR-0005、ADR-0010、ADR-0025；逐条复核旧记录中明确保留的远程 socket 缺陷，并直接读当前生产 caller 与 characterization test。没有运行测试、build、lint 或完整回归，没有产品源码/Git 写操作。

## 全文 EOF 章节矩阵

| 文件及原文行 | 章节/内容 | EOF 与复核结论 |
|---|---|---|
| `review.md` / 1–8 | 范围与限制、基线、旧审查范围 | 旧复核自述是当时的局部审查及未跟踪文件检查；不把旧 SHA 或当时结论当作当前全树证据。 |
| 同上 / 9–28 | R2-N01/N04、首轮 hash | indicator request identity 和 company registry 结论限于所列七文件；没发现与当前正式 host/状态权威冲突。 |
| 同上 / 29–46 | mobile、prototype | 记录明确保留行情单位、Kline 投影和 prototype 实际接线边界；固定样例不是规则来源，VM/SSR fixture 不代表真实浏览器。 |
| 同上 / 47–62 | remote N01/N07/R2-N02 | 当时迁移结论只证明窄 owner、顺序与既有协议保持；原文明确旧 `onerror`、dispose pending 等仍在。对旧 `onerror` 已独立复核，见下节。 |
| 同上 / 63–75 | WASM N04/N05 首轮 | slot/loop 所有权、restore 与 publish 顺序保持结论和当前实现相符；报告承认重复 create 泄漏、部分失败状态等旧边界。 |
| 同上 / 76–95 | fixture 修订、N06/N07 remote、N03/N06/Tauri | 类型/fixture 小修属于当时审查范围；不据测试 fixture 将 React/IPC 扩大解释成真实 GUI 验收。remote 仍显式注明若干既有错误路径保留。 |
| 同上 / 96–131 | FR01、A27/N02/N03、修复关闭 | FR01 是指标 series 删除后 ref 记账漂移；后续逐项清 ref 并增 retry 断言，关闭结论有明确修复描述。不是本轮新问题。 |
| 同上 / 132–170 | App hooks、类型/lint 增量复核 | owner 权威、交易状态和调用顺序结论与所列窄范围相符；hook 私有 dispatcher 限制及静态接线守卫未被冒充为浏览器运行证据。 |
| 同上 / 171–179 | 最终三门结论 | 绑定的是当时 70 文件清单/hash 和记录的验证版本；其“无未解决发现”仅针对该 batch diff、FR01/FR02，不可推导旧 remote 残余已解决。 |
| 同上 / 180–196 | FR02 台账发现、TradingCommands 增量 | FR02 是 caller 分类和错误 prototype 接线描述的文档问题；后续增量 hash 有明示。非产品源码 finding。 |
| 同上 / 197–230 | FR02 复核关闭与最终需求核销 | 39 requirements/70 文件、completion/verification hash 绑定完整；测试、tsc、lint 均为实施方历史记录，本次没有重跑。scope exclusions 与旧 remote 生命周期问题仍须分别看待。 |
| `verification.md` / 1–8 | 批次验证声明、tsc 命令和首轮失败修复 | 明确区分首轮失败与最终通过，且用外部 10s deadline；并发 tsc 结论是历史执行证据。 |
| 同上 / 9–27 | lint、warning 修复、精确测试结果 | 记录最终 lint 零 warning、11 case 与增量 app tsc；remote dispose 等被明确列作已有残余，不因该组通过而视为关闭。 |
| 同上 / 28–36 | diff check 与验收限制 | 没有声称完整回归/build/E2E/真实浏览器/三宿主矩阵。 |
| `wasm.md` / 1–12 | N04/N05 范围、阅读依据 | 两个新 owner 提取；协议、存档 schema、交易规则与 restore helper 未改。 |
| 同上 / 13–26 | owner 与生产 caller 迁移 | `WasmSessionSlot` 单归 handle/generation；`WasmTickLoop` 单归 timer/速度/节拍；Worker 所有消息路径委托 owner，host 仍走原消息协议。 |
| 同上 / 27–42 | 保留顺序、既有缺陷及 TDZ 修正 | 重复 create、restore 部分失败、旧 drop、baseline prepare 等未声称已修；restore reply 旧 generation 后发新 baseline；TickBatch/CivilUpdate 计数和 yield 顺序保持。TDZ `slot` 遮蔽已改为 `savedSlot`。 |
| 同上 / 44–69 | 测试、lint、静态检索历史证据 | 57/57 是实施方历史短测，fixture 执行真实 listener 但 fake bindings/port/timer；不是本轮执行，也不是正式 WASM/browser 集成矩阵。 |
| 同上 / 71–77 | 交易语义与未完成 | 说明 owner 提取不改变 A 股语义，并将独立 review 留给父级；后续 `review.md` 已提供当时 review，但仅对对应 batch。 |

## 旧发现与真实 caller 复核

### 旧 socket `onerror` 可使新连接失败

- 旧发现可追溯 `review.md:49`、`frontend/remote.md:37,64`：明确说 mode 切换后旧 socket 的 `onerror` 仍直接 `fail`，按当时提取范围保留；`remote-state-contract.test.ts:107–116` 还特意将此行为固定为 characterization。
- 当前生产代码再次证实：`remote-host.ts:117–121` 为每个连接取得 identity 并安装新 socket；`:122–127` 的 `onmessage` 经 `handle(..., identity)` 校验；`:130–134` 的 `onclose` 调 `isCurrentConnection(identity)`；唯独 `:129` 的 `onerror` 无 identity/disposed guard，直接调共享 `fail`。
- 实际后果沿当前调用继续成立：`setDeliveryMode` 在 `remote-host.ts:201–208` 先更换 delivery、invalidate identity、close/detach 旧 socket，再在 running 时 `connect()` 建新 socket。此后旧 error 触发 `fail`，在 `:55–62` 停止运行、detach/close 当前 socket、拒绝 command registry 中所有 waiter 并报 fatal。属于可达的旧连接事件污染当前连接，不只是旧测试保留的死行为。
- 独立行为证据：现有 characterization `remote-state-contract.test.ts:107–116` 明确断言旧 socket error 会关闭 `sockets[1]`、报告 `REMOTE_SOCKET`、tick 停止。源码路径足以确认；本次未执行该测试。
- 正式合同：ADR-0010:12–13 要求传输宿主差异不能改变状态、错误与命令确认语义；:59–60 要求 fatal 结构化且有副作用命令等待确认；:67 把重连列为 Remote 宿主能力。文档没有逐字指定 stale callback identity，但“旧连接错误使新连接 fatal”不是重连/状态一致性的合理成功路径。依据足以交主控按 Remote 生命周期候选处理，不宣称这是本 OOP 提取引入，也不擅自改码。
- 结论归属：这是 `sweep61/62` 已发现的同一旧连接候选，归入既有 G05 生命周期/恢复边界，不重复开编号。旧复核“缺陷保持”作为历史迁移说明正确；如果将 `review.md` 的 batch “无未解决发现”解释为产品全局无缺陷，则该解释不成立。

### 其他 remote 残余的限界

- dispose 后 pending command 不 settle、并发 resync 单槽覆盖旧 waiter 都可由当前 `remote-host.ts` / `remote-publisher-state.ts` 与 characterization 覆盖证实；它们已由 `sweep61` 区分为 C61-1/C61-2。ADR-0010 有等待确认、错误显式边界，但对 dispose 期间结果未知和并发 resync 的具体策略没有完整文字合同。维持旧候选等级，要求主控跨审计去重与定级，不把它们混成 `onerror` 同一个触发面。
- `review.md` 中 FR01、FR02 已有明确关闭证据，不复活。FR01 代码逐项删除成功后清除对应 ref，FR02 的最终 caller 分类/原型接线已按真实绑定修正；没有发现本轮给出的新反证。

## WASM owner 边界复核

- 正式分层符合 ADR-0002（Rust engine + WASM binding）与 ADR-0005/0010（Worker `postMessage`、共享 engine 更新单位、宿主控制 step 频率）：当前 `wasm-worker.ts` 仍是异步 initialize 和消息 composition root，slot/loop 不把网络/DOM/交易规则带进 engine。
- tick/publish 的关键调用真实接线可确认：`WasmTickLoop.frame` 每有限倍率 frame 最多 step 一次、Infinity 每 task 一次并 0 delay yield；`publish` 先 post protocol 再 stop/post barrierPaused；Worker `endCivilDay` 通过同一 `loop.publish`。当前实现与 ADR-0005 step 由宿主调用、ADR-0010 统一更新语义相容。
- 存档边界：Worker `restore` 先用请求 generation 守卫，slot 调 `restoreWasmSession`，回 `restored` 时回显旧请求 generation，再发新 generation 的 baseline；此为当前 `wasm-worker.ts:227–243` 的真实 caller。ADR-0025 要求只恢复完整日结档、资金/股份/T+1/费用/披露边界保留；此 owner 改动没有改变解析/schema 或 engine restore 合同，不从静态宿主复核声称重新验证领域内容。
- 边界仍显式存在：`WasmSessionSlot.create` 重复调用覆盖旧 handle 且 generation 增长；正常 Web host 每局建新 Worker，未看到常规用户路径在同一 Worker 重复 create，故不单独确认为用户可达泄漏。restore 交换 authority 后旧 drop 失败、candidate cleanup 失败以及 baseline prepare 部分失败均为旧事务路径的错误状态；真实 Rust `drop_session` 是 registry removal 的 void 函数，测试 fake throwing 不能直接证明生产可返回此普通错误。保留为低层失败边界，不夸大成新缺陷。
- WASM 验证只采纳历史记录自身的边界：fake WASM listener 接线可说明命令顺序；没有本轮真实浏览器 Worker、真实编译产物或三宿主矩阵证据。未发现本次三份文件能推翻既有 restore/tick 语义结论的新证据。

## 结论

- 三份记录均实际覆盖 EOF；旧 batch review 对其当时 scope 的结论仍可成立，不能拿来关闭独立旧功能缺陷。
- 旧 socket late `onerror` 的影响经生产 identity/fail caller 及既有 characterization 独立证实；归并现有 G05，不新增重复编号。其余 remote pending 边界保留既有候选及合同限界。
- FR01/FR02 修复关闭结论未发现反证；WASM slot/loop 的 ownership 与 ADR 分层相符，已记录的低层失败和真实 WASM 验证边界仍未解决/未执行。
