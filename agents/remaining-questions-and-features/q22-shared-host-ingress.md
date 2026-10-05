# Q22 共享真实宿主 ingress 实施记录

## 领域边界

依据用户 Q22 回答与 accepted ADR-0032，Player 与 NPC 按同一个每会话入口实际登记的 receipt 竞争；不按来源类、key 或 OrderId 排列业务优先级。现有 A 股价格时间、账户 Cash/Share、手数、费用、T+1 与 ExpiryShadow 校验保持不变。本批没有新增真实交易制度、真实行情、已有账户补钱、日内持久化、WAL 或 schema version 兼容层。

## Core 方案

- `SharedSessionIngress` 只拥有账户身份索引、入口生命周期、receipt cursor 与尚未完成消费的 Player 输入；没有账户现金、股份或订单簿 authority。极短入口 metadata `Mutex` 一次分配同账户、同股的两条 ordinal，撮合与账户验证不持有此锁。
- `ProtocolSession` 创建后启用每会话 shared receiver。直接 Engine `GameSession` 未启用 shared receiver 时仍沿用原同步入口；启用后 Player 入站与 NPC 单账户完成后的 projection 共同使用 receiver。
- `TickShadow::capture` 一次冻结 Player cutoff：当前 tick 只消费此时已正式登记的完整 payload；之后到达的输入留给下一 tick。NPC private receipt 也在共同 receiver 分配，但 private intents 只写入当前 candidate 的下一轮 `PendingNpcBatch`。
- 每个 shadow/checkpoint 有独立消费 cursor 租约。成功 CommitTick 才替换 authority 的 cursor；失败 candidate 被丢弃，不删除真实 Player 输入。宿主未发布批次 rollback 后，checkpoint cursor 仍能重读这段 Player 输入。只保留最旧活跃 cursor 后的暂存输入，失效弱租约及已经完成的前缀在下一次 snapshot 清理；不是永久 journal 或持久化 WAL。
- private NPC 失败可能留下入口 ordinal 空洞，这是已经发生的接收 metadata，不代表委托提交；失败不发布 NPC intents、业务状态或事件，也不回拨已经由并发 Player 使用的 cursor。
- `GameSession::save` 对 shared receiver 中尚未导入 shadow 的 Player 输入及 receipt cursor 做投影，不因同步 actor 尚未处理而遗漏已接收事实。公共日级 archive 仍由既有自然日日终边界提供，没有新增日内落盘入口。

## Native 接线

Server/Tauri 实施由 `native_ingress` 子任务负责：handles 入站直接访问 shared receiver，不先排入阻塞 actor 的 Enqueue command；会话 restore 用仅涵盖生命周期的 slot 切换 receiver，并关闭旧 generation capability。鉴权仍由既有宿主入口负责；业务 validate/match 仍在 Engine。

独立审发现稳定宿主 handles 自动指向新 receiver，不足以拒绝旧页面迟到请求。因此本批还在真实 REST/WS/Tauri enqueue envelope 显式携带 canonical generation 字符串，slot 在同一个生命周期 guard 内验证 generation 后才分配 receipt；不默认补当前代或建立旧协议兼容。Remote/Tauri Web adapter 已同步提交 envelope。最终实测见后文 `final-build-3`；WS 补充 exact 与最终完整复核由 root 收口。

## Browser 接线

- 新增独立 `wasm-ingress-worker`，与 Engine worker 共享 compiled `WebAssembly.Module` / `WebAssembly.Memory`，拥有独立 JS bindings instance 和 thread-local state。
- Rust `INGRESS` registry 保存每会话 `SharedSessionIngress`。token 不重用，旧会话 drop 时 close；intake worker 直接通过 `ingress_enqueue` 登记完整 Intent，不在忙碌 Engine worker 返回后补盖 receipt。
- `submitIntent` 等待 intake bind，随后只发给 intake worker，不等待 Engine worker 消费旧 Enqueue。generation 单调绑定、旧 token 拒绝、错误显式响应、dispose/fatal 清理两 worker。
- Browser 实施者报告代表性短测试 29/29 通过（外部 10000ms deadline、case 10000ms、并发 4），TypeScript 检查通过。新增 receiver 测试先因模块缺失红灯，随后通过；真实 WASM 多线程代表性验收见后文，仍不等于生产忙碌 NPC 竞争验收。

## 验证与未完成项

- Native 已添加不 poll actor `cmd_rx` 的真实 host red tests，要求 Player enqueue 在 100ms 内完成、随后生产 step 产生 working order；root 统一编译与执行，实施者未自行 Cargo。
- Core 已添加 concurrent Player/private receipt、cutoff、失败丢弃、checkpoint rollback 保留、关闭 generation 等短合约测试，最终 root 实测通过，见下文分阶段日志；不用源码存在冒称通过。
- Core 生产 step 的跨来源同账户 winner／同股顺序、NPC stream 并发握手与三宿主生命周期/失败路径已通过代表性短测；真实 Browser 忙碌 production step/NPC/cutoff 竞争 runtime 仍待验收。
- 必须由未实施该批的 subagent 复核完整 diff；有效发现修复后再次复核。在完成这些门禁前，本批与 Q22 整体都不能宣称完成。

## 阶段验证与独立复核

- root 第一轮统一实测 Core 11 个 exact case 全绿，最长 0.78 秒；两个 Desktop 真正 intake/rebind case 分别 0.19 / 0.60 秒通过。日志位于 `.tmp/checklist-wave3/*-shared-core-green-1.log`、`.tmp/checklist-wave3/*-desktop-shared-green-1.log`。它们只覆盖当轮源码，不是后续 generation API 修复或新增 NPC stream case 的最终结果。
- `shared-host-green-build-1` 的 Server 有 E0596 编译失败，已交 Native 作者修复；其他三个产物成功。不能将该批报告为四包全部通过。
- 非作者 `review_shared_ingress_core` 第一轮发现 hash 穷举漏 `ingress` 字段、PlanChain local/source cursor 分叉、公共日终 archive overlay 带活动 Player 三项；均已修复并经同 reviewer 再复核。新增实际 ready PlanChain stamp、failed private receipt gap 的 live save→restore，以及公共日终 archive/live exactly-once 测试。
- 完整宿主复核又发现真实 Native API 未带 generation；root 已批准本批严格修复。Desktop 真实 stale generation red case 确实失败（0.20 秒），不是仅接口编译失败。REST red 编译当时受旧 pending tuple fixture 阻断，随后改为真实 Engine producer 生成 receipt fixture；最终绿色与 transport 补测结果见后文，不把当时阻断列为当前待办。
- Browser registry 已新增三个 Rust 短测试：producer 跨 scoped OS thread 进入同 receiver、关闭旧 token / 已有 clone 及新 token 不复用、两个会话输入隔离。未自行 Cargo，待 root 统一实测。
- Web generation 修复由 Browser 作者实测新增两 case 红→绿（先报 Missing expected rejection）；相关 Remote/Tauri 短集 41/41，通过 TypeScript 检查。命令同时使用外部 10000ms deadline、case 10000ms 与并发 4。
- 后续新增 `shared_receiver_accepts_player_between_fast_and_slow_npc_completions`：真实 NPC stream 在受控慢账户 hook 等待时，先登记快账户 receipt，独立 OS thread 通过 shared source 登记 Player，再释放慢账户。它验证实际 stream 完成和共同入口 metadata 的交错，不冒称完整 Browser production step 的 NPC projection 运行验收。
- root 已用 pinned `nightly-2026-09-05`、jobs 32 / Rayon 32 和外部 300000ms deadline 完成正式 WASM release 重建，耗时 64 秒；日志 `.tmp/checklist-wave3/shared-ingress-wasm-build.log`。本机 Playwright 1.63 / Chromium 1243 / Node v25.8.2 的真实 Browser 代表性验收随后两次通过，末轮含 Vite/Chromium 初始化及清理共 4.13 秒，外部 10000ms deadline，实际 Rayon 4 workers、Engine/Intake worker 各 1。
- 真实 Browser 使用 production `wasm-worker.ts` / `wasm-ingress-worker.ts` 及 compiled Module / shared Memory。外层 owner gate 未释放时已收到完整 Player 委托；释放后生产 tick 1 有唯一 100 股、1008 分 Buy；生产 day-end save tick 30 恢复 token 1→2 / generation 2，旧 Rust token 与旧 Browser generation 显式拒绝，随后 tick 31 只含新代 200 股单。原始输出 `agents/remaining-questions-and-features/q22-browser-ingress-runtime.log`（被全局 `*.log` 忽略，但在盘），脚本与中文说明为同目录 `.mjs` / `.md`。
- 该 Browser fixture 明确无 NPC，外层 gate 只阻止 owner 接收消息，不冒称 production Rust step 忙碌。`productionStepBusyRuntimeVerified` / `npcCompetitionRuntimeVerified` 明确输出 `false`；真实生产忙碌 NPC、Browser cutoff/跨来源 ordinal 仍无 runtime 证据，因此 Q22 整体保持开放。
- root 阶段复核 focused Web 五文件共 60 tests 通过（358ms），TypeScript force 编译通过；日志 `.tmp/checklist-wave3/shared-host-web-green-1.log` / `shared-host-tsc-1.log`。后续 Browser 生命周期修复后又实测 64 tests 与 TypeScript force 全绿，最终 Rust 与复核见后文。
- WASM 编译暴露的本轮 test adapter dead-code 已按实际调用收窄 `cfg(test)`：`CapturedDecisionSnapshot` 测试 import、`project_npc_state`、`NpcAccountDecision.account` 与 `capture_decision_chain_roots`。未 `allow` warning 或删除测试；其他既有 warning 不借本批顺手改动。

### 最终收口中的失败与修复

- `shared-host-final-build-1` 新 NPC stream 握手测试因 `NpcDecisionStreamError<()>` 缺 `Debug` 而编译失败；添加正常 `Debug` derive 后 `final-build-2` 四包/API 编译成功，28.56 秒。没有把编译失败称为业务红灯。
- root 发现 WASM `SessionRegistry::remove` 未验证本地 ownership 就访问共享 `INGRESS`，错误 token 可关闭其他 registry 的会话。新增错 token 合约在外部 deadline 下真实红灯（0.19 秒），随后最小 owner guard 修复；不通过串行运行所有 registry 测试掩盖此问题。
- 最终完整独立复核发现 Browser consumer 返回 `false`、初始化 timeout 未关闭 intake worker/scope，以及 enqueued 确认后未再次验证 generation/disposed。三项都经作者真实红→绿修复；root 最终 focused Web 64 tests 全绿（354ms）、TypeScript force 检查通过。
- `final-build-2` 后 48 个 exact 实测为 46 绿 / 2 新失败：Server fatal rollback 的完整 live save cursor equality，以及在已经 closed 的原 actor receiver 上续行 intraday 验证。fatal 产品行为明确不可恢复，不能为测试取消 close 或放开 Start/Restore。
- 经 root 与非作者核定，新增仅 `verification-harness` 可用的 `ProtocolSession::fork_for_verification`：完整克隆 ProtocolState / intraday / fact cursor，先同步真实未消费 Player/cursor，再 detach 旧 binding、建立独立 receiver。原 actor 及旧 capability 保持 closed，原 fatal API 拒绝断言不变；Server 只在 dev-dependency 对同一个既有 engine crate 启用该验证 feature。
- fatal rollback 测试继续严格比较完整业务 save JSON、business hash、pending receipts 和 retained intraday；只有不可撤销的外置 `ingress_receipt_cursors` 单独严格断言原 lane 不丢、数值不回退、零成功步完全相同、已有成功步产生实际 gap、所有 pending receipt 小于相应 next cursor。这不是删除业务字段或放宽资源/事件回滚。
- 新增验证 case 覆盖 outer checkpoint 后由独立 OS thread 收到的真实 Player、malformed production frame 失败/rollback、失败非空 private batch 留 gap、旧 source 拒绝、fork 不消费原 reader、分支输入不污染原会话，以及后续真实 ordinal / exactly-once 受理与 retained frame。
- WASM protocol 17 cases 并行 16 的第一次实测为 16 绿 / 1 新失败：跨线程同源测试误假定同股 ordinal 从 0 开始，真实 seed 71 的初始 NPC 已占 receipt。仅将 fixture 改为正式 enqueue 前读取实际 account / stock baseline，继续严断两条 Player receipt 连续与最终 next cursor 增量 2；没有删除 NPC 或降低同源证明。
- root `final-build-3` 四包 lib + Server API 编译成功（65 秒），49 个 exact 全绿，包含 verification fork、两个 fatal rollback 修复、Native generation 与 Core 合约；WASM protocol 17 cases 并行 16 全绿（0.83 秒）。日志 `.tmp/checklist-wave3/<kind>-<test>-final3.log`、`.tmp/checklist-wave3/wasm-protocol-final3.log`；最终 Web 64 tests 与 TypeScript force 同样通过。没有把前轮失败覆盖成从未发生。
- verification fork、严格业务/外置 cursor 分层、原 fatal 保护与生命周期修复已再次经非作者静态复核 PASS。追加 Native 编译成功（47.09 秒），WS restored-session/gateway 两个 exact（0.61 / 0.20 秒）、Server actor integration 17/17（0.90 秒）、intent-known-session / 拒绝活动 Player archive 两个 REST case（0.20 / 1.55 秒）全部通过。日志 `.tmp/checklist-wave3/shared-native-integration-build.jsonl` / `.stderr`、`*-transport-final.log` 与 `actor-integration-final.log`。
- 最终源码正式 WASM release 重建成功（59.78 秒），root 同一正式十秒 Browser basic 命令 exit 0；真实输出原样归档为 [final runtime log](q22-browser-ingress-final-runtime.log)，完整日志 `.tmp/checklist-wave3/browser-ingress-final-runtime.log`。实际 4 Rayon threads，阻塞 owner 时登记 full payload，生产 step、日终 producer restore、旧 token/generation 拒绝及 tick 31 唯一新代 200 股单全部通过。该结果对应最终源码，不将此前 4.13 秒阶段结果外推；busy production step/NPC 两项仍为 `false`，整个 Q22 继续开放。
- 最终 `final-build-3` 的真实 Engine artifact 已编译正式 producer；generator 仅新增 `session.shared_ingress()`，未改 setup/seed/NPC 或人工构造 receipt/cursor。两日 tick 120 样本含 26 NPC、5 公司、pending Player 0，GameSession save→restore→resave 严格相等，公司 slice 原样投影。Web 存档短测 42/42（3.36 秒）通过，日志 `.tmp/checklist-wave3/shared-fixture-compile.log` / `shared-fixture-generation.log` / `shared-fixture-web-final.log`。样本保留 1 条真实 pending NPC，属于低层 GameSession 可恢复完整样本，不冒称 Protocol 公共无活动委托 archive；非作者仅审结构/来源边界，不冒称全字节审计。

## 更强 Browser 验收的后续设计

若继续补 production step 忙碌证据，应使用 `verification-harness` 的每会话、非持久化控制：在实际 NPC worker 的 `before_decide` 路径设置受控 entered/release 握手，在 worker 真正进入本轮决策且 owner 正运行 production step 时，由独立 intake 登记 Player，再释放 worker；只读 receipt 观察应来自同一个 receiver 的真实分配，不由 JS timestamp 或返回后补盖推导。控制不得持有入口 metadata 锁、现金/股份 authority 或改变 production 决策排序，正常构建不包含此阻塞控制。fixture 仍应保持十秒 deadline，失败必须释放所有 worker 并保留原始日志。这是未实施的后续验证设计，不是当前验收证据。
