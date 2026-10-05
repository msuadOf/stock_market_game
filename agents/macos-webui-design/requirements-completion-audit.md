# 终端需求逐项完成审计

2026-10-05。这是仍在更新的工作审计，不是全部完成声明。将用户原要求、当前owner和证据分开；自动验收通过仅代表相应用例，不替代默认NPC局、真实窗口和全部边界。后续同主题复用此文件。

## 当前需求映射

| 用户要求 | 当前实现及复用边界 | 已有证据 | 仍需关闭的边界 |
|---|---|---|---|
| macOS编译、网页可玩、内置浏览器打开 | release WASM/本地Vite服务，原IAB tab2 | 最新独立production成功，实际902×833启动/读档 | 保留本地服务；不把源码HMR当无状态恢复 |
| 横屏电脑版、竖屏手机版 | App方向判据与各自导航，公共数据/命令 | desktop-workspace、mobile-layout，IAB902及320 | 完整稳定性复验继续 |
| 同花顺信息层次、模拟器看盘下单布局 | desktop-terminal，右买卖展开底部非模态栏，委托组件唯一实例 | desktop-workspace的多尺寸、草稿、Escape、焦点用例 | 最新58项看盘栏路径通过；保留此前27项超时历史，不冒称各次均稳定 |
| 二三级菜单、进入/返回、当前选择 | ChartDisplayMenu：显示→均线/副图→选项；公司报告/报表/附注，行情/个股/F10返回 | 先前完整54项对应通过，实际参考观察见terminal-fidelity-plan | 最新58项相应菜单/返回路径通过；历史19/27失败仍保留 |
| 尽可能以手机版为准复用 | MarketKlinePanel/renderer/gestures/MA、盘口FiveLevelBook、CompanyPanel、UserPanel及命令共用 | shared-mobile-audit、shared-chart-state，两端运行用例 | 分时仅投影及竞价显示共用，桌面与紧凑手机renderer保持不同，不能称全部单renderer |
| 当前分辨率完整显示 | 弹性主/副图、坐标DOM文本、下单栏与侧栏；手机暂停选项独立44px整行 | 902/1020/1440/844横屏、320/390竖屏验收及IAB；暂停设置新6项通过，IAB两行44×309px | 手机暂停触控间距已关闭；报价320/390普通/较长/零成交量完整可见、涨跌与日内价格列无重叠；其他更长数据继续实测 |
| 分时数据积累后核对 | MobileIntradayProjection与真实权威minute/auction/day candle，时间简化明确 | desktop-intraday、mobile-intraday-projection；先前默认局实看 | 默认20007 NPC局已实际推进至09:48:49并核对两端截图；未据此声明全天正确 |
| 分时上下界是当日已有最高/最低 | DesktopIntradayChart采样域纳入有成交权威OHLC，零成交占位不污染域 | desktop-intraday.test精确上下界/单价/重渲染 | 手机继续自己的紧凑scale；用户原要求的两端范围需对照确认，不猜测扩大 |
| 集合竞价左侧不空、无单0轴、更新粗点，连续竞价无点 | auctionDisplayPoints无指示价映射昨收0%，updated标指示价/可匹配量更新；两端auction dots，连续仅polyline | 两端projection/SSR，desktop连续无circle断言 | 粗点为竞价指示更新，不冒充已成交；默认09:48:49两端已实看，09:30连接仅已有末/首槽，不补数据 |
| 去掉图内TradingView标 | Lightweight Charts attributionLogo=false；SVG K线无logo | price-chart-runtime，游戏管理保留关于图表归属说明 | 不删除库必须的归属信息；当前K线截图可见无图内标 |
| 均线多选、鲜明颜色、MA5/10/20/30/60数量/顶部布局 | kline-moving-averages五条橙/蓝/紫/青/橙红；MarketKlinePanel共享开关和数值图例 | desktop多选、shared-chart-state，颜色源常量核对 | 不把viewport天数当MA；不改变其他外壳配色 |
| 滚轮缩放、Shift滚轮平移 | useKlineGestures共同输入，无设备两份逻辑 | desktop-workspace鼠标与手势；kline-gestures短测 | 最新58项对应通过，但不声称任何硬件滚轮都实测 |
| 触屏双指缩放、单指点按对齐 | 同一useKlineGestures处理pinch/tap，拖动阈值与合成click抑制 | desktop工作台触屏用例 | 真实实体触屏未实测，浏览器合成手势已验收 |
| 首次无对齐线，点开、再点关；随后移动吸附，小窗详细信息 | MarketKlinePanel局部selectedTime/following，snap最近K，详情与竖线/坐标同步 | desktop详情吸附、kline-coordinates、切股/切屏关闭 | 页面不使用单独“模式”入口；最新58项详情用例通过，保留此前耗时/失败记录 |
| SVG拉伸边框不变粗 | SVG renderer的non-scaling-stroke，坐标文字DOM | desktop轮廓拉伸、kline-coordinates | 自动几何/样式检查已做，任意系统缩放组合未全部验证 |
| 排序/搜索/自选等真实可用，去无关工具 | useSecurityBrowser唯一owner，精确sort model，手机仅可见涨幅；无伪指数/禁用金融工具 | security-browser、security-sort、mobile-market-scope；IAB实际排序 | Vite开发页面仍显示不可用NPC诊断入口，需按实际debug能力过滤；正式production原本不显示 |
| 公司资料层级和跨屏阅读保持 | CompanyPanel controlled readingByCompany，真实CompanyQueryCoordinator，baseline civil_date | company-information阅读/日期等7项相关路径通过 | 精确净利gold12928574075.43对actual12822166575.42因果仍未核清，不可改gold凑绿 |
| 游戏设置和存读档细节 | UserPanel两端共用；日终候选/快速槽/重复文件目标；load beforeRead等待 | 833全量Web、quick-load-barrier真实WASM2项；新文案定向5项/IAB | 文件选择在pending写入期间的边界未本批扩展；首日缺DB的旧E2E helper检查会造空库，不应复用到该前提 |
| 好后commit/push、关闭编译subagent | 编译agent已中断，仅必要独立review复核；日常提交沿现有feat/ui-design推送 | 前批c0ea9f1本地==origin；每批review记录 | 全部goal未完成，不将阶段提交当整体收尾 |

## 验证状态不能合并

- 快速槽屏障批：833/833 Web，完整54浏览器53通过1旧财务gold失败42.3秒，production成功。
- 日终文案首轮：27浏览器25通过2新fixture失败；修正只读DB检查后第二轮19通过8原路径失败，2.7分钟。
- 文案最终仅受影响5项重跑5/5通过15.2秒、24项短测及production成功。没有完整56项全绿的证据。
- 全库lint仍有5项原有children-prop警告；变更文件lint通过不能冒充全局通过。
- 手机暂停设置批：有效红两端20px、期望至少44px；6/6相关浏览器8.5秒、6/6宿主偏好短测107.82ms通过。IAB320×844两行44×309px；独立production359ms/release WASM成功。仅CSS两条，不改变确认pending与偏好命令。
- 完成门禁仍开放：财务gold因果、浏览器时限/返回路径稳定性、默认局分时动态最终证据和开发页面不可用诊断入口范围。诊断入口源码明确受DEV限制，正式production原本就不显示，不能把开发截图错误推广到正式构建。

- 分时接缝批：36/36短测、837/837完整Web（8分片，2475ms）、58浏览器57通过1旧财务gold失败53.3秒。菜单/返回/时限路径本轮通过，不消除旧失败记录。独立production480ms/release WASM、premium0finding、独立复核通过。默认NPC局09:48:49两端截图已补，连接与粗点资格共用，无新成交样本。
- 手机320动态成交量省略已修复，真实2893手完整可见，涨跌列重叠已修复；新两端几何与相关共7项通过。信息tab能力范围与DEV灰诊断入口仍开放，正式production从来不显示诊断入口。

- 手机报价批：实际组件几何TDD先复现量值截断，再以Range复现列内文字重叠；最终7/7相关浏览器8.5秒，production304ms/release WASM、新test lint/premium0finding/独立review通过。真实320默认局2893手完整且涨跌文本不侵入高低开，恢复原电脑窗口及暂停。没有重复全量测试，财务gold仍开放。
