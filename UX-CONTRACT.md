# UX Contract

## Product context

- Audience: 中国 A 股模拟交易玩家
- Primary jobs: 浏览自选、查看个股量价与盘口、提交模拟委托、管理存档
- Target market(s): 中国 A 股
- Active locales: zh-CN
- Timezone/calendar policy: 游戏内交易日；A 股 09:30–11:30、13:00–15:00，共 240 个交易分钟
- Accessibility target: WCAG 2.2 AA

## Business-context sources

| Domain / scope | Authoritative source | Source type | Reviewed date |
|---|---|---|---|
| 状态所有权 | `docs/decisions/0004-frontend-state-redux-toolkit.md` | ADR | 2026-09-06 |
| 多端前端架构 | `docs/decisions/0007-three-deployment-frontend-framework.md` | ADR | 2026-09-06 |
| 核心逻辑边界 | `docs/architecture.md` | Architecture | 2026-09-06 |
| 市场展示惯例 | `DESIGN.md` 与用户提供的参考截图 | Product brief | 2026-09-06 |

## Visual contract

- Project `DESIGN.md`: `DESIGN.md`
- Token ownership model: `DESIGN.md` 记录规范，运行时 CSS 变量实现
- Runtime source: `apps/web/src/index.css`, `apps/web/src/mobile/MobileStockDetail.css`
- Supported themes: 桌面亮/暗；移动行情详情保持高对比浅色终端主题

## Canonical UI Map

| Capability | Canonical owner | Source of truth | Allowed variants | Verification |
|---|---|---|---|---|
| Scrollbar | 全局应用样式 | `DESIGN.md` | 稳定沟槽例外 | 窄屏浏览器检查 |
| Form | Blueprint 字段与现有委托面板 | 现有 `App.tsx` | 委托/条件单 | 构建与手工流程 |
| Select/Listbox | 浏览器原生 `select` | `UX-CONTRACT.md` | 倍速选择接受操作系统弹层外观与几何 | 窄屏浏览器、键盘与选中态检查 |
| Toast | 现有 `notice` live region | `App.tsx` | success/error 文案 | 屏幕阅读器语义检查 |
| Layer / overlay | `apps/web/src/index.css` 全局层级 token | `DESIGN.md` | 详情、底栏、遮罩、交易底页 | 浏览器叠层与焦点检查 |

## Navigation and responsive behavior

- 主导航：移动端底栏切换行情、自选、交易、持仓、我的；切换后退出个股详情。
- 模拟控制：移动端列表和个股详情的红色顶栏共用运行切换键与倍速选择控件；运行时显示暂停图形并执行暂停，暂停后显示播放图形并执行继续。倍速提供 1x、1.5x、2x、3x、6x、30x、60x、180x、360x、720x、最快；“最快”映射引擎无限速模式。不得为倍速恢复额外的顶部状态栏。
- 游戏时间：当前交易日和 `HH:MM:SS` 常驻全部移动端主页面顶栏；进入个股详情后仍显示在返回键与“上一只”之间。两处共享同一组件，数据来自 `Snapshot.day/tick`，每 tick 为一秒，跨过 11:30–13:00 午间休市并随读档恢复。
- 列表到详情：点股票进入详情；详情头返回按钮回到原列表。
- 图表周期：分时、日 K、周 K、月 K、五日、更多只改变上方图表组合。
- 信息标签：看点、资讯、盘口、资金、社区、简况独立于图表周期，默认资金。
- 图表布局：分时/盘口和分时量/逐笔采用相同两列，不允许覆盖；详情页是文档滚动，底部导航固定并预留安全区。
- 列表缩略行情：预览窗口在行情行中居中且宽度固定，并使用全天 240 个固定分钟槽位；已有行情按分钟位置从左向右增长，不按点数缩放，未来槽位保持空白。早盘和午后的容器宽度一致，阴影只覆盖已有行情至昨收中轴之间的区域。
- K 线横轴：移动端默认使用 72 个固定槽位，放大档依次为 48、30 根；每一档均以其完整容量铺满横轴，只有真实数据不足容量时才靠左并保留右侧空白。K 线实体和成交量柱共享槽位中心；默认密度下柱宽取槽位的 70%，最大放大时柱宽最大 10px、柱间净距为 3px。K 线、均线、成交量与 KDJ 共用槽位，不按当前实际数据量平均拉伸。
- K 线实体：上边界取当日开盘价与收盘价的较高值，下边界取较低值；最高价与最低价只绘制影线。昨收不得补入当日实体，跳空高开或低开必须保留真实缺口。开盘前零成交 tick 只能建立昨收占位；集合竞价后的第一笔真实成交必须重置占位 OHLC，并成为当日 `open`。
- 涨跌柱形：移动端 K 线实体、日 K 成交量和分时成交量统一为上涨红色空心柱、下跌绿色实心柱；影线与对应 K 线实体同色，并在实体边界处断开，不能穿过实体内部。
- 预置历史：新游戏启动或加载存档后，由 Rust engine 为任何没有日 K 记录的股票按该局种子生成 360 个交易日 OHLCV；最后收盘价与该股初始价衔接，已有记录不覆盖。历史和盘中蜡烛通过连接/重连/日界快照同步，Web 不生成行情。移动端默认显示最新 72 根，完整历史通过现有 K 线窗口工具访问。
- K 线窗口工具：`+ / −` 分级放大或缩小，`‹ / ›` 查看更早或更新历史，`«` 跳到最早历史，复位按钮返回最新数据与默认缩放；边界操作禁用，并保留可见焦点和中文可访问名称。
- 滚动所有权：列表态仅 `.app-grid` 主滚动；详情态冻结列表并仅让 `.mobile-detail-page` 主滚动；交易底页打开时只允许底页内部滚动。
- 层级顺序：详情内容 < 固定应用主导航 < 遮罩 < 交易底页 < 通知；遮罩拦截背景点击，Escape 关闭交易底页并把焦点还给触发按钮。
- 主导航常驻：进入个股详情后仍保留行情、自选、交易、持仓、我的；交易打开统一底页，切换其它主页面退出详情。
- 文档标题：应用级标题保持“股票模拟游戏”；后续引入路由时使用“页面 — 股票模拟游戏”。

## Flow ledger

| Operation | Trigger | Pending | Success destination | Success feedback | Failure recovery | Focus outcome | Source ref |
|---|---|---|---|---|---|---|---|
| 打开个股 | 点击行情行 | 无 | 个股详情 | 选中股票数据 | 保持列表并显示现有错误边界 | 详情返回按钮 | 当前任务 |
| 返回列表 | 详情返回按钮 | 无 | 原行情列表 | 无 | 无 | 原列表区域 | 当前任务 |
| 切换周期 | 周期标签 | 无 | 原详情滚动位置 | 标签选中态 | 保留上一张可用图 | 选中标签 | 当前任务 |
| 切换信息 | 信息标签 | 无 | 原详情滚动位置 | 标签选中态 | 显示明确占位 | 选中标签 | 当前任务 |
| 打开交易 | 底栏交易/浮动交易 | 无 | 交易底页 | 可提交委托 | Escape/遮罩关闭 | 底页首个字段 | 现有实现 |

## Async and resilience

- 行情由 Worker/WASM 事件驱动；背景刷新保留可用快照。
- 每个引擎 step 的 `PriceTick.tick` 是权威游戏秒；前端按每 60 tick 归入一个交易分钟。高倍率可压缩逐秒事件，但必须保留当前交易日每分钟的最后采样，且同一批次跨日后继续消费最后日界之后的数据。
- 日界清空前端分时缓存；Rust engine 提交当日 K 线并随新快照同步。错误通过现有错误页或 live region 明确显示。

## Verification

- Required static commands: `pnpm --filter web test`, `pnpm --filter web lint`, `pnpm --filter web build`
- Browser matrix: 320px、390px、430px 竖屏；桌面现有布局回归
- Accessibility: 原生按钮/标签语义、可见焦点、缩减动效、固定导航不遮挡焦点
- Visual regression: 与 `design/ui/mobile/` 参考及用户截图并排检查
