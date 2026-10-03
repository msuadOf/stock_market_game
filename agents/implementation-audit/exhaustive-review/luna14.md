# Luna14：测试与错误文档复核

复核基线：产品 `08e4fc7`，审计 merge `a7c7ce357bdc9f88c03633744b2d5815db49e9b2`。当前 HEAD 与指定审计 merge 相符。已读 `AGENTS.md`、`docs/principles.md`，并逐篇从首行连续读至 EOF：`docs/testing.md` 214 行、`docs/test-cleanup-checklist.md` 116 行、`docs/error-handling.md` 137 行，共 467 行。初次多文件合并输出遭截断，随后三篇均分别完整读取；本记录仅审计指定范围及其现行调用者，不运行测试/构建/长验收，不改产品或 Git 状态。

## 逐章矩阵

| 章节 | 原文行 | 当前契约/代码追踪与判断 |
|---|---:|---|
| testing §1 TDD、红灯三问 | 8–37 | 过程规范；未以缺失历史红灯日志推断实现缺失。 |
| testing §2 金字塔 | 39–53 | Unit、integration、browser E2E 层均有套件；比例及 engine 覆盖率目标不能仅由文档/本次静态审计核验。 |
| testing §3 层级、正式命令、类型生成 | 55–87 | engine/server/desktop Rust、Web Node 与 Playwright 入口存在；根命令与 types generate/check 有脚本接线。ADR-0019 对单局草稿范围的限制仍优先，不恢复多局压力测试。 |
| testing §3 普通时限、Web发现与分片 | 89–101 | `scripts/run-web-tests.mjs` 递归发现、去重拒空、CPU分片并有每case/整批门限；`scripts/run-with-deadline.mjs` 提供进程树 deadline。硬限与多核约束有实现承载。 |
| testing §3 长验收、full regression、K7 | 99–120 | 根 runner 的 build/execute 分阶段、CPU预算、外部门限存在；`scripts/simulation/baseline-run.mjs:1426` 拒绝 `before`。历史 `before` 工具已经退役，不能要求复活；未运行 K7 after/sensitivity 是本轮验证边界，不是实现缺口。 |
| testing §3 CI/缓存/类型/Windows | 122–158 | 手动 CI 保留开发验收；ADR-0028 明确产品发布 build-only，发布成功不代表游戏回归通过。文中旧式“CI 必须全绿才能合并”不能覆盖新决定而要求发布链重加测试。 |
| testing §4 必测 | 160–166 | 规则、错误路径及不变量已有多个生产测试族；本次没有运行测试，故不报告测试通过或覆盖率。错误 producer 接线候选见下节。 |
| testing §5–6 少测/测试质量 | 168–182 | 规范性质量要求，不据此制造缺口。 |
| testing §7 诚实反馈、并发重放 | 184–207 | 重放约束区别固定受理事实与自由调度；与 ADR-0017/0018 的实际受理语义一致，不复活旧的全局字节同一要求。手动验收与产品发布契约需按现行 ADR-0028 分开解释。 |
| testing §8 质量工作 | 209–214 | 覆盖率门槛、远程/Tauri E2E 仍是明确待办；不能当作新发现，也未以 SSR 冒充浏览器验收。 |
| cleanup 执行约束 | 6–14 | 属历史清理流程、范围及状态说明；不可把旧“先修改后统一编译”或未填记录升级为当前产品缺陷。 |
| cleanup 01–03 | 18–31 | 伪宿主自比较、测试辅助验证、旧新路径自比较均已退役；本次不要求恢复已删除测试或入口。 |
| cleanup 04–06 | 33–46 | 重复建局/健康检查测试和固定 files 数门槛已清；条目声称的替代覆盖须按历史范围理解。 |
| cleanup 07–09 | 48–61 | 同步仓储/裸 JSON 兼容已删；`before` 明确拒绝；旧重放哈希证明不属于当前门禁。 |
| cleanup 10 | 63–74 | 显示行为迁至 SSR/数据/回调测试；文档明确真实浏览器 E2E 尚待，不将其状态写成已通过。字段级交易输入反馈沿用 G22。 |
| cleanup 11 | 76–79 | 大型旧存档改为 parser 投影 fixture，文档明确悬空引用、不保证 Rust 整档恢复；不是恢复旧档作为当前有效存档证据。 |
| cleanup 12 | 81–91 | 明示某些格式 helper 仅测试调用、历史 bundle 不再能执行复验且用户接受；不能以“没有生产调用者”自动判作错误。 |
| cleanup 13 | 93–98 | K7 CLI 短测修复限于输出捕获；真实 after/sensitivity 未执行，文档对此诚实标界。 |
| cleanup 独立发现、批次记录 | 100–116 | scripts 范围归 Q05；历史计数/日志不是当前 head 新验收结果。 |
| error §0–1 立场与红线 | 8–48 | 宿主 fatal UI/错误反馈已存在；异步 dispose 和 React 子树异常仍有窄化缺口，见 C14-1/2。 |
| error §2 Result、上下文及 UI | 50–85 | `HostFailure`、`FatalHostError` 与可预期业务拒单 notice 存在；具体协议 producer 上下文不足，见 C14-3。 |
| error §3 fallback | 87–96 | 未发现可独立成立的新 fallback 候选。取消、无档、过期异步世代属于显式生命周期分支。 |
| error §4 展示契约 | 98–112 | `apps/web/src/app/error-details.ts:86-122` 构造可复制详情并呈现复制失败；展示容器已实现不代表每种 producer 已接入。 |
| error §5 外部输入 | 114–125 | schema/网络/启动边界有校验；交易输入只给全局 notice、无字段级关联为 G22，非全局“完全不校验”。 |
| error §6–7 不变量、总结 | 127–137 | 总则成立；协议 cursor producer 未带 actual/expected 是 C14-3。不可据文档要求抛错后进程必崩，关键是可诊断地显式展示。 |

## 旧结论复核

### G22：字段级交易表单反馈仍成立

`apps/web/src/App.tsx:565-566` 的价格与数量是 `InputGroup`，没有 `aria-invalid`、字段关联错误或字段级提示。`apps/web/src/app/useTradingCommands.ts:58-71,74-83` 通过 `parseShareQuantity`、`validateAShareQuantity`、`buildPlayerOrderIntent` 校验；校验与提交错误均进入全局 `setNotice`。因此不能说错误被吞或缺少校验，旧结论应维持窄定义：错误虽可见，未定位到对应字段，也无即时字段状态。属于 UX/可访问性反馈缺口，不改 A 股规则。

### Q05：已有递归测试 runner，但持续入口仍未收口；受新决策约束

`agents/oop-release-validation/run-scripts-tests.mjs:14-26,28-47` 递归发现全部 `scripts/**/*.test.mjs`，排序并行运行；每文件 Node case 和进程树均 10000ms。`agents/oop-release-validation/scripts-supervised-results.md:1-5` 是历史 32/32、13.675 秒记录，不能作为本轮执行结果。此前 Q05“没有自动发现入口”错误，修正为：独立任务 runner 已有，根测试/手动 CI 是否调用它的持续策略未收口。ADR-0028（`docs/decisions/0028-tagged-release-and-static-pages.md:84-90`）明确发布链不运行契约测试；因此不能把 Q05 变成产品 release 测试门禁。任务目录 runner 可供独立开发诊断，不代表应接入构建或发布。

## 新候选及反证

### C14-1：React 异步渲染/组件树异常无用户可见错误边界

`apps/web/src/render-app.tsx:7-14` 直接 `createRoot(root).render(...)`，当前入口没有 React Error Boundary；`apps/web/src/main.tsx` 的启动 Promise catch 只覆盖异步导入/启动，不会捕获随后由 React 调度的 render/effect 异常。生产组件确有显式 invariant throw，例如 `apps/web/src/app/WorkspaceGrid.tsx:36,40,42,49`。全局错误回调/ErrorBoundary 检索未见等价捕获接线。契约应限于非预期 React 树异常能进入诊断/用户反馈，不要求业务错误都崩溃。

反证：`apps/web/src/app/HostStatusViews.tsx` 的 `FatalHostError` 与 `App.tsx:416` 已消费宿主 fatal，`error-details.ts` 也可复制详情；这些路径不捕获组件树自身 render/effect throw。因此候选成立，尚需根任务去重/接受，不代表普通状态必然触发。

### C14-2：Tauri dispose 的 stop IPC rejection 丢失

`apps/web/src/host/tauri-host.ts:165-175` 清空 `fatalCallback` 后以 `void invoke("stop_session", ...)` 发起 IPC，没有 rejection handler；owner `apps/web/src/app/useSessionHostLifecycle.ts:75-77` 同步调用 `dispose()`，无法 await/catch。相较 `tauri-host.ts:163,180` 的 pause/speed catch→fail，此路径没有用户错误出口。候选限于 `stop_session` rejection；不据同步接口推断其他同步清理必然失败。

反证：暂停和速度调用存在显式错误回调，说明一般 Tauri 错误路径已经接好，但不能覆盖 dispose 分支。应报告清理失败，不要求重试或恢复已释放 host。此问题与 C14-1 的全局兜底有关，但即使添加全局未处理 rejection handler，也需确认报告在 fatal callback 清除后的生命周期可达。

### C14-3：ProtocolError 丢失 cursor/generation 的 expected/actual

`apps/web/src/host/protocol/reduce.ts:37-40` 知道当前及 incoming generation 却只发通用消息；`:50-54` 知道 `expectedTick`、`range.firstTick`、`state.cursor.seq`、`range.from` 却只报告“不连续”。`apps/web/src/host/protocol-coordinator.ts:104-107` 转换 `ProtocolError` 时仅传 code/where/message，复制详情无法复现游标数值。建议限定为脱敏、有限的 expected/actual 诊断，不含 raw update/凭据。

反证：`apps/web/src/host/protocol-failure.ts` 支持上下文，`apps/web/src/app/error-details.ts` allowlist 接受 tick/seq/generation/expected/actual；engine fatal 和 server error 可传更充分上下文。因此不是所有 host error 都缺上下文，实际缺陷是前端自产 cursor/generation 断言没有填入可用字段。

## 结论边界

未运行测试、构建、浏览器或 K7 验收，不报告任何新通过结果。未改 A 股交易语义、代码或 Git 状态。G22 窄义仍成立；Q05 改为“已有任务 runner、持续入口未定”，且不得违反 ADR-0028 的 build-only 发布决定。C14-1～3 是源码可定位的新错误展示候选，应由父任务与总台账去重、独立复核后决定登记。
