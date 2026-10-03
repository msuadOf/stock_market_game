# Batch 225

基线 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。计划中的三份材料均已连续全文读取至 EOF，行数与 SHA-256 一致。它们是旧中文本地化穷尽审查的历史复核与操作指南，不是当前生产源码证据。

## 文件核验

- `web-08.md`：17 行，SHA-256 `26bb10f86e4536e65e67616f0a211155ff686a58707670600724fc91d5886dc2`。记录对四个 HTML 的历史独立复核通过，说明交互所有权、原型示例边界及拟议页面控制器需保留的 DOM/事件契约。
- `short-scan-guide.md`：10 行，SHA-256 `4242930abd301b2072d6775c199515d47861f7308e83cba940562afe8504b238`。要求按短扫描任务元数据核对调用/所有权并区分源码事实与候选建议。
- `unit-guide.md`：9 行，SHA-256 `d6a97f9ecad55591fef616413cdc07e648c5f265325e0945a108aaf273bdbe71`。要求逐文件完整核验符号归属、对象聚合可行性及跨文件边界。

## 当前基线核对

- `apps/web/index.html` 与 `apps/web/inspector.html` 是各自 Vite 入口，分别挂载 `#root` 并加载 `/src/main.tsx`、`/src/dev-inspector-main.tsx`。它们不是 mobile mock 的调用者。
- `design/ui/mobile/mobile-stock-detail.html` 仍是独立静态 HTML；除自身内联 `DOMContentLoaded` 外，当前非审计材料中未见生产模块消费该文件。仓库另有正式 React 组件 `apps/web/src/mobile/MobileStockDetail.tsx`，不能把静态图稿等同于该生产组件。
- `design/ui/mobile/mobile-trading-concept.html` 的 `MobileTradingConceptController` 拥有交互所需 root、panel、按钮及图表 DOM 引用；`conceptProgress` 独立执行纯数值映射。图表和导航处理器由控制器绑定，book 导航确实先设置 detail/watch，再调用 book 图表按钮 `.click()`，最后更新 nav class。`apps/web/src/mobile/mobile-trading-concept.test.ts` 直接读取此 HTML 并校验控制器及相关交互基线。
- `docs/open-questions.md` 中 Q6 首发全中文、不引入 i18n 已由 ADR-0007 解决；最新 ADR-0028 为发布与静态 Pages 边界，没有把这些设计稿提升为生产数据或交易规则。`docs/trading-rules.md` 仍将未支持真实交易功能限定在当前模拟范围之外。图稿中的证券行情及“盘后固定价格交易”应继续按示例内容理解，不构成现行 A 股规则证据。

## 结论

历史复核对其所描述的四个静态 HTML 及交互契约表达准确，未见应修正的冲突。当前基线进一步证实 trading-concept 已有页面控制器与源码读取测试；stock-detail 静态稿与正式 React 组件是两份不同实现。未运行测试、构建或浏览器验证。
