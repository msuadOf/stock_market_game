# DEV诊断入口能力过滤独立复核

2026-10-05。只读审查本批App.tsx、DESIGN/UX，以及新增playwright.dev.config.ts、e2e-dev/npc-diagnostics-entry.spec.ts完整diff。未参与实现，未运行浏览器或构建、未写dist、未提交。

1. 大 A 语义：一行实现只决定开发工具入口可见性，不修改行情、撮合、账户、存档、诊断查询或原始记录。能力继续来自统一EngineHost协商，严格true才展示，不按Web/桌面/远程部署名称猜测。DEV lazy组件定义未改，production仍排除检查器，与正式文档一致。
2. 最小必要范围：在原统一section增加能力条件，横竖屏共用，不增加新owner或依赖。已有按钮、showNpcInspector状态、Suspense、generation key、当前host参数及错误处理均保留；有能力时逻辑分支与修改前相同。原disabled条件在外层严格true后冗余，但保留防御表达不构成复杂度问题。
3. 边界及验证：新增配置显式运行真实Vite DEV，而不是用production入口不存在制造假绿；继承原并发/trace等配置，独立端口与testDir，不写dist。新测试读取转换后的App模块确认DEV=true，先等App真实启动再检查入口不存在，并覆盖两种尺寸及导航后状态。它只验证release WASM无能力路径，不声称真实debug宿主正向浏览器验证。既有npc-decision-inspector.test.ts包含capability=true当前host渲染及不另建会话断言；本批正向App路径另由保留的原分支静态核对，不夸大成端到端实测。未发现有效finding。

主agent报告有效red两端按钮count1而期望0；首次DEV验收与完整unit并跑失败保留，隔离重跑unit841/841 wall3019ms、DEV2/2 5.1秒，未扩大case期限。本review此阶段未独立执行或核定最终production/full E2E及IAB结果。静态门禁通过，最终验证待补；旧财务gold、全库lint及整体目标其他边界不因此关闭。

## 最终证据与工作文档复核

已核对当前完整diff及plan/audit新增记录，生产实现没有再扩大。最终production E2E日志62/63通过、1.1分钟，exit1仅原company-information净利gold；production-build日志318ms、release WASM verified。DEV隔离2/2与841完整Web结果按主agent最终记录补齐，首次并行失败、没有证实因果以及未取得live CPU采样均如实保留。全库5条原有lint告警exit1没有被定向成功替代；产物两个诊断标签缺席是主agent额外验证，与原DEV排除条件相符。

IAB902/320无入口属于主agent现场证据；记录明确HMR重建host后为09:16:47暂停1x，未冒称延续前批状态。真实debug宿主App正向入口仍未新做浏览器验收，静态保留分支及既有Inspector组件测试的证据范围明确。新增发现自选标题问题独立留待下一批，没有顺改。

最终结论：本批静态/证据独立复核门禁通过，无未解决有效finding，可按既有授权提交。此结论不代表全量E2E或全局lint全绿，不关闭财务gold、文件pending及整体终端目标其他边界。未重复浏览器或构建，未修改产品、提交或推送。
