# Luna 48：行情 UI 性能回归说明独立复核

日期：2026-10-03。目标产品提交 `08e4fc7`，审计工作树 merge HEAD `a7c7ce3`（merge 同）。已读根 `AGENTS.md` 与 `docs/principles.md`。连续读取并完成 EOF：`scripts/performance/README.md` 共 23 行；未按筛选片段代替全文。复核范围包括其正式入口、工具完整执行路径、相关 UI 启动选择、单元用例及 `docs/testing.md` 的 deadline 约束。仅新增本审查记录；未改产品文件、未执行 Git 写操作、未运行测试或长测。

## 条款矩阵

| README 原文位置与承诺 | 当前入口、生产调用方、实现 | 复核结论 |
|---|---|---|
| 1–3：行情 UI 最快档；用本机 Chrome/Edge CDP 驱动真实页面，不加 E2E 框架依赖 | `package.json:23` 正式命令直调 `scripts/performance/market-ui-report.mjs`；工具 `:12–19` 列浏览器候选，`:120–155` 实现 `CdpClient`，`:272–303` 启动浏览器并连 CDP。无 Playwright 或其他项目级 E2E 依赖调用。 | 入口及描述吻合。测试代码与工具存在不等于真实浏览器验证已运行；本轮未运行。 |
| 5：复用 `--url` 页面；页面没启动时自动启动 Vite | 默认 URL 在 `market-ui-report.mjs:22–26`；`:260–269` 探测后按 URL 启 Vite。`apps/web/package.json:7` 的 Vite `dev` script 未指定 mode；Vite 默认 development mode。新 Chrome 以 `about:blank` 起步并使用新 profile（工具 `:276–286`），之后 `:335` 新导航至 URL。 | 服务可复用或启动；此处复用的是 URL 服务，不是既有游戏 tab/React 状态。启动产品所需的用户确认未驱动，具体见新候选 C48-1。 |
| 6：390×1180 移动视口、进入首只股票详情、切“最快” | 工具 `:334` 设置视口，`:350–359` 点击 `.mobile-market-row`、等待详情、写 `Infinity` 并派发 `change`，必要时恢复运行。生产选择器在 `apps/web/src/mobile/MobileStockDetail.tsx` 的速度控件；`.mobile-market-row` 生产渲染位于 `apps/web/src/components/MarketGrid.tsx:199`。 | 后续交互代码存在，但默认正式旅程先因启动页等待 `.app-root` 失败；不能把不可达交互误报为已测。启动选择是独立宿主策略，不建议为性能工具改变产品自动启动政策。 |
| 7：分别观察分时与日 K，验证权威 tick 和图表签名推进 | 工具 `:364–389` 采集首尾数据、点击日 K；`market-ui-report-lib.mjs:10–29` 分别判断游戏 tick、分时与 K 线；DOM 数据由 `MobileStockDetail.tsx` 的诊断属性产出，时钟权威标记由 `ClockMarker` 产出。 | 推进检查实际存在。总旅程比较前后 tick，不是每个图表观察窗口独立各验一次 tick；README 只承诺图表分开观察与游戏 tick/图表签名推进，没有明示分阶段 tick 断言，不据此新增缺陷。 |
| 8：采集任务、脚本、布局、heap、DOM、长任务、布局偏移、近似 FPS | 工具 `:199–220` 计算 `Performance.getMetrics` 差量；`:340–348` 注入 `PerformanceObserver` 与 `requestAnimationFrame` 计数，`:385` 读 probe。 | 采集代码与“近似 FPS”相符。采样窗口和 observer 不支持时的数值局限属测量可信度/浏览器覆盖验收债；文档未承诺阈值或可靠性全覆盖，不升级为实现缺失。 |
| 9：输出 HTML/JSON 与四张前后截图 | 工具 `:366,369,378,382` 保存四图，`:404–405` 写 JSON/HTML，HTML 生成器为 `market-ui-report-lib.mjs:44–60`。 | 正常报告路径已接线。失败时不保证完整四图是合理失败边界。 |
| 11–15：正式运行命令 | README `:14` 使用 `corepack pnpm test:market-performance`；其 package script 是直调 Node 工具（`package.json:23`）。旁边 `:22` 的 unit 入口则有 `run-with-deadline.mjs 10000` 与 Node `--test-timeout=10000`。 | 正式工具入口真实存在。unit 的十秒门禁正确，不可拿它抵消集成工具自身的 deadline 候选。 |
| 17–21：duration、URL、output 可选参数 | `market-ui-report.mjs:21–42` 解析参数，限制 duration 1000–60000ms、校验 URL protocol、解析输出路径。 | 参数行为吻合；两段观察时长不是全批次 deadline。 |
| 23：默认输出；推进失败非零；初始化失败写 `failure.txt` | 默认目录 `:25`；推进结果控制 exit code `:390–407`；catch 写失败文件 `:408–411`；finally `:412` 清理。 | 对主流程失败描述属实。无限等待或清理中途抛错时仍有边界，见 C48-2。 |

## 旧候选复核

### C48-1：默认性能旅程无法越过生产启动选择 — 保留

正式命令没有附加 `--mode e2e`（`package.json:23`），启动的 Vite 使用默认 development mode（`apps/web/package.json:7`）。`apps/web/src/host/startup-policy.ts:14–16` 只在 `buildMode === "e2e"` 时提供初始 WASM target；`apps/web/src/App.tsx:700,726–740` 对其他模式初始显示 `StartupScreen`，由用户提交“启动游戏”后才设置 target。`StartupScreen.tsx:20–40` 的根是 `.app-error`，没有 `.app-root`。工具在 `market-ui-report.mjs:335–336` 导航后只等待 `.app-root`（最长 30 秒）；找 `.app-root` 失败后，`:350` 的首只股票点击和 `:352–359` 的最快选择无法执行。即使 `--url` 指向已有服务，新 CDP target 也是新页面，不能继承其他 tab 中用户已经启动的 React 状态。

独立确认首个实际调用是首行股票选择器，生产节点 `MarketGrid.tsx:199`；它位于启动门之后。故这是文档宣称的正式旅程在默认入口不可达，不是没测某种性能阈值，也不是要求产品自动跳过用户确认。应在性能工具内显式驱动既有启动 UI 并检查启动错误，或给该入口明确匹配的启动方式；不改变大 A 交易语义。

### C48-2：性能批次没有进程外总 deadline，异常清理不能收敛 — 保留

`docs/testing.md:99,120` 规定必要长验证的 child/批次总墙钟上限为 300000ms，包含进程树终止与临时文件清理；正式入口需 `run-long-validation.mjs 300000 -- <command>` 或等价进程外门禁。当前正式 `package.json:23` 直接运行工具，工具中未创建该门禁。`waitFor`（`:72–85`）只限制循环，不能限制单次永不 settle 的 probe；`CdpClient.connect/send`（`:127–150`）无超时，连接断开时 pending 不会统一拒绝；Chrome version/target fetch（`:288–300`）没有 abort signal。各 `setTimeout` 与 `durationMs` 不构成总截止。

收尾方面，POSIX `stopProcess`（`:224–231`）只发 `SIGTERM`，不等待退出、不检查进程树或升级 kill；`MarketUiReportRun.close`（`:306–312`）顺序 await，前一步抛错会跳过后续 server/profile 清理。现有单测 `market-ui-report.test.mjs:96–109` 特意断言失败步骤后停止后续收尾，因此这不是只缺一个新用例，而是当前实现清理合同有缺口。Windows `taskkill /t`、正常 finally 调用 close、每阶段若干 `waitFor` 和最多两段 60 秒观察均不能证明 5 分钟批次及收尾硬上限。

适用边界：README 将此项称为性能回归工具，但它不一定是每次普通测试的必跑项；如项目将它作为必要长验证执行，则当前入口不符合长验证 deadline 约束。可在总审查中确认分类与是否合并；不要把历史运行速度或尚未取得的浏览器报告当成实现缺失。

## 排除项与审查意见

- `scripts/performance/README.md` 在 `08e4fc7` 相对父提交无改动；当前 merge 工作树亦保留原 23 行全文。旧 `sweep48.md` 的全文范围、产品路径与两候选仍可用，本次用当前源码重核后两项均未修复。
- `market-ui-report.test.mjs` 已有 helper 通过/失败与 HTML、会话清理、远程 URL/已退出 browser 边界覆盖；`package.json:22` unit 命令的 case 与进程外 10 秒门禁正确。本次遵指令未运行测试，不声称其通过。
- 没有发现会改变 A 股领域事实或交易规则的路径；候选仅在测试工具的启动旅程与进程生命周期约束，不扩张产品范围。测量基线、采样精度、各浏览器支持差异及真实环境报告是验收/测量债，不能并入为代码遗漏。
- 依 `AGENTS.md` 所要求的独立审查回答：本工具不建模交易制度，未发现 A 股语义偏移；README 所宣称的真实启动旅程与工具本身不匹配，C48-1 属必要修正；C48-2 是入口可靠性/资源清理问题但需按是否列为长验证决定接纳；未发现需要通过 A 股法源核查的规则变更。原始条款没有额外每阶段 tick 断言、性能阈值或浏览器覆盖保证，均不人为添加。
