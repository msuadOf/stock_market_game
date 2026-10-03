# Sweep 14：测试策略、测试清理与错误展示全文复核

审查日期：2026-10-03。工作树 HEAD 为 `4ad5a2e298d086024e6b0069b98b4a6b195a0a01`，按任务指定核对产品基线 `b76ece3`（HEAD 为审计 merge）。仅新增本工作记录，未修改产品、未执行 Git 写操作、未执行编译或长测试。

已阅读根 `AGENTS.md`、`docs/principles.md`，并参考 `docs/architecture.md`、`docs/open-questions.md`、ADR-0019 的当前范围。指定三篇正文连续全文读取：`docs/testing.md` 214 行、`docs/test-cleanup-checklist.md` 116 行、`docs/error-handling.md` 137 行，共 467 行。以下行号均指当前工作树。

## 全文章节覆盖

| 文档章节 | 原文位置 | 当前代码核对与状态 |
|---|---|---|
| testing §1 红绿重构、红灯三问 | 8、33 | 工作流程约束，不根据缺少旧日志猜测未实现代码。 |
| testing §2 测试金字塔 | 39 | Unit/Integration/E2E 均存在；比例为目标，不能无覆盖率实测断言已满足或未满足。 |
| testing §3 各层契约 | 55 | engine/server/desktop Rust 套件、Web Node、Playwright 已有；ADR-0019 不要求恢复多局压力测试。 |
| testing §3 正式命令与类型生成 | 69 | `package.json:15` 接 `run-full-regression.mjs`；`types:generate/types:check` 已有命令；未运行不等于未实现。 |
| testing §3 时限、发现、分片、冷编译与 execute | 89 | `run-web-tests.mjs:27` 递归发现、`:58` 非空/重复拒绝、`:63` 最多 8 shard、`:114` case timeout、`:146` 整批进程外监督；`run-full-regression.mjs:300` CPU 预算、`:328` 并行 binary、`:626` 独立阶段监督。主干已实现。 |
| testing §3 K7/历史 before | 118 | `baseline-run.mjs:1426` 明确拒绝 before；不能恢复已退役工具。未再次运行 after/sensitivity 是验收边界。 |
| testing §3 CI 构建、缓存、类型、Windows | 122 | `.github/workflows/ci.yml` 仅 workflow_dispatch，Windows 原生库预编译、密封 build/execute、执行后类型检查均有步骤。产品发布 build-only 是更新决定，§7 旧合并全绿句子不是恢复发布测试门禁的依据。 |
| testing §4 必测范围 | 160 | 交易规则/异常/不变量现有套件；不以本审计未运行全测试宣称测试不存在。新增错误生产缺口见下文 C14-1～3。 |
| testing §5 少测范围 | 168 | 规范性边界，无新增功能要求。 |
| testing §6 测试质量 | 176 | AAA、隔离等是质量规范，不将任意命名/魔法数字升级为产品缺口。 |
| testing §7 诚实反馈 | 184 | 手动诊断保留全回归；产品 build-only 优先，旧 CI 必须绿方可合并表述按新决定解释。 |
| testing §7 并发受理与重放 | 190 | Rust 当前正式套件保留实际受理、固定调度重放、恢复；旧跨自由调度字节比较不恢复。 |
| testing §8 质量待办 | 209 | 覆盖率门槛、远程/Tauri 浏览器 E2E 为原有明确待办；不作为新发现。已有本地 WASM E2E 文件。 |
| cleanup 执行约束 | 6 | 纯工作流程/历史协作记录；未把未填验证记录当未实现功能。 |
| cleanup 01～03 | 18、23、28 | 伪宿主同源比较、旧辅助参数测试、两入口自比较已退役；搜索未发现旧入口复活。 |
| cleanup 04～06 | 33、38、43 | 重复建局/health test 名及固定 files.length >= 54 已不在当前源码；baseline example 属单独定向测试，不能把其不在正式 inventory 当清理遗漏。 |
| cleanup 07～09 | 48、53、58 | 当前 `save-repository.ts:59` 仅压缩异步仓储，`:89` 拒绝缺前缀；before CLI 拒绝已实现；旧重放哈希证明不恢复。 |
| cleanup 10 | 63 | HostStatusViews、KlineViewportControls、volume-histogram 行为模块及测试存在；E2E 尚未运行属验收记录，G22 已列字段反馈，不能重复编号。 |
| cleanup 11 | 76 | 旧 mature-save-test-fixture 已继续被后续清理替代，不按历史旧文件名要求恢复；无 archived-schema-save.json.gz 测试动态加载引用。 |
| cleanup 12 | 81 | 原文 87～88 明确 KEEP helper 可仅测试调用、历史 bundle 不再能执行复验；旧 CorpusProjection/verifyEvidenceBundle 等已不在源码，不能登记成缺少生产接线。 |
| cleanup 13 | 93 | `verify-simulation-artifacts.mjs:548,553` 实际使用 writeSync；未跑真实 K7 不等于 CLI 修复未实现。 |
| cleanup 单独发现 | 100 | scripts test 正式发现范围归既有 Q05，未重复登记。 |
| cleanup 批次记录 | 104 | 历史数量、日志、待 E2E 均限于原批次，不冒充当前 head 验收。 |
| error §0 核心立场 | 8 | 大部分宿主错误已进入 FatalHostError，前端渲染异常仍有缺口 C14-1。 |
| error §1 红线 | 17 | 现行生产 catch 大多显式报告；Tauri dispose 未处理 rejection 见 C14-2。`wasm-failure.ts:6` 在 String 本身抛错时返回“无法读取错误详情”是显式展示异常，不算业务静默 fallback。 |
| error §2 上下文、UI、Result | 50 | HostFailure/parseHostFailure/FatalHostError 均已支持上下文；producer 缺口 C14-3，不再声称 UI 没有复制或原因链。业务拒单有结果及 notice。 |
| error §3 fallback | 87 | 没有新增广泛 fallback 候选；不存在的存档、已取消选择、旧异步世代失效都属于明确生命周期分支，不算吞错。 |
| error §4 错误展示契约 | 98 | `error-details.ts:86` 输出 code/where/message/cause/context/recovery，`:106` 复制状态及失败提示；契约容器已实现，producer 仍不全。 |
| error §5 外部输入 | 114 | schema、网络协议、启动配置校验存在；字段级即时反馈是既有 G22。 |
| error §6 不变量 | 127 | engine fatal producer 与宿主 failure 展示已有；协议断言实际/期望值遗漏见 C14-3。 |
| error §7 总结 | 134 | 汇总上述契约，不重复新增任务。 |

## 新候选与反证

### C14-1：React render/effect 异常未接用户错误页

原文 `docs/error-handling.md:68` 要求 UI 显式渲染错误，`:69` 要求位置、动作与反馈细节；`:131` 要求不变量立即报错并展示。

生产入口 `apps/web/src/render-app.tsx:8` 直接 `createRoot(root).render(...)`，没有 root 错误回调或 ErrorBoundary。全源码搜索 `ErrorBoundary/componentDidCatch/getDerivedStateFromError/onUncaughtError/unhandledrejection` 无匹配。`apps/web/src/main.tsx:24` 的 start Promise catch 只覆盖动态 import/调用启动，React 调度的后续 render/effect 异常不通过该 Promise。

明确 producer 例子：`apps/web/src/app/WorkspaceGrid.tsx:36,40,42,49` 在 render 中对错误面板 ID、重复面板、非法 width 抛异常；`MarketRuntimeProvider.tsx:97,103,109` 对缺 Provider 抛异常；`LocalRefreshViews.tsx:55` 在 effect 校验挂载。发生此类非预期 UI 错误时，没有与 `FatalHostError` 等价的用户反馈路径。应覆盖“实际 React render/effect 抛错后能见错误详情”，不只测试错误页独立 SSR。

反证：`HostStatusViews.tsx:21` 的 FatalHostError 已有 alert/重试/复制；`App.tsx:416` 消费宿主状态 error。这证明宿主报错完成，不能覆盖组件树在渲染自身时抛异常。此候选不宣称当前正常 fixture 必然触发。

### C14-2：Tauri dispose cleanup Promise rejection 没有错误出口

原文 `docs/error-handling.md:19,43,68` 禁止错误被吞或只留开发者可见；所有异常须显式到达用户。

`apps/web/src/host/tauri-host.ts:165` 的 dispose 把 `fatalCallback` 清空（`:171`），然后 `void invoke("stop_session", ...)`（`:175`），该 IPC Promise 无 catch、无返回给 owner 的 Promise。owner `apps/web/src/app/useSessionHostLifecycle.ts:77` 仅同步调用 `host?.dispose()`，不能捕获其后异步 rejection。没有 window unhandledrejection UI 兜底（C14-1 搜索结果）。故 IPC 停局失败只产生未处理 rejection，用户无动作/诊断。此候选确定范围收窄为 `stop_session`；`:173–174` 的两个 unlisten 静态类型为 `() => void`，不能仅凭该类型声称必有异步 rejection。父任务独立复核另确认本地 `@tauri-apps/api` 2.11.1 的 `event.js:81` 实际返回 async unlisten；此 runtime 依赖差异留作版本限定观察，不作为本候选成立的必要依据。

反证：同文件 `:163` pause、`:180` speed 的 catch→fail 已存在，但 dispose 清 callback 且没有 catch，不能兜底。`dispose` 接口同步签名不意味着可以放弃内部异步错误报告。候选只要求 cleanup 失败可见，不要求恢复旧 host、自动重试或删除存档。

### C14-3：协议 cursor 错误 producer 未提供可复现的实际/期望值

原文 `docs/error-handling.md:55` 要求输入/位置/原因上下文，`:108` 要求 context，`:111` 要求足够复现反馈，`:132` 要求断言实际值和期望值。

`apps/web/src/host/protocol/reduce.ts:37` 明知当前/传入 generation，却只抛“更新代际与当前状态不匹配”；`:50` 明知 expectedTick、range.firstTick、state.cursor.seq、range.from，却只抛“更新起始 tick 或 seq 游标不连续”。`protocol/types.ts:93` 的 ProtocolError 仅 code/where/message；`protocol-coordinator.ts:106` 转 HostFailure 时仍仅传这三项。实际生产收到错序/跨代消息后，复制反馈中的上下文为“未提供”，无法知道需要重现的数值。最小需求是保留公开 cursor expected/actual 诊断及合理恢复建议，不含完整 raw 消息、存档或 credential。

反证：`host/protocol-failure.ts:29` 能传 cause/context，`app/error-details.ts:12` allowlist 已接受 tick/seq/generation/expected/actual；server `actor.rs:1223` failure context 包含 tick/seq/day/generation；WASM `wasm-worker.ts:48` 透传结构化 fatal。这些路径说明完整 engine fatal 并非缺少上下文，缺口应收窄到前端自产 ProtocolError，不能把全部三宿主错误当空壳。

其他观察：remote-request、tauri-host、save-repository 多处转换 Error.message 丢原 cause，非 fatal 操作也仅 notice。但其 message 常已含具体原因；本轮不把所有 message-only 报错一概升级新缺口，优先记录上方可确定损失 actual/expected 的协议断言。

## 排除与验收边界

G22 表单即时/字段级错误与 Q05 scripts test 持续入口均已在主台账，未重复。覆盖率和远程/Tauri E2E 仍为现行已声明质量待办。未恢复 retired corpus/before、未要求产品发布运行测试、未将“未跑某个平台”当作代码未实现。本轮依据源码审查，没有新运行的测试结果。未改变 A 股单位、受理时序、T+1、交易阶段与存档语义；三个候选是现行显式错误契约的生产接线边界，需父任务独立去重及复核后登记。
