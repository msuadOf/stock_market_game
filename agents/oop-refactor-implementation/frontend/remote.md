# Remote frontend OOP 实施记录

日期：2026-10-03。范围：`frontend-N01`、`frontend-N07`、`frontend-R2-N02`。

已读取仓库 `AGENTS.md`、`docs/principles.md`、`docs/testing.md`、`docs/architecture.md`、
`docs/open-questions.md`、ADR-0004、ADR-0010 和 ADR-0018 的时间线、公开事实及命令确认相关段落；
改动前完整读取 `remote-host.ts`、相关 Remote 测试、`remote-request.ts`、`remote-wire.ts`。
本次只提取 Web Remote adapter 的既有所有权，不修改 A 股交易制度、账户/委托单位、公开报告 schema、
Rust 权威状态、`EngineHost` 或 Redux 契约；领域依据沿用 ADR-0010 的完整 baseline/protocol 和
`CommandQueued` 只表示入队契约，无新交易规则需要引入。

## frontend-N01：ReportQueryContext

- 唯一 owner：`createRemoteHost` 持有一个 `ReportQueryContext`；原 `reportQueryEpoch` 与 `reportCompanies` 已从 facade 删除。
- 方法：`captureEpoch`、`invalidate`、`assertCurrent`、`acceptPage`、`companyFor`、`validateReport`。
- 生产 caller：`queryPublicReports` 使用捕获 epoch 后的 `acceptPage`；`publicReportById` 由 `companyFor` 构造 company 路由，响应由 `validateReport` 校验；跨 generation baseline 和成功 load 调用 `invalidate`。
- 索引登记继续先验证整页后写入；opaque decimal ID 保持 string，不转换 number、不从 baseline public IDs 猜测归属。
- 测试：原 Remote 页登记、跨公司冲突、未知 ID、by-ID 错配和 load 后旧 ID 拒绝；新增实际 adapter 的 page/by-ID 晚到响应跨 generation 失效；owner 测试验证拒绝有效前缀、精确 ID、失效清索引、新时间线重用 ID。
- App → `CompanyQueryCoordinator` → `EngineHost` 查询接口保持既有调用；未增加 Worker/Tauri 映射或第二 authority。

## frontend-N07：RemoteCommandRegistry

- 唯一 owner：`createRemoteHost` 持有一个 `RemoteCommandRegistry`；原 `requestSequence` 和 `pendingCommands` 已删除。
- 方法：`next`、`register`、`resolveQueued`、`rejectGateway`、`rejectAll`。
- 生产 caller：`submitIntent` 保持预检 → 分配 ID → 登记 waiter → socket.send 的顺序；message queued/gateway-error 分支分别委托单次完成；`fail` 委托批拒绝。
- 测试：实际 adapter 并发逆序确认、重复确认显式错误、单 request gateway 拒绝、HostFailure 拒绝其余命令；owner 测试验证完成/拒绝前删除登记、未知/重复 ID、批拒绝清表且 sequence 不重置。
- 当前 send 同步 throw 后登记仍留存；dispose 不主动 reject pending command。特征测试明确保留这些现状，未把提取宣传成缺陷修复。

## frontend-R2-N02：RemotePublisherState

- 唯一 owner：`createRemoteHost` 持有一个 `RemotePublisherState`；socket、connectionGeneration、awaitingBaseline、baselineWaiter、cachedBaseline、baselineEpoch 原字段已删除。
- 方法：socket attach/detach/identity/conditional clear；baseline read/install/gating；单槽 waiter begin/take/resolve/reject；query cursor capture/assert。
- 生产 caller：start/connect、fail、message baseline/protocol/resync、mode 切换、submitIntent 预检、load 新连接/重同步、save/snapshot/tick/day、pause preferences、player orders、NPC diagnostics。
- socket.send/close、socketFactory、HTTP、parser/switch 和用户 callback 仍由 adapter 编排；load 的 timer 留在移交给 owner 的 waiter 闭包中，resolve/reject 继续 clearTimeout。
- baseline 顺序仍为 baseline epoch 增加 → 跨 generation report context invalidate → cache 安装/清 awaiting → 用户 callback → resolve waiter/清槽。
- fail 仍先取清 socket/waiter，再 reject waiter、close、reject command、fatal callback；不清 baseline cache，不重写 awaiting。
- mode 切换仍先增加 identity，再 close/detach/reconnect；不新增 awaiting 写入。onmessage/onclose 继续 identity guard；旧 socket onerror 按现状仍直接 fail。
- requestResync 仍先写 awaiting/send 后登记 waiter，不新增 timeout/排队；单槽覆盖不主动 settle 旧 waiter；load 新连接仍唯一使用 5000ms timer。
- orders/diagnostics 继续 pin generation + baseline epoch；save 只 pin generation 并检查 awaiting/disposed，未统一增强检查条件。
- 测试：实际 adapter 旧连接 message/close 丢弃、旧 onerror fail 新连接、cache 保留、mode 后同 generation protocol 接受、单槽覆盖、callback 抛错、send 同步 throw、generation mismatch/ResyncRequired、恢复 timer 成功/超时、各 query guard；owner 验证 socket identity、timeline invalidate 的次序、epoch、重入 waiter 清槽顺序。

## 验证与 TDD 记录

这是行为保持的重构：实现前先新增并运行原 adapter 的特征测试；它们在原实现上通过。
没有人为制造功能红灯，也没有将“原实现通过”说成失败测试。提取之后新增 owner 行为测试验证窄对象不变量。

Node 实际版本 `v25.8.2`；运行前已完整读取 `scripts/run-with-deadline.mjs`。
精确测试使用 `runWebTestBatch` 注入固定文件清单，未递归启动全 Web 回归：

```sh
node scripts/run-with-deadline.mjs 10000 -- node --input-type=module -e 'import path from "node:path"; import { runWebTestBatch } from "./scripts/run-web-tests.mjs"; const files = ["remote-host", "remote-company-reports", "remote-state-contract", "remote-command-registry", "report-query-context", "remote-publisher-state", "remote-startup", "remote-host-token"].map(name => path.resolve("apps/web/src/host", `${name}.test.ts`)); await runWebTestBatch({ discover: async () => files, cpuCount: 8, log: console.log });'
```

- 8 文件、8 个真 Node shard、每个 case `--test-timeout=10000`、整个命令进程树 deadline 10000ms；runner 按原策略预留清理。
- 初次完整 Remote 精确批次 58 case 全通过，runner wall 861ms，最慢 shard 821ms。
- 随后新增 resync send 同步失败发生在新 waiter 登记前的特征测试；仅重跑 `remote-state-contract`/`remote-publisher-state` 两 shard，16 case 全通过、wall 312ms。当前精确范围共 59 case（其余文件未再改动）。
- 指定 9 个改动 TypeScript 文件的 oxlint，显式 `--threads=4`，通过。最后新增测试沿用既有风格，无生产代码变化。
- 未运行全回归、build、tsc、E2E；由总协调者安排跨分组验证。没有 Git 写操作、提交或 push。

## 未完成与独立复核

三个动作的生产 caller 与字段迁移无待实施项。独立 subagent 全 diff 复核尚待总协调者安排，
在复核与总协调验证前不宣称整批 frontend 完成。
本记录不把旧 onerror、单槽 waiter 覆盖、dispose pending、同步 send 留登记和 fail cache 保留认定为已修复。

## 总协调类型检查后的测试 fixture 修正

总协调者 app tsc 检查指出两个新增测试文件存在类型错误，已补齐：
`remote-command-registry.test.ts` 的三个 fake `HostFailure` 增加必需的 `where`；
`remote-state-contract.test.ts` 在 mode 相关调用前显式 `assert.ok` 断言可选能力存在并完成 narrowing。
没有改 production 契约或弱化行为断言。
修正后仅重跑这两个文件，2 个真 Node shard、case/整命令进程树 10000ms，15 case 全通过、wall 305ms；
两个文件 oxlint 显式 `--threads=2` 通过。tsc 再次检查由总协调者执行。
