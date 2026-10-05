# 以手机版为准的组件复用检查

- K 线、MA、成交量、KDJ：抽出 MarketKlinePanel，两端直接使用同一 SVG renderer、MobileKlineProjection 和 KlineViewportControls。保留此前确认的五条均线开关，使用完整周期 SMA；均线值纳入可见价格域。
- 五档盘口：抽出 FiveLevelBook，两端共用价格、手数、真实空档、卖盘反向顺序及深度条。
- 分时：两端已共用 MobileIntradayProjection、auctionDisplayPoints、时间坐标和数量格式。外层 renderer 暂保留：桌面需满足此前明确要求的当日极值贴边、价格/百分比双标尺，手机有紧凑盘口与逐笔组合，不能直接互换。
- 委托下单、条件单、持仓、成交记录、游戏存档：App/LocalRefreshViews 已复用同一业务组件与状态，桌面仅由 WorkspaceGrid/desktop-terminal.css 提供布局，无需再造手机版副本。
- 公司资料：两端已复用 CompanyPanel。
- 游戏时钟/速度/暂停：数据和 sessionControls 共用；移动端紧凑控件、桌面工具栏布局继续保留。
- 行情列表、导航：保留设备差异；桌面需宽表/侧栏，移动端需纵向列表与底部导航，复用行情与选择状态，不把移动页面整体嵌进桌面。

本轮只重构 renderer 和展示容器，不改变撮合、交易制度、存档或股/手单位。
