# 批次 172：web-08 原型候选独立复核

## 读取凭证与范围

- 基线为 `.worktree/implementation-reaudit` 的 `HEAD=43b1aa5f25226c72976ca172d32f4a8eeb2272ad`，唯一计划为 `hidden-review/scan-plan.json` 的 `id=172, owner=2`。
- 三篇指定来源均从主工作区连续全文读取至 EOF；实测行数与 SHA-256 匹配计划，逐篇记录见 `batch-172.json`。另核对当前 `design/ui/mobile/mobile-trading-concept.html`、其定向测试、实施复核、现行实现总账及 ADR-0004/0009/0014。
- 本批只是静态材料与调用链复核；未运行测试/构建、未执行 Git 写操作，未修改产品代码、正式规则或 G/Q 总账。

## 逐篇复核

1. `modules/web-08.md` 将两个 `apps/web/*.html` 描述为薄挂载入口，并为 `mobile-trading-concept.html` 提出可选 `MobileTradingConceptController`，同时把其他一次性 DOM 装配和硬编码行情样例留在原型边界。当前基线的实际原型文件已包含 `conceptProgress`、私有 DOM 引用字段、`bindEvents`、`selectChartView`、`navigateTo`、`renderProgress`，并在脚本末尾实例化 controller。该状态与“候选设计，未实施”的历史文档定位并不冲突：那句话描述文档里的提案；后续独立复核 `sweep62.md` 已记录 controller 提取接入。
2. `reader-guide.md` 规定调查/复核方法：完整读取、按真实调用方与契约核边界，不以历史建议替代当前实现。它是流程指引，不是产品需求或实现承诺；本批依照其边界，并没有把该文件当业务证据。
3. `relationships.md` 将原型 controller 限定为 DOM 交互状态 owner，明确不创建第二份可写视图状态、不把硬编码行情转成 engine 领域对象，并要求保留同目标 `watch` 按钮身份、`book` 分支的程序化 click/原标题和事件顺序。当前 controller 的字段与方法、`button === candidate`、book click 与 render 调用均支持这些约束；静态 DOM 搬移脚本仍在其后执行。

## 当前 owner、caller 与 consumer

- `design/ui/mobile/mobile-trading-concept.html` 的内联脚本创建唯一 `MobileTradingConceptController` 并调用 `bindEvents()`；它只持有本页 DOM 引用，读写 DOM 的 hidden/class/text/SVG/value，不进入 React、Redux、EngineHost 或 engine 调用链。`conceptProgress` 保留为独立数值映射函数；随后的脚本继续做一次性节点搬移与静态拼装。
- `apps/web/src/mobile/mobile-trading-concept.test.ts` 是该原型控制器的直接行为测试，覆盖 controller 存在、视图切换、重复 `watch` 按钮身份、`book` 分支、进度映射与节点搬移后引用。它验证 DOM API 模拟中的脚本行为，不构成浏览器像素/E2E 证据；本轮未运行。
- 正式移动行情的 consumer 位于 `apps/web/src/mobile/` 组件和 `market-model.ts`，不是此静态概念 HTML。实现审计总账 G10–G14、G44、G46、G47 等仍按正式生产链的量、逐笔、图表槽和桌面盘口行为单独登记；原型里的固定报价与图表值不能核销这些项，也不支持新增缺口。

## 契约与结论

- ADR-0004 约束 Redux 保持可序列化 UI 快照边界、engine 保持权威；原型 controller 不触碰 Redux，符合该分层。ADR-0009/0014 规定的竞价与收盘阶段是正式游戏规则，原型内盘口、时间映射和价格数字均为展示样例，不能当作真实交易制度。此处未改变 A 股语义，不需要以原型材料扩写 `trading-rules.md`。
- 来源提出的 controller 聚合在当前原型中已有实现和定向测试；其余 DOM 拼装维持一次性流程，来源未提出需转为领域对象的理由。正式行情仍由独立生产 owner 消费，原型 controller 不构成其 caller/consumer。
- 未发现可由本批来源证明的新增、关闭或误标 G/Q，也未发现已批准交易语义被遗漏。以上判断只覆盖指定三篇来源及其明确关联的原型/生产链；不把代码现存当成官方规则背书，也不将历史 OOP 候选升级为修复授权。
