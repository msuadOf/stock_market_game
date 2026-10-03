# frontend 批次执行门禁

全部 16 个动作与 3 个具名扩展已实施。逐条需求对应真实 owner/method/caller/文件见 [completion.json](completion.json)；分片行为命令和实际结果在 [app.md](app.md)、[chart.md](chart.md)、[host.md](host.md)、[mobile.md](mobile.md)、[remote.md](remote.md)、[wasm.md](wasm.md)、[request-owners.md](request-owners.md)。

未实施 reviewer 独立全文审查 70 个改动代码/测试/原型文件，含全部未跟踪新文件，并审查已跟踪文件相对 baseline 的完整 diff。三门结论通过，16 个动作、3 个扩展及 39 条需求均已核销；FR01 的指标系列删除失败部分状态和 FR02 的台账 caller 分类均已修复并复核关闭，无未解决发现。最终签名、每文件 SHA-256 及 completion/verification 文档指纹见 [review.md](review.md)。

## TypeScript

root 授权在 production 稳定后由本组检查。Node 实际版本 `v25.8.2`，三个独立 compiler 进程使用 `Promise.allSettled` 同时执行：

```sh
node scripts/run-with-deadline.mjs 10000 -- node apps/web/node_modules/typescript/bin/tsc --project apps/web/tsconfig.app.json --noEmit
node scripts/run-with-deadline.mjs 10000 -- node apps/web/node_modules/typescript/bin/tsc --project apps/web/tsconfig.node.json --noEmit
node scripts/run-with-deadline.mjs 10000 -- node apps/web/node_modules/typescript/bin/tsc --project apps/web/tsconfig.inspector.json --noEmit
```

- 首轮 node 通过，app 与 inspector 失败；app 失败是新测试 fixture 的只读档案赋值、不完整 EngineHost/PositionSnap/HostFailure、可选方法未收窄、隐式 any 与未用变量，已全部修正，断言未弱化。
- inspector 失败来自既有 `npc-decision-inspector.test.ts` 的 React `createElement` 默认 props 泛型推断。已全文读测试与组件，只在测试调用显式使用 `NonNullable<Parameters<typeof NpcDecisionInspector>[0]>`，不改组件或断言。两个精确 DEV SSR/gate 文件并发 2，通过约 0.76s；单独 inspector 检查通过约 1.53s。额外修正也纳入未实施 reviewer 全文复核。
- 最终三个 project **全部 exit 0**：app 约 5.63s，node 约 1.09s，inspector 约 1.49s；总 wall 约 5.8s，每个进程树均受 10000ms 外部监督。

## 精确 lint 与范围限制

仅对 `git diff --name-only -- apps/web` 与 `git ls-files --others --exclude-standard -- apps/web` 合并去重后的 69 个 `.ts/.tsx` 文件执行 oxlint，实际参数 `--threads=4`，进程树 deadline `10000ms`，exit 0，约 0.35s。

首轮有一项 `react-hooks/exhaustive-deps` warning：`useTradingCommands.refreshPlayerOrders` 沿用原 `[setNotice]` 依赖，而新 hook 借用的 `hostRef/playerOrderRefreshGateRef` 被 lint 视为外部依赖。原 App 中二者为本组件创建的 ref，因此原 callback 没有此 warning。这是历史结果，不是最终版本。

root 要求最小修正后，依赖明确包含这两个由 AppShell 同一次挂载固定持有的 **ref 对象**，没有依赖 `.current`、generation 或表单值。新增身份断言验证 form 更新、hostRef.current 变化与 gate invalidation 都不重建 refresh callback；实际 App 的宿主 effect 触发频率保持。未实施 reviewer 已完整审查最终生产/测试增量并复核通过，纳入最终 70 文件签名。

- 最终交易/启动两个精确 suite `--test-isolation=none --test-timeout=10000 --test-concurrency=2`，11/11 通过，约 0.46s；外置 deadline 10000ms。
- 最终 app TypeScript 检查再次 exit 0，约 6.37s；与精确 lint 并发执行。node/inspector 项目未受此次增量影响，保留前一稳定版本的通过证据。
- 最终 69 个 TS/TSX 改动文件再次 `oxlint --threads=4`，exit 0，**零 warning**，约 0.30s；外置 deadline 10000ms。
- 最终 70 文件 review manifest hash：`abe7a5142fa745d38e066cc1eda6b97448d9cdc17c986a1825027660395adeaa`。修前 group totals/首轮失败均保留在历史分片记录，不能与最后增量复测拼成重复的 case 总数。

统一台账复核发现 FR02：部分 production_callers 最初混入测试、callee/共享 owner，且将原候选“返回按钮”文案误写成真实原型 caller。已按真实代码分别归入 test_callers/observations/callees_or_related_owners；原型仅绑定 `[data-v]`、`[data-p]`、`#p`，底栏 book 按钮派发图表 book click。旧 header 返回和 quick 订单按钮没有 controller 接线，本批保留旧行为，不宣称新增功能。该文档修正没有更改源码或断言，未实施 reviewer 已逐条核对最终台账并关闭 FR02；completion 与本终稿的最终 SHA-256 由同一 reviewer 绑定在最终签名中。

`git diff --check -- apps/web design/ui/mobile/mobile-trading-concept.html` 通过。本组没有 Git 写入或提交，未执行完整回归、build、E2E、真实浏览器或三宿主完整矩阵。普通精确测试均设 10000ms case timeout 与外部进程树 deadline，并使用各分片记录的实际多进程并发。没有用本批静态复核或短测声明既有 disposal、半初始化 rollback 等独立缺陷已修复。
