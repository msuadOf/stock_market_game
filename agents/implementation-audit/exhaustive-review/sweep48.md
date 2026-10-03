# Sweep 48：行情 UI 性能工具全文复核

日期：2026-10-03；产品基线 b76ece3，审计 merge 工作树4ad5a2e。连续全文读完 `scripts/performance/README.md`，共23行。并追踪当前 `market-ui-report.mjs` 422行、`market-ui-report-lib.mjs` 60行、`market-ui-report.test.mjs` 150行，共632行工具/测试源码。已遵循根AGENTS/principles；仅新增本审查记录，未修改产品、未执行Git写或长测试。

## 全文条款状态

| 原文位置/承诺 | 当前入口、数据、case | 状态 |
|---|---|---|
| README:1、3 真实Chrome/Edge CDP | report.mjs:12列候选路径、:277启动headless Chrome、:120 CdpClient、:294连接CDP | 实现存在；没有退役记录，不把工具当旧sealed corpus要求恢复。 |
| README:5 复用url/自动Vite | report.mjs:87端点探测、:96本机Vite、:260按需启动；新Chrome空profile | 服务启动存在，当前游戏启动确认漏接，见C48-1。远程url不自动启动本地服务是合理边界。 |
| README:6 移动视口、首股、最快 | report.mjs:334设390×1180、:350点首行、:352设Infinity并恢复运行；mobile/MobileSpeedSelect.tsx:14消费change、mobile-ui-state.ts:8最快标签 | 选择代码已实现，但生产startup阻断使默认旅程不可达；不把“最快收益未测”当未实现。 |
| README:7 分时/K线与tick推进 | report.mjs:364–389分别采样；lib.mjs:10 analyzeChartProgress；MobileStockDetail.tsx:90–91 K诊断、:111–114分时诊断；LocalRefreshViews.tsx:49权威时钟marker | DOM诊断与实际判定存在。tick只取总旅程前后，并非每阶段各一对；文档无逐阶段tick单独门禁，不新增需求。 |
| README:8 task/script/layout/heap/DOM/longtask/shift/FPS | report.mjs:199 CDP Performance.getMetrics、:204计算差量、:340注入PerformanceObserver/rAF probe、:385读取probe | 采集代码存在；无性能阈值/历史baseline要求，不因只PASS推进而声称未实现性能工具。测量窗口/unsupported观察见排除项。 |
| README:9 HTML/JSON/四截图 | report.mjs:366、369、378、382四图；:404–405两个report；lib.mjs:44构建HTML | 已接线；初始化失败不能保证四图属正常失败边界。 |
| README:11–15 正式pnpm入口 | package.json:23直调report.mjs | 已有入口；启动确认缺口C48-1、总deadline缺口C48-2。 |
| README:17–21 duration/url/output参数 | report.mjs:21 parseArgs、:37 duration范围1000–60000、:40 URL校验 | 参数存在；每图观察时长不是总批次硬期限，不能代替deadline。 |
| README:23 默认output、失败非零、failure.txt | report.mjs:25默认路径、:407失败exit1、:408–411 failure.txt；:412 finally close | 报告失败路径存在；cleanup失败会停后续且不进上方catch，deadline收尾遗漏归C48-2。 |

单元case真实覆盖：`market-ui-report.test.mjs:14`浏览器异常description、`:21`三诊断推进、`:32`图表停止负控、`:43`HTML数据/图片；`:62`资源会话以及`:123`远程URL/退出browser边界。`package.json:22`普通测试同时使用10000ms case timeout与进程外10秒deadline。没有跑本轮测试，不声称通过。根test发现策略仍属既有Q05，不重复。

## 新候选C48-1：性能工具未适配生产启动选择，默认命令无法进入游戏

原文README`:5–7`承诺启动/复用真实页面后进入第一只股票并观测图表。正式`package.json:23`和README`:14`运行`market-ui-report.mjs`，`:104`自动起Vite且未传`--mode e2e`，默认为development；`:276`新建空browser profile。

当前`apps/web/src/host/startup-policy.ts:14–15`只有e2e模式才自动给WASM startupTarget，其余均null。`App.tsx:700`采该策略，`:726–738`等用户启动确认；`app/StartupScreen.tsx:20`渲染`.app-error`启动选择，`:15–17`form提交才调用onStart，根`.app-root`尚不存在。

工具`:335`导航后`:336`立即等待`.app-root`，直到30秒失败；全文没有选择local模式或提交启动form，`:350`首个UI点击是股票行，位于这次wait之后。因此默认Vite路径及复用正常production页面都停在启动选择，无法执行承诺的移动最快档测量。独立CDP新tab不复用已手动开始游戏的旧tabReact状态。

反证：Vite服务启动逻辑、后续移动点击/Infinity选择确实有实现；e2e模式会自动启动（`runtime-startup-policy.test.ts:8–12`固定该区别）。但工具默认不启e2e，不能以其他测试模式替代真实产品启动旅程。最小修复边界是工具驱动现行启动确认并明确被测宿主，不改变产品自动启动政策、不新增隐藏生产旁路。

## 新候选C48-2：正式性能批次缺进程外总deadline及收尾收敛

现行`docs/testing.md:99,120`和根AGENTS要求必要长验证child/整个批次至进程树终止、临时文件清理均不超过300000ms，并有进程外supervisor；不能靠各步骤时间相加或duration参数。

当前正式`package.json:23`直调Node report，README`:14,20`也只调用该入口，工具全文无run-long-validation/外部worker监督。`report.mjs:72`waitFor的循环虽然有20秒截止，但await probe不带abort；`CdpClient.connect`在`:127`、`send`在`:144`都没有超时，socket close/error也没有拒绝已挂起pending。浏览器CDP断开或无回复时，首次页面查询/截图即可永远不settle，循环截止不再能执行。`connectPage:297`和Chrome端口fetch`:289`同样没有AbortSignal。

资源收尾：Linux `stopProcess:224–230`仅向直child SIGTERM，不等待close、不终止树或必要时SIGKILL；`MarketUiReportRun.close:306–312`依序await，一次client.close/browser.stop失败会跳过server/profile清理。现行测试`:96–109`明确锁定“清理失败按原有边界停止后续”，并未提供共享deadline内全部cleanup收敛。这不是尚未测量快慢，而是正式约束代码缺席。

反证：默认两阶段观察各5000ms、最大各60000ms，并有若干30秒/20秒wait；Windows有taskkill /t；正常finally也会调用close。这些限制不能约束未返回的CDP Promise、Linux后代或异常cleanup。普通unit入口双10秒门禁已满足，不能将其误记本候选。手工外包`run-long-validation`可限一次调用，但README/package正式入口未接该路径。建议父任务合并deadline与cleanup为同一工具收口，避免额外要求产品发布跑性能测试。

## 测量观察、反证与排除

- `report.mjs:340`probe在测量前初始化，CDP before位于`:363`，二者窗口不同；longtask observer使用buffered；ApproxFps因此只是更广窗口的近似值。README明确近似FPS，未承诺精确帧时间，这里不新增确定缺口。
- observer不支持时`:345–346`仅console.warn，默认longtask/shift零值可能表示不可采而非实测零；CDP metrics缺项`:205–214`也以0处理。记录测量可信度边界，不据此声称所有浏览器都不能采集或伪造当前测量结果。
- lib.mjs只比较diagnostic变化，未对NaN/缺字段建专用负控；目前真实生产诊断节点已有数值/签名数据。本轮不以假页面构造认定游戏图表推进未实现。
- 原文没有强制benchmark baseline、性能门槛、多宿主矩阵或CI接线要求，未执行真实Chrome/Wayland等保留为验收债。已退役的sealed corpus/before不在此工具范围；G16/G18/G19/G31性能生产缺口继续独立，不能靠本工具存在核销。

两个新候选仅属当前工具真实生产入口/资源时限契约，未改变A股语义或扩大产品范围；待总审查独立复核、去重与登记。
