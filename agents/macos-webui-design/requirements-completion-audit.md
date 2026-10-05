# 终端需求逐项完成审计

2026-10-05。这是本轮产品交互验收的工作审计；Git结果由本记录之后的提交/推送命令及最终回报验证。将用户原要求、当前owner和证据分开；自动验收通过仅代表相应用例，不替代默认NPC局、真实窗口和全部边界。后续同主题复用此文件。

## 当前需求映射

| 用户要求 | 当前实现及复用边界 | 已有证据 | 状态及验证边界 |
|---|---|---|---|
| macOS编译、网页可玩、内置浏览器打开 | release WASM/本地Vite服务，原IAB tab2 | 最新独立production成功，实际902×833启动/读档 | 保留本地服务；不把源码HMR当无状态恢复 |
| 横屏电脑版、竖屏手机版 | App方向判据与各自导航，公共数据/命令 | desktop-workspace、mobile-layout，IAB902及320 | 最终完整67/67浏览器跨屏/导航通过；保留历史失败与具体设备限制 |
| 同花顺信息层次、模拟器看盘下单布局 | desktop-terminal，右买卖展开底部非模态栏，委托组件唯一实例 | desktop-workspace的多尺寸、草稿、Escape、焦点用例 | 最终67项看盘栏路径通过；保留此前27项超时历史，不冒称各次均稳定 |
| 二三级菜单、进入/返回、当前选择 | ChartDisplayMenu：显示→均线/副图→选项；公司报告/报表/附注，行情/个股/F10返回 | 历史54项及最终67项对应通过，实际参考观察见terminal-fidelity-plan | 最终67项相应菜单/返回路径通过；历史19/27失败仍保留 |
| 尽可能以手机版为准复用 | MarketKlinePanel/renderer/gestures/MA、盘口FiveLevelBook、CompanyPanel、UserPanel及命令共用 | shared-mobile-audit、shared-chart-state，两端运行用例 | 分时仅投影及竞价显示共用，桌面与紧凑手机renderer保持不同，不能称全部单renderer |
| 当前分辨率完整显示 | 弹性主/副图、坐标DOM文本、下单栏与侧栏；手机暂停选项独立44px整行 | 902/1020/1440/844横屏、320/390竖屏验收及IAB；暂停设置新6项通过，IAB两行44×309px | 手机暂停触控间距已关闭；报价320/390普通/较长/零成交量完整可见、涨跌与日内价格列无重叠；本批document越界已以真实833/833和新增跨屏case关闭；未据此声称任意数据/硬件组合全覆盖 |
| 分时数据积累后核对 | MobileIntradayProjection与真实权威minute/auction/day candle，时间简化明确 | desktop-intraday、mobile-intraday-projection；先前默认局实看 | 默认20007 NPC局已实际推进至09:48:49并核对两端截图；未据此声明全天正确 |
| 分时0%轴永远居中（最新纠正，替代旧高低贴边要求） | symmetricIntradayScale共同对称价域；DesktopIntradayChart纳入有成交OHLC，手机保持原留白，迷你图中轴height/2 | 新单边上下/空/唯一点/极值短测、跨屏E2E；IAB09:34:48真实上涨行情桌面y50、手机141/282px | 显示边界不冒称成交高低；K线未改 |
| 集合竞价左侧不空、无单0轴、更新粗点，连续竞价无点 | auctionDisplayPoints无指示价映射昨收0%，updated标指示价/可匹配量更新；两端auction dots，连续仅polyline | 两端projection/SSR，desktop连续无circle断言 | 粗点为竞价指示更新，不冒充已成交；默认09:48:49两端已实看，09:30连接仅已有末/首槽，不补数据 |
| 去掉图内TradingView标 | Lightweight Charts attributionLogo=false；SVG K线无logo | price-chart-runtime，游戏管理保留关于图表归属说明 | 不删除库必须的归属信息；当前K线截图可见无图内标 |
| 均线多选、鲜明颜色、MA5/10/20/30/60数量/顶部布局 | kline-moving-averages五条橙/蓝/紫/青/橙红；MarketKlinePanel共享开关和数值图例 | desktop多选、shared-chart-state，颜色源常量核对 | 不把viewport天数当MA；不改变其他外壳配色 |
| 滚轮缩放、Shift滚轮平移 | useKlineGestures共同输入，无设备两份逻辑 | desktop-workspace鼠标与手势；kline-gestures短测 | 最终67项对应通过，但不声称任何硬件滚轮都实测 |
| 触屏双指缩放、单指点按对齐 | 同一useKlineGestures处理pinch/tap，拖动阈值与合成click抑制 | desktop工作台触屏用例 | 真实实体触屏未实测，浏览器合成手势已验收 |
| 首次无对齐线，点开、再点关；随后移动吸附，小窗详细信息 | MarketKlinePanel局部selectedTime/following，snap最近K，详情与竖线/坐标同步 | desktop详情吸附、kline-coordinates、切股/切屏关闭 | 页面不使用单独“模式”入口；最终67项详情用例通过，保留此前耗时/失败记录 |
| SVG拉伸边框不变粗 | SVG renderer的non-scaling-stroke，坐标文字DOM | desktop轮廓拉伸、kline-coordinates | 自动几何/样式检查已做，任意系统缩放组合未全部验证 |
| 排序/搜索/自选等真实可用，去无关工具 | useSecurityBrowser唯一owner，精确sort model，手机仅可见涨幅；无伪指数/禁用金融工具 | security-browser、security-sort、mobile-market-scope；IAB实际排序 | DEV无诊断能力入口已按统一host能力过滤，真实DEV两尺寸/导航后通过；production本就不显示；桌面标题及空态已按同一范围更新，真实WASM及IAB复核通过 |
| 公司资料层级和跨屏阅读保持 | CompanyPanel controlled readingByCompany，真实CompanyQueryCoordinator，baseline civil_date | company-information阅读/日期等7项相关路径通过 | 因果已关闭：独立历史/单调用反事实完整报告相等，当前精确gold与分项/1602附注验收通过；见company-causal-audit |
| 游戏设置和存读档细节 | UserPanel两端共用；日终候选/快速槽/重复文件目标；load beforeRead等待 | 833全量Web、quick-load-barrier真实WASM2项；新文案定向5项/IAB | FS/Tauri/upload提交前读档、失败/取消及新局pending短测已关闭；不冒称所有原生文件系统环境都实测；首日缺DB不可用会造空库的旧helper |
| 好后commit/push、关闭编译subagent | 编译agent已中断，仅必要独立review复核；日常提交沿现有feat/ui-design推送 | 前批c8dfe6e本地==origin；各批独立review记录 | 本批全新review产品/测试/契约门禁通过，最后commit/push由本记录之后的命令验证；编译agent已停，服务保留 |

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

## 2026-10-05 分时0%居中与信息能力范围

最新用户纠正明确是分时线，不是K线。桌面与迷你图复用已有symmetricIntradayScale：相对昨收最大绝对偏离上下对称；桌面参考轴固定50%、中心刻度直接昨收，有成交权威OHLC仍纳入。手机原对称实现保留，三个入口都居中，设备留白参数未强行统一。旧贴边契约已同步撤下。信息标签仅保留财务/盘口/资金，删除四个同文占位入口，MOBILE_INFO_TABS定义类型及菜单；真实报告、盘口和资金组件保留，周期/返回保持选择。Home/End补齐同一手动激活owner，没有新增键盘状态。

有效TDD红分别为新桌面中心缺轴、迷你轴43.35不等于24，以及菜单7项不等于3；原坐标期望随用户新契约更新，未删极值/null/分钟/连接/量能边界。初次相关E2E3/5通过、2项Home失败，补实现及首尾/循环/只focus断言后5/5通过8.7s。完整Web841/841、155文件8分片、wall2609ms、case及进程树10000ms。完整E2E63项62通过1旧净利gold失败，workers3/共享300000ms；期间实际多个Chromium进程采到CPU，不把短测试与长验收混为一项。production1.74s、tsc及release WASM验证成功；变更文件lint/diff-check/strict premium0finding通过，全库lint仍5条原children-prop告警exit1。

IAB源码HMR重建host，启动后实际推进至第1日09:34:48，暂停并恢复1x，未提交玩家委托。002156实际+10.02%，桌面上下±10.02%且轴y50；320手机priceplot282px、轴141px，原留白上下±10.42%。两端截图intraday-centered-axis-desktop-current.png/mobile-current.png。财务入口实看公开报告与四张报表。重置临时viewport并保留原tab2、本地服务。不声称延续前批09:48:49状态。两项需求独立review通过，见intraday-axis-info-independent-review.md；全终端goal的旧财务gold、DEV无能力诊断入口及文件pending边界仍未关闭。

## 2026-10-05 DEV 无能力诊断入口

App仅一行新增当前宿主npcDecisionDiagnostics===true门禁，与原DEV/lazy门禁并用，不按设备或部署猜测。有能力分支原按钮/Inspector/错误反馈未改，既有Inspector正向组件短测随841全量通过；未新验收真实debug宿主的App入口，静态复核确认分支保留。

新增独立playwright.dev.config使用真实Vite --mode e2e及DEV=true，沿原受控无NPC局，release WASM；不是production preview。两个尺寸先红：诊断button实际1、期望0。第一次绿DEV启动加载超过5s未进入App，且同时启动的全量unit触及整命令10000ms；保留两份失败日志，不将并行与失败推断为已证明因果。改为隔离运行、原case期限不变，完整Web841/841、8分片、wall3019ms；DEV2/2通过5.1秒，含个股/游戏导航后无入口。普通门禁10000ms，长验收共享300000ms，DEV配置workers3实际2个case用2worker；CPU采样已结束未取到live样本，不冒称测得。

完整production E2E62/63、1.1分钟，仅旧净利gold失败。确认浏览器结束再build：tsc、vite318ms、release WASM成功，production JS不含两个检查器标签。定向lint/diff-check/strict premium0finding成功，全库5原children-prop告警exit1。IAB源码HMR重建host，实看并暂停第1日09:16:47、1x；保持当时自选范围，不声称延续09:34:48。902/320实际无入口，保存dev-diagnostics-hidden-desktop-current.png/mobile-current.png，重置viewport保留tab2/服务。独立复核待最终日志增量确认。新现场发现桌面自选范围仍标题“全部股票”，下一批需修正；文件pending与财务gold因果仍待核实。

## 2026-10-05 证券范围标题与空态更新

桌面标题/说明读取已有securityBrowser.view，标签与原范围按钮共用SECURITY_LIST_VIEW_LABELS，无新增范围或标题状态。真实WASM先复现自选仍显示全部股票；新标题断言修正后又发现0行自选→0行持仓仍残留旧提示。MarketGrid改用稳定React overlay及params，文案仍来自securityListEmptyMessage，未重建表格、修改筛选/排序/撮合。新case同时检验无匹配→清空搜索、持仓空态、320→1440及恢复全部5行。

TDD首轮标题1失败6.5s；完整首轮64项62通过2失败49.6s（新空态及旧财务gold）。最终源码完整Web841/841、8分片/155文件、wall3032ms；浏览器下一轮62/64、47.2s，新case通过，另有分时轴fixture切屏竞态。保留trace/error-context于security-scope-title-axis-failure：setViewport结束后约2ms即isVisible=false，因此未执行进入个股，最终停在真实手机行情列表；桌面三条中心轴断言已通过。仅将非等待条件改为等待列表出现后点击，未改轴断言、10秒case或产品切屏逻辑。重跑完整64项63通过1旧财务gold失败45.2s，exit1，不能拼为全绿。

长验收共享300000ms、workers3/RAYON10；首轮实际采到多个Chromium进程CPU15.6%/8.0%/94.3%/27.1%，最终采样已结束，无新live样本。浏览器exit1结束后独立production297ms、tsc/release WASM成功。全库lint5原有children-prop告警exit1，全部变更source/E2E定向lint、diff-check、premium strict0finding通过。

IAB原tab2真实DEV页面已实看自选/持仓正确标题及提示，保存security-scope-holdings-desktop-current.png和watchlist截图。当前第1日09:24:23暂停、1x；源码HMR后重建host，不冒称延续此前局。恢复原自选范围和临时viewport，未改名单/玩家委托，保留localhost服务。整体目标仍留财务gold因果、文件读取pending边界及需求最终门禁。

## 2026-10-05 财务因果及文件读取最终边界

两个旧门禁已关闭：财务精确gold的差额通过历史源码和20e160b仅省略settle_period_end调用的反事实，首份完整报告逐字段恢复旧报告；新增折旧109278690.49及减值减少2871190.48精确解释106407500.01差额。正式游戏简化已有docs/company-accounting2.6契约，主engine未改，不以当期实际值直接替换预期。E2E保留精确net并新增admin/impair/1602科目断言。具体输入、原样历史编译失败、mtime缓存/政策错误和隔离边界见company-causal-audit.md。

文件读档/recover在完成用户选择后复用beforeRead，再抓取FS File/读取Tauri路径/上传text；保留选择器用户激活与取消语义，不提前invalidate写入。只有选择器的AbortError为取消，屏障AbortError明确错误。新测试覆盖3adapter及命令转交，新局原有失效后等待退出也验收；无第二队列或新会话状态。有效功能红为提前getFile及漏拒绝，后续AbortError误吞为取消的红；Tauri字节mock、upload复用listener和tsc测试类型错误分开保留，不能说每轮绿。

最终完整Web847/847、155文件8分片、wall2173ms；最后纯类型标注后的定向33/33、410.49ms及tsc exit0。完整浏览器64/64、50.6s、3workers/RAYON10、共享300000ms，旧财务gold不再失败；这是一次真正完整绿色，不能覆盖此前各轮失败。结束后独立production343ms、releaseWASMverified/tsc成功。定向5文件lint、diff-check、strictpremium0finding通过；全库仍5既有children-prop告警exit1，未顺带改无关测试。实际采到多个headless进程24.5%/4.1%，不声称10核始终满载。

用户新要求：每轮Independent review新建gpt-6.1-sol high，不复用旧agent或astra。本批此前reviewer及第一次sol reviewer均已按要求中断，independent_review_fresh_sol_high完成的审查保留在final-boundaries-independent-review.md。用户再次要求重新新建后，启动全新independent_review_restart_sol_high，对当前完整diff自行复核；本次结论另记final-boundaries-restarted-independent-review.md，不以旧审查替代。IAB当前第1日09:27:19暂停1x，实际000812+10.18%两端截图final-intraday-centered-desktop.png及mobile.png，设备留白维持原差异；恢复600101/自选/列表和viewport，本批未新增玩家委托/改名单，保留已有委托拒绝notice。源码HMR曾重建host，不能说延续上一轮09:24:23局。

## 财务及文件批次结束时的范围反馈细节

最终现场又观察到手机列表在自选页切范围为全部后，按钮/数据为全部，但顶栏仍显示自选。下一小批需先用实际App复现并判断标题owner，再复用已有范围状态修正；不混入当前两个门禁修复，不因64项绿色就宣布全goal结束。任意硬件/全天全部行情的无限组合不是已验收声明，已有浏览器输入及实际窗口证据按上表保留。

## 2026-10-05 手机标题及玩家反馈范围

财务/文件批已由全新restart_sol_high完成复核，bf8ef0e/d22de32分主题提交并推送，本地与origin一致。手机顶栏现在经mobilePrimaryTitle读取同一securityBrowser.view，仅行情/自选列表使用实际范围，其他主页面保持标题；NPC普通IntentRejected不冒充玩家notice，完整events/facts/游标/重试保留，account0拒单及所有SettlementError继续显示。详见feedback-scope-audit.md及该批独立审查记录。

有效TDD11通过2失败，首次实现后另一次新fixture误将canonical facts当输入顺序的失败保留，修正为既有规范排序后仍逐字段校验全部事实。最终13/13短测116.30ms，完整849/849、8分片/155文件、wall2371ms；完整65/65浏览器44.0s、workers3、共享300000ms；结束后独立production362ms/tsc/releaseWASM通过。变更lint/premium0finding/diff通过，全库仍五个旧children-prop告警exit1。没有重复或延长普通10秒门禁，也不冒称浏览器live CPU采样（采样时已结束）。

IAB HMR回启动页后重新本地启动，新会话09:15:51暂停1x；320实际自选入口→全部标题模拟行情/5行→持仓标题及空态一致，902保持范围。未提交玩家委托、改名单或读私人档；不冒称延续旧09:27:19，也不把早盘无notice当NPC拒单现场证据。

## 手机反馈批次结束时的布局边界

最终902×833真实K线/MA及报价买入展开交易栏，截图terminal-kline-trade-final-902.png。工作区与body高度833，但html.scrollHeight996；只读DOM定位两个auto-form sr-only绝对定位标签的bottom959.3125/996.3125，无显式锚点，逃离内部滚动区导致document额外滚动。原始几何terminal-trade-scroll-before.json已保存。该问题需下一小批先加真实浏览器失败几何断言，再修公共sr-only owner；本批标题/notice复核不替代它，整体goal仍未闭合。

## 2026-10-05 document滚动边界最终验证

22a1c68/c8dfe6e分主题提交并推送，与origin一致；feedback_scope独立复核逐项读取19需求owner、对应65项真实回归和实际截图，唯一已证实的UI未闭合项是上段document越界。该批没有把原生Tauri打包、任意实体触屏/所有全天组合当本次已验证事实，也不把旧lint告警隐去。

本批公共sr-only仅补top:0/left:0，无页面overflow禁令或新scroll owner。先修新fixture的Node侧document/window类型错误（不作行为红），随后真实902/1020两case均999px对833px为有效红；2/2初绿8.2s，补完整交易页/手机条件单可访问名、编辑、草稿及关闭后2/2、7.4s。增强前完整67/67、47.9s及增强后最终完整67/67、1.2分钟分开记录；每case仍10000ms，workers3/外部300000ms。生产在最后浏览器结束后tsc/Vite1.15s/releaseWASMverified；新增test lint/premium0finding/diff-check通过，JS产品与上批849/849完整Web源相同，不因纯CSS重复短测。

CPU观察的16次ps/8秒sleep超过总10000ms，监督exit1且无样本；缩为6次、逐条flush/单ps1秒后为资源核对重跑两case2/2、27.2s，观察器exit0但六次均未见headless进程。两轮均不冒称本批liveCPU证据；没有抬高时限，也不无限重跑以取得漂亮数值。完整日志及适用限制见desktop-scroll-audit.md。

IAB同一09:15:51暂停会话CSS HMR，document833/833、scrollY0，order-panel203/497内部可滚；terminal-trade-scroll-after.json及terminal-kline-trade-after-902.png为实际修复后证据，不以绿色case替代当前真实窗口。当前服务127.0.0.1:3000已由lsof确认node监听，最终图表/交易截图已保存，已收起看盘交易栏，切回分时与600101自选列表，并reset临时viewport、markDeliverable保留原tab2；自然窗口仍902×833，html833/833、scrollY0，未推进暂停局。恢复截图terminal-restored-final-current-window.png。

本批全新gpt-6.1-sol high独立review正在核对最后diff与19项；用户要求停旧审查并重新新建，desktop_scroll_final_review_sol_high已中断，新desktop_scroll_restarted_review_sol_high使用gpt-6.1-sol high、独立记录desktop-scroll-restarted-independent-review.md。其最终记录与stage/commit/push验证完成前，不宣称整体goal结束。历史失败与已修复证据全部保留；五个旧children-prop告警为仓库既有限制，不是本批新增UI缺陷。

## 本轮最终独立门禁与收尾边界

全新desktop_scroll_restarted_review_sol_high（gpt-6.1-sol high）自行核对完整diff、19项owner、最终67回归、原始几何/截图及财务反事实证据，产品/测试/契约门禁通过，无待修复finding；正式结论见desktop-scroll-restarted-independent-review.md，不使用已中断审查结论替代。当前UI具体边界已关闭，IAB/服务恢复已实测。最后commit/push在本工作记录之后执行并以HEAD与origin一致核对，不在写入记录时虚构Git结果。五个既有lint告警、当前live CPU缺样本、原生打包及实体触屏未实测仍按证据限制保留。
