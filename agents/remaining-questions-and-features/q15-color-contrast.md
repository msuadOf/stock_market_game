# Q15 颜色与对比度审计记录

## 范围

- 保留 A 股红涨绿跌的图形语义：`riseGraphic` 使用 `#EF3F49`，`fallGraphic` 使用 `#009B22`。
- 按实际背景分流行情文字与图表图形：浅色面上涨文字为 `#C21807`、下跌文字为 `#007A36`；暗色面上涨文字为 `#FF8B94`、下跌文字为 `#65E6A0`。普通正文使用 `#222222`，浅色面次要文字使用 `#767676`。
- 白字涨跌价签使用独立深色背景：上涨 `#C21807`，下跌浅色主题 `#007A36`、暗色主题 `#006E32`；没有拿 K 线图形色充当底色。
- 移动详情标题栏使用 `#D4202A`，以确保白色标题文字满足 WCAG 2.2 AA。报价与详情页组件继续使用紧凑行情布局。
- 同花顺相关查找结果为 `DESIGN.md`、`design/ui/mobile/mobile-stock-detail.html`、`mobile-trading-concept.html` 和 `mobile-system-board.svg` 草图；没有找到原始同花顺截图文件。它们仅作为结构和风格草图，不作为像素复刻基线，也未生成或引入仿官方图片。

## 实现

- `apps/web/src/index.css` 定义浅色面与暗色面的上涨/下跌/持平文字 token，并保留独立图形 token。
- `apps/web/src/App.css` 将通用涨跌数字、移动端指数和投资组合行情数字显式接到适合各自表面的文字 token；面积线、宽度条仍使用图形 token。涨跌价签白字使用独立深色背景 token，保持 AA 对比度。
- `.mobile-market-toolbar` 与其按钮固定使用白色表面，不随暗色主题切换，因此次要文字固定为浅色面安全色 `#767676`，不消费暗色表面专用 `--muted`。
- `apps/web/src/mobile/MobileStockDetail.css` 将 `.rise`、`.fall`、价格轴标注和成交量文案接到文字 token；K 线、成交量 SVG、盘口深度色块仍明确消费图形 token。没有改动 SVG 点位、成交量、柱形空心/实心规则或任何撮合/成交数据。
- `DESIGN.md` 记录 token 区分与 AA 规则；`UX-CONTRACT.md` 将现有本地草图标为可用视觉参考，并注明缺少原图时不进行像素复刻验收。

## 对比度依据

按 WCAG 相对亮度公式在白色背景下计算：旧上涨图形色 `#EF3F49` 为 3.83:1，旧下跌图形色 `#009B22` 为 3.68:1，均不适合小字号正文；文字/价签底色 `#C21807` 为 6.13:1，`#007A36` 为 5.47:1，暗色主题绿底 `#006E32` 配白字也高于 4.5:1。正文色 `#222222`、次要文字色 `#767676` 对白色背景分别高于 4.5:1。标题栏 `#D4202A` 上的白字对比度为 5.20:1。

## 验证

- `apps/web/src/mobile/mobile-color-contrast.test.ts` 启动短时 Chromium 页面，加载实际全局、应用和移动 CSS，逐个从 DOM 祖先叠加透明背景得出实际有效底色；前景颜色的 alpha 也合成到有效底色后再算对比度。测试覆盖浅色/暗色正文、报价涨跌数字、标题栏、涨跌价签、行情摘要和导航，并检查 320px、390px、430px 视口下报价字号。
- `apps/web/src/mobile/mobile-color-consumer-ssr.test.ts` 通过真实 `MobileStockDetail`、`PositionsPanel`、`MarketGrid` SSR 校验浏览器覆盖的报价、持仓及上涨/下跌价签消费者结构。
- 本轮逐一核对 `App.css` 的 `--muted` 消费面：指数卡片在暗色主题有独立暗底；行情行在暗色主题使用 `--card-bg`；分类栏、导航栏随主题表面/token 配对；行情列表工具栏则明确固定白底，改用 `#767676`。浏览器测试新增工具栏文本与排序按钮并同时测亮/暗主题。
- 两个文件分别使用 `node ../../scripts/run-with-deadline.mjs 10000 -- node --test --test-timeout=10000 --test-isolation=none <file>` 运行；各自通过，均在 10 秒单命令/单 case 限制内。
- 独立复核先后发现横屏导航浅色覆盖和暗色上涨价签白字对比不足；均按实际背景修复并再次运行两条测试，浏览器 case 6.18 秒、SSR case 0.54 秒，均通过。
- 第三次复核发现白底行情工具栏错误消费暗色 `--muted`；新增断言先在暗色主题复现失败，再将工具栏及按钮改为 `#767676`。
- 工具栏修复后 root 再次并行运行两个独立文件，浏览器 case 3.31 秒（整命令 3.77 秒）、SSR case 0.50 秒（整命令 0.82 秒），均通过；实际日志分别为 `.tmp/checklist-wave2/mobile-color-contrast.test.ts.log` 和 `.tmp/checklist-wave2/mobile-color-consumer-ssr.test.ts.log`。非作者 `review_q15_colors` 第四轮完整复核确认代码发现全部闭环。
- 未运行完整 Web 测试、lint、E2E 或构建。
