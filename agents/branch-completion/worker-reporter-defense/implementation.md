# Worker 错误报告器异常保护

## 范围与依据

基线：本地 `main` 的 `35ac58a1a18cdea67c953758381bfff677404045`。本批只修 Web Worker 错误消息边界，不改变 engine、交易制度、价格、金额、股份或存档语义；领域层依据仍为现行正式文档，本批无新增 A 股规则。

历史来源为冻结提交 `df994d3f183af0beac6ac4fabcbbbda60e59c4cc`，四文件原 diff 原样保存于 `source-df994d3f.diff.gz`（gzip 解压后与 Git 原 diff 字节一致）。只恢复当前 production 边界必要保护；不恢复旧 `REPORT_CORRECTION_REJECTED` 特判、B4f 或其他未接回功能。协议依据为 ADR-0010；`parseHostFailure` 保持原严格校验。

## 输入与可见结果

- 合法核心字段，`where/context` getter、Proxy own-property 陷阱或详情 clone 抛错：发送 `WASM_WORKER_PROTOCOL`，消息明确阶段和字段，`cause` 保留已读 clone-safe 原故障快照，消息保留原 `code/message`。
- `message` getter 抛错：消息注明 `message` 访问失败，`cause` 保留已读 primitive `code`，不重复触发 getter。
- 标准 Error、非枚举 `code/message/cause`：保持原可见字段。核心分类及直接详情各首读一次；非结构化说明复用分类结果。Proxy get trap 同样使用首读快照。
- `recoveryActions`：先 clone 稳定快照，再严格 parser；不能用先前 getter 值为另一次读取背书。非法值仍明确报协议错误。
- `postMessage` 首次失败：再送包含原故障快照的协议错误。两次失败：通过 `reportError(Error)` 触发既有 `worker-host` 的 Worker error 用户回调；没有该 API 时在独立 `queueMicrotask` 抛出，避免 async listener 把错误变成无人消费的 rejected Promise。文本同时保留原故障与两次发送原因。
- `operationError` 发送失败：通过同一受保护 failure 通道暴露原请求失败和发送原因。
- 标准 Date/URL 等原生标量先由 intrinsic 取不可变值；其他非结构化对象使用保留 prototype 的 descriptor 投影，替换已读核心，保持普通 `toJSON` 诊断。自定义 `toJSON` 失败会明确展示失败和已读原核心。

本批不承诺任意循环对象图所有 getter 只执行一次。`context` 回指原始 error 时，平台 `structuredClone` 可再次遍历原 getter；若失败，会明确显示 `克隆结构化错误字段 context 失败` 并保留首次原核心。已有真实反例测试固定这一边界，不实现通用 clone 框架。自定义闭包主动回访外部原对象也不属于直接字段读取保证。

## TDD 与验证原件

普通命令均由 `run-with-deadline.mjs 10000` 监督，Node case 均设置 `--test-timeout=10000`。三个定向文件使用 `--test-concurrency=3`，包括 `wasm-worker-failure.test.ts`、`worker-host.test.ts`、`protocol-failure.test.ts`。

- `evidence/red.log`：首次 getter/Proxy/parser/发送异常及 listener 消息丢失红灯。
- `evidence/red-2.log`：`operationError` 发送失败红灯。
- `evidence/red-3.log`：独立复核发现的首读核心丢失、非结构化重复读取红灯。
- `evidence/red-4.log`：非结构化 Proxy get trap 重读红灯。
- `evidence/red-5.log`：原生 Date 的 own custom `toJSON` 重读红灯。
- `evidence/green-final-66.log`：最终 66/66 通过，无跳过，Node 报告约 372ms。
- `evidence/types-final-66.log`：`tsc --project apps/web/tsconfig.app.json --incremental false` 通过；使用 `run-long-validation.mjs 300000` 外监督，与窄 lint/browser 独立并发。类型检查复用已有 WASM bindings 声明，未重建 engine/WASM。
- `evidence/lint-final-66.log`：四个改动文件 `oxlint --deny-warnings` 通过。
- `evidence/browser-final-66.log`：Chromium 153.0.8010.12 同时验证实际产品源码经 Vite development transform 的 listener 的 `reportError` 有/无两条路径，都收到完整 Worker error；`browser-check.mjs` 为原件，可从仓库根运行 `node scripts/run-with-deadline.mjs 10000 -- node agents/branch-completion/worker-reporter-defense/browser-check.mjs`。使用两个并发 Worker，`launchServer` 与显式 `kill()` 完成收尾。

保留负结果：前三次 browser fixture 已获得正确错误事件，但 `browser.close()` 卡住，十秒 supervisor 终止，因此这些运行不算通过；对应 `browser-report-error*.log` 保留。改用 `launchServer/kill` 后完整退出成功。`types-final.log` 的 TS2366 失败原件也保留，修正穷尽分支后 `types-final-66.log` 通过。

原先两个测试要求 reporter 对非法可选字段/getter 继续抛错且消息为空。该行为正是本次要修的缺口；现在保留非法 fixture 与拒绝原因，并加强为完整可见 failure 消息及原故障 `cause` 断言，未放宽 parser。非结构化 listener getter 用受控第二次 generation 返回值模拟故障注入，避免发送 operationError 信封时测试自身再次抛同一输入 getter，混入另一个发送故障。

独立复核由 root 派发未实施 subagent；本记录只表示实现与定向验证已冻结，最终通过以独立复核记录为准。未运行完整回归、未合并 main、未推送。

## 复核修正与有限阶段表

- `red.log` 原件为 50 测试中 43 绿、7 红，另有 1 次异步 unhandledRejection；不是 8 个断言全红。
- prototype.toJSON/Date/URL 与空 message 原 code 的新增回归用例是复核修复后的验证，没有单独保存先红原件，不将其冒充完整 TDD 红绿循环。Proxy、自定义 Date.toJSON、双发送原 origin 和原型检查失败均另外有 `red-4/5/6.log` 先红原件。
- `red-6.log` 固定原 `where` 在双发送或先详情降级后双发送丢失，以及 prototype 检查失败覆盖首读 message 两个缺口。修复后统一用 `describeReadFailureFields` 汇总首读 primitive 诊断，所有失败阶段不得回读原核心。
- `cause` 保留 schema 合法核心和已读 primitive 诊断；不承诺任意畸形复杂 `code/message` 的全保真，不为它们构建通用 snapshot。合法复杂 `cause/context` 使用平台 structured clone。

| 失败阶段 | 可见结果及原故障保留 |
| --- | --- |
| classification 的 code/message 访问 | 明确失败字段与异常原因，`cause` 和错误文本保留已读 primitive 核心 |
| 非结构化 inspection/serialization | 明确检查/序列化失败，追加同一已读核心说明；不再读源 getter |
| 直接详情 getter/own-property 陷阱 | 明确字段/阶段，`cause` 保留此前稳定快照，文本保留原核心和已读 where |
| clone/parser | 明确 clone 字段或 parser 校验原因；快照通过严格消费，失败不会伪装成功 |
| 首次 postMessage | 第二条消息显式记录发送原因，`cause` 保留完整已校验原故障 |
| 双 postMessage | Worker error 文本包含原故障 code/message/where 及两次发送原因；原快照继续挂在 Error.cause |

Chromium 只验证实际产品源码经 Vite development transform 的 listener，不是 production build 或产品 WASM 整体验收。与 worker-host 的 error 事件到用户 callback 单测一起构成本次消息边界证据。
