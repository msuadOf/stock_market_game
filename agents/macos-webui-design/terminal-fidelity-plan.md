# 终端交互完整性工作表

目标：同花顺为信息层次与交互主参考，股票主力模拟器为游戏操作补充。完成必须用当前实现和运行证据逐项确认。

- 已实看参考：同花顺左自选列表、顶部周期/MA、右侧报价/盘口/明细/行情数据、底部功能标签；模拟器右上买卖入口与底部同屏交易栏。
- 本批优先：盘口买卖打开非模态底部交易栏；原委托与持仓/成交复用；收起/Escape/焦点；草稿保持；窄横屏与竖屏互通。
- 后续要求：左栏自选/搜索与切股；二级页面与三级菜单的明确返回/选中/键盘关闭；分时/日周月与指标设置复用；右侧逐笔与权威行情摘要；公司信息层级与阅读保持；游戏存档/设置入口；全界面空/错/加载、键盘、主题与尺寸检查。
- 不能复制无引擎依据的行情指标、金融服务、社交、会员和辅助工具；不能用占位按钮冒充可用功能。
- 已知验证限制：上批完整回归中公司报告预期数据、暂停偏好和委托冻结/存档失败仍须核查；不能把窄图表验收当作完整目标达成。

## 2026-10-05 看盘交易批次

已实现右侧买卖入口展开底部非模态交易栏，保持图表和盘口；原委托、持仓及市场逐笔成交只挂载一份。补足 Escape、隐藏页焦点、草稿保持、同屏切股、完整交易页返回、移动详情标题同步和当前窗口几何检查。提交按钮保留真实提交语义，报价入口不下单。直接复用原证券选择与交易命令，没有改 engine。

验证结果：

- TDD 失败证据：缺少报价入口、买卖按钮超出面板、同屏委托与盘口证券分叉、完整交易页返回证券分叉、移动标题不同步，分别有失败日志。
- 20 项相关短测通过，case 和整命令进程树 deadline 均 10000ms，concurrency=3；reviewer 独立重复通过。
- 13 项专项浏览器测试通过，workers=3，共享 300000ms deadline；覆盖 900×833、1020×833、1440×900、844×390 和 390×844。
- production Web 构建和 release WASM 校验通过；变更文件 lint 通过；premium strict audit 无 finding；git diff --check 通过。
- 全部浏览器验收 24 项：18 通过、6 失败。公司报告公开编号及半年度发布日期与 fixture 预期不一致；新游戏确认提示未出现；自然日暂停偏好 checkbox 未确认；连续竞价冻结及日终存档两项未找到预期活动委托。未证明这些失败均为 baseline，不宣称全回归通过。
- 全部 Web 短测启动 8 个并发进程（机器 10 核），因缺少 desktop 生成的 acl-manifests.json 失败并取消其余分片；并未完成全量。全局 lint 仍有 5 项既有 children-prop 警告，未压制或弱化断言。
- 当前内置浏览器实看并保存截图；留在暂停的个股日K与交易栏页面。真实运行时曾显示浏览器快速槽 quota 错误，需后续独立排查存档容量；不能隐藏该错误。通知栏另有引擎委托拒绝消息，未在本批改动其来源。

独立复核最终通过；记录在 trading-dock-independent-review.md。整个终端仿照目标仍未完成：二/三级菜单、左侧自选搜索、周期入口复用、右侧逐笔与权威摘要、公司层级与完整回归问题继续按原目标处理。

现场菜单观察：同花顺“•••”展开季K、年K、多周期、周期管理；点击周期管理和 MA 后现有工具捕获仍绑定主窗口，未取得下一层设置窗口的内容，不据此猜测或宣称已复刻。Escape 已关闭临时弹层。后续优先梳理游戏已有功能的真实层级，避免照搬不支持的数据/功能。

## 2026-10-05 个股行情信息批次

桌面报价下增加“盘口／明细／行情”二级信息入口。两端共用 MarketTradeTape 的证券筛选、成交 tick、精确元价格、小数手和缓存展开；桌面摘要、移动顶部及资金页共用 marketQuoteFacts，读取权威活动日 K，不使用近期缓存反推全天成交统计。零成交 OHLC 显示未形成，缺统计显式不可用。未改变 engine、交易制度、账户或持久化。

细节验收：手机完整单位表头曾挤压数量列，缩写后仍保留完整 accessible name；展开/收起移到 sticky 明细标题，窄栏不用滚到底寻找按钮。真实暂停局600610在桌面和手机均验证38行展开、7行收起及切股后重置；390/320px量列无横向溢出（scrollWidth/clientWidth分别86/86与64/64）。测试视口已复原到902×833，游戏仍暂停，当前看盘交易栏可用。没有提交模拟委托，也没有操作参考客户端的真实交易。

验证：24项相关短测通过，case/进程树deadline10000ms、concurrency=3；reviewer独立重复24项通过。14项桌面专项E2E通过，workers=3、共享300000ms外部deadline。生产构建、release WASM校验及变更源码lint通过，strict audit无finding。补查了Tab进入信息面板的焦点路径。独立记录 quote-detail-independent-review.md；截图 quote-detail-current-window.jpg 与 quote-detail-mobile.jpg。

完整浏览器回归本轮26项：21通过、5失败。公司报告公开编号与半年度发布日预期不一致、暂停偏好checkbox、连续竞价活动委托与日终存档验收仍失败；新游戏两项本轮通过。不宣称这些失败都已证明为baseline。全量Web短测仍因desktop生成的acl-manifests.json缺失而失败并取消部分分片；全局lint仍有5项既有children-prop告警，未压制断言或规则。

下一批仍按完整目标继续：二/三级菜单与返回层次、桌面周期入口复用、左侧自选/搜索、公司资料层级，以及上述完整回归问题。手机资金区窄列长文本和横竖屏切换时document.title恢复也需单独核查，本批没有顺带改变它们。

## 2026-10-05 共用周期与分层显示菜单批次

App 的 chartPeriod 统一桌面与手机入口，ChartPeriodTabs 提供分时／日K／周K／月K及自动激活的方向键、Home／End。主导航及横竖屏切换保留周期；手机分时和 K 线改成稳定实例，切换周期保留 MA／indicator／viewport。横竖屏切换仍重建两端图表实例，局部 MA 和窗口配置未跨实例保存，已明确登记，不称为全部设置保持。document.title 按 portrait 详情／landscape 应用名恢复，真实 hook 覆盖往返切屏。

共用 ChartDisplayMenu 接入 Blueprint Popover；显示→均线／副图指标逐级进入，MA 多选保持菜单，副图单选关闭，直接图例和指标按钮继续共用原设置。上下／Home／End 导航，右方向键进入，左方向键／Escape 逐层返回，Tab 退出，外部点击关闭；不可见锚点的 portal 撤下。现有项目未引入 Blueprint 整包 CSS，本批只为该弹层建立共享定位、pointer-events 和 --z-popover 样式，没有扩大到无关弹层。320px手机实看点击、键盘、菜单边界及触发后焦点路径；902×833桌面实看，临时视口已恢复。截图 chart-menu-current-window.jpg 和 chart-menu-mobile-320.jpg。

周K／月K沿用既有 aggregateCandles 的5／20游戏交易日分组，末组保留；UI与trading-rules明确不是公历聚合，MA 的N为所选周期N根收盘价。没有改变engine、委托、撮合、费用、结算或权威日K。正式构建发现 Popper ESM星号转导出解析失败，精确alias到本机包的官方main入口修复，未增加依赖或修改node_modules。该 Vite 配置修复触发开发页整页重载；当前本地预览重新建立为第1日09:16:07暂停局，不声称维持了修复前日内运行状态。

TDD证据包括：共用周期及菜单缺失；F10切公司页遗留portal；320px菜单被详情图表盖住；周月K说明造成同屏交易底部按钮超出chart边界（红测443，预期≤428）。最后问题通过副图40px目标／24px最低的弹性高度修复，主图最低80px不变。现场chart clientHeight和scrollHeight均292，按钮底边与chart底边均428.5，无裁切。

验证：35项相关逻辑短测通过，case/进程树deadline10000ms、concurrency=3；独立reviewer重复35项通过。最终19项桌面专项E2E全部通过，workers=3、共享300000ms外部deadline，覆盖桌面鼠标、触屏手势、窄手机、隐藏菜单、周/月说明与看盘交易。变更文件lint通过，strict audit无finding，git diff --check通过。独立复核记录 chart-controls-independent-review.md，无未修复有效finding。

全量Web短测仍因desktop生成的acl-manifests.json缺失失败并取消部分分片；机器10核，实际启动8个并发测试进程。完整lint仍有5项既有test children-prop告警，新增组件告警已通过纯options模块解决，未压制规则或弱化断言。真实预览的浏览器快速槽quota错误仍可见，未在本批吞掉或伪造成功；该容量问题继续独立排查。

最终完整浏览器回归31项：26通过、5失败，workers=3，共享300000ms外部deadline。公司首次季度报告与半年度披露日期不符fixture预期；自然日暂停偏好checkbox未改变；连续竞价活动委托和日终存档两项找不到预期委托。这些问题继续保留，不认定全部是baseline。生产构建及release WASM校验通过；最后CSS改动后的生产构建另存 chart-controls-production-build-final.log。

暗色主题现场核对：新菜单在共用浅色图表旁保持清楚可见、可点击；已有图表量能／KDJ标题继承暗色浅字到白底，仍需后续明确统一图表文字主题，不能把此处检查当作完整暗色验收。现场已恢复原浅色主题、原902×833视口，留在暂停的周K＋交易栏，不提交模拟委托。

完整终端目标仍在途：左侧自选／搜索、剩余二／三级游戏与公司资料层次、全回归中的报告日期／暂停偏好／活动委托与存档，以及跨实例图表设置保持与暗色图表文字。已有菜单以真实能力分层，不复制无权威数据的金融辅助工具。

## 2026-10-05 自选与证券查询批次

确认原“自选”实际显示全市场。现由 App/useSecurityBrowser 保存唯一低频范围、query 与 favorites；SecurityListControls 共用全部／自选／持仓和名称／代码查询，WatchlistToggle 在桌面及手机报价区共用加入／移出。真正持仓仍取玩家 qty>0，偏好不改变撮合、价格、手数、T+1 或游戏存档。默认空自选，全部范围保持原初始可浏览股票；自选通过 WatchlistPreferences 独立保存到当前浏览器，验证 JSON、代码与重复项。不在当前局的代码保留偏好但不展示为当前交易证券。

过滤不会自动切股或下单。移出后保持当前报价，空自选、无持仓、查询无匹配和读取失败分别说明。搜索 Escape 清空且保留焦点，Enter 打开当前可见首行；返回和横竖屏保留 query/范围。左列表上下、Home/End 同步焦点与选中。手机相邻切股按筛选后的代码顺序，不足两只或当前已移出时禁用；原涨幅排序仍为手机列表局部设置，尚未与任意桌面列排序及跨实例状态统一，不把此限制描述为已完成。

读取失败保留原存储，禁用自选写入而仍可浏览全部行情。保存失败保持上次有效名单；重试成功明确提示并撤下旧错误、恢复开关，不自动重置损坏数据。本地存储容量错误与权限错误有可见反馈，没有吞掉异常。新原生 search 表单不使用浏览器约束校验，查询规则由共用模型负责。

独立复核发现 P2：AG Grid 默认恢复行追加尾部，清空查询后的屏幕首行与 Enter 目标不一致。TDD 修复 MarketGridRowSynchronizer：成员/代码顺序变化先 flush 已提交异步事务，再用稳定 row ID 设置有序 rowData，保留主动列排序；普通报价仍仅对应行 update，API 失败不前移提交目标。真实 E2E 按屏幕行位置核对恢复顺序、默认与主动排序下 Enter 首行一致，手机不读取隐藏 Grid 首行。新增 UI 行序红测首次用了 AG Grid36 不存在的旧容器选择器，不能算有效行为红证据；有效顺序红证据来自新增 synchronizer 行为短测。搜索／开关缺失、键盘缺失、重试成功通知缺失均有实际浏览器红测日志，未通过弱化断言过关。

细节实看：902×833 左栏按钮原挤成两行，修复后每个32×34，列表clientWidth/scrollWidth均139；320×844 手机无横向溢出，单只筛选时上一只/下一只禁用。桌面与手机截图 security-browser-desktop.jpg、security-browser-mobile-320.jpg；两张截图最终均以暂停状态重新保存。原测试自选002156已移出，恢复初始空偏好，最终全部名单与暂停预览保留。源文件热更新过程中本地引擎曾重建/恢复运行，最后重新暂停；不声称保留前批局的日内行情。当前真实预览仍显示“日终存档不能包含未处理的日内请求”错误，没有隐藏它或宣称日终存档正常。

验证事实：

- 最后27项相关短测通过，case与整命令进程树deadline10000ms、concurrency=3；独立复核也重复相关短测通过，P2修复再次复核无finding。
- 自选／桌面专项最后26项全部通过；额外隐藏桌面排序→手机首行一项单独通过。最终完整浏览器回归39项，34通过、5失败，workers=3，共享300000ms外部deadline，约1.3分钟。失败仍为公司报告2项日期/公开内容、暂停偏好1项、活动委托与日终存档2项，继续保留，不宣称全部属于baseline。
- 全量Web普通测试再次失败：缺少desktop生成acl-manifests.json，2个测试分片未成功。机器10核、8个并发测试进程；运行中进程树CPU记录有62.4%／88.8%的独立测试进程，日志 security-browser-unit-processes.log。不是全量通过。
- 全局lint仍有5项既有test children-prop警告，未删除/压制。变更生产源码、新逻辑测试、同步器测试及相关E2E的定向lint通过；改动过的local-amount-render.test.ts仍有原先2项children-prop警告，不能把定向lint称为所有变更文件无告警。
- premium strict audit 最终无finding，git diff --check通过；production Web构建、release WASM验证通过，最终构建显式RAYON_NUM_THREADS=10。
- 在最终完整E2E之后只改了低价股价格格式一行及对应SSR：原手机列表把8.55多补成8.550，现统一精确两位元价。last_price855／last_close900（跌5%）新增SSR红后绿，纳入最后27项短测，独立复核17短测通过。没有据此宣称完整E2E覆盖了此最后一行；最终生产构建覆盖该改动。

暗色现场：新列表、search、自选开关可读可操作；原图表周期文字、量能/KDJ标题与FiveLevelBook仍有白底继承浅色字的问题，继续待修，最后已恢复浅色与真实窗口。完整终端目标仍在途：剩余二／三级游戏/公司资料阅读层次、全回归5失败、跨实例图表与排序状态、暗色共用图表/盘口文字及日终存档问题。不能把本批验收当作全部细节已完成。

## 2026-10-05 暂停偏好真实操作批次

复现完整回归中的 checkbox 不更新：usePausePreferences 将显式用户修改也按 tradingE2EMode 跳过。移除此 UI 测试旁路及 synchronize 无生产用途的 skip 参数；手机/桌面都沿用宿主确认→Redux settings→sessionStorage，不改变 engine 屏障、交易阶段或初始化受控停止。

原浏览器红测复现失败；第一次修复后 check() 仍因异步确认要求即时状态失败，保留日志而不称通过。最终用 click 后逐步等待选中/取消状态，并加强两次 sessionStorage 精确断言；不采用乐观状态、不弱化确认要求。9项相关短测通过（case/命令10000ms、concurrency=3），2项移动浏览器验收通过（实际2 workers、8.1秒含构建、暂停偏好case1.7秒）。production build与release WASM验证通过（RAYON_NUM_THREADS=10），四个变更源码/测试文件lint与git diff --check通过。独立复核再次通过，见pause-preferences-independent-review.md。

本批未重复完整回归，不据此称其他四项公司报告/活动委托/存档失败已解决。完整终端目标继续在途；暗色共享图表/盘口、横竖屏图表设置保持及剩余层级继续处理。

## 2026-10-05 共用图表设置与数据面主题批次

将 MarketKlinePanel 的 MA 开关与 indicator 迁入独立 Redux chartSettings，两端及全部证券共用；viewport 按证券 code 保存，不同证券互不串窗口。当前应用会话/横竖屏/切股往返保留，刷新恢复初始值，不写游戏存档；selectedTime 保持局部，实例重建或切股后关闭。沿用共用 klineWindow/reduceKlineViewport，较短历史限制可见窗口，用户操作先按当前 total 归一化，不在 render/隐藏实例自动改写窗口。

共用 MobileStockDetail.css 拥有浅色 K 线/FiveLevelBook 文字与涨跌/平价/深度 token，移除桌面重复覆盖，暗色外壳不再把浅字继承到白底；MA 调色、外壳主题及领域行情都未改变。

TDD：深色文字实际rgb237/243/251，预期34/34/34；跨屏MA5误恢复true，预期false，均有有效行为红证据。第一次状态红用了非exact的 getByLabel 同时匹配关闭按钮，不能算有效状态失败；纠正后再取得跨屏红证据。复核发现旧桌面flat高优先级覆盖、.test.tsx不能原生执行、短历史首次右移无动作，三项均修复再次通过。原生测试保留.ts入口，用单个现有Vite编译的ChartSettingsFixture.tsx创建独立SSR store；保留全部原断言，没有新增依赖/抑制规则。短历史红测实际offset8，预期0；归一化后通过。

最终31项相关短测通过1.39秒（case/命令10000ms、concurrency=3），独立复核23项通过0.808秒。21项专项浏览器全部通过17.3秒（workers=3、共享300000ms外部deadline），覆盖菜单、桌面/触屏手势、详情吸附、线宽、看盘交易、320px与切股/横竖屏设置保持。production构建与release WASM验证通过，RAYON_NUM_THREADS=10；新/生产变更文件lint、strict audit及git diff --check通过，手机改动test仍有原先2项children-prop告警，未称该文件lint clean。独立记录shared-chart-state-independent-review.md无未修复有效finding，旧DESIGN/UX跨实例恢复默认描述已直接替换。

完整浏览器41项：37通过、4失败，56.2秒。暂停偏好通过；公司报告2项、连续竞价活动委托与日终存档2项仍失败，未证明全部baseline。全量Web短测仍缺desktop生成acl-manifests.json而2个shard失败，未完成全量；10核机器实际8个worker，CPU抽样有146.3%、89.6%、85.2%的独立进程。全局lint仍5项原先children-prop告警。

真实窗口实看902×833深色及320×844手机：MA5关闭/MACD/48槽offset12往返保留，手机chart clientWidth/scrollWidth均309；最终恢复实际视口、浅色、全部MA/KDJ/72最新窗口，选择002156，暂停第1日09:15:55。store源文件热更新触发回启动页，重新启动本地宿主后检查，不声称保留此前局日内行情。未提交模拟交易。日终存档未处理请求错误仍明确显示。截图shared-chart-dark-desktop.jpg、shared-chart-mobile-320.jpg、shared-chart-current-window.jpg。

完整目标仍在途：公司资料/游戏剩余层级和阅读保持、全回归四失败与真实日终错误、行情排序共用状态。后续还须对照参考细查 K 线价格/时间/量能坐标读数、焦点路径及盘口/下单尺寸，不把本批共用状态完成当作全部终端细节完成。


## 2026-10-05 实际会话时间投影与恢复确认批次

完整回归中的连续竞价委托/日终验收失败实际先触发 UI_RENDER_FAILED：默认900 tick开盘投影用于30/9/3短局，得到-15分钟。新增行为红日志记录实际[-15,200]与期望0/223/239槽，时钟红为09:15:03而非09:20:00。TradingTimeline读取实际SessionSetup，生命周期在connectProtocol前安装；两端分时、游戏时钟与逐笔共用低频Context。默认秒级/6秒竞价槽/60秒分钟槽保持，跨日按真实长度清空量基线。自定义长度局明确时间简化及缺阶段；开盘时钟沿用engine observation_civil_instant整段900秒比例，非三整除、0窗口及极短窗口不另造民用时间。图表覆盖区间起点与成交端点的区别在trading-rules说明，没有插值伪造成交。

独立复核定位恢复提交后resume失败仍用旧Timeline，增加EngineHost.load可选onRestored提交回调：WASM/Tauri/Remote确认后，在baseline交付前幂等安装新setup。Remote新generation baseline可能早于HTTP成功响应，红[false,true]→绿[true,true]；HTTP先丢失但服务端已恢复，红[false]→绿[true]，保留pendingRestore直到权威新/旧generation确认，旧确认不改配置并允许重试，dispose释放回调。未改变原始tick、交易阶段、撮合、价格/股/手单位或存档格式。全部有效finding修复后再次独立复核通过，见trading-timeline-independent-review.md；reviewer最终41项短测通过。

最终168项相关短测全通过1.575秒，case/进程树10000ms，concurrency=3；28项WASM浏览器专项全通过23.3秒，workers=3，共享300000ms外部deadline，含真实委托冻结、拒绝资金不足、日终委托失效、保存/刷新/读取资金持仓及继续运行、横竖屏、鼠标/触屏、菜单和暗色。该浏览器批次早于最终Remote乱序修复，不能据此称真实远程E2E已验证；Remote新增路径由短fixture验证。中途浏览器启动因新测试直接修改readonly setup而tsc失败，保留trading-timeline-final-e2e.log；fixture改为不可变参数后重跑通过，未改断言。最终production及release WASM验证通过，RAYON_NUM_THREADS=10；所有改动TS/TSX和新文件lint通过，strict audit无finding，git diff --check通过。本批未重复全量Web短测及完整公司资料E2E，原生成manifest缺失和报告日期两项仍未解决。

真实IAB恢复默认902×833并检查320×844：游戏时钟均09:16:03，320px document.scrollWidth=320；日K/MA/KDJ两端共用，未提交模拟委托。源文件HMR触发启动页后重新启动，不能称维持了旧局；最后暂停第1日09:16:03的600101日K，视口902×833。trading-timeline-current-window.png和trading-timeline-mobile-320.png为实看截图。真实预览仍明确显示“日终存档不能包含未处理的日内请求”，此前还观察到快速槽quota错误；短局存档测试通过不代表默认NPC局这些问题已关闭。

整个终端目标仍在途：K线价格/时间/量能坐标读数缺口在本次截图仍可见，需下一批重点补；真实默认局日终错误、公司报告确定性E2E、剩余游戏/公司二三级阅读层级、行情排序状态及全量生成artifact问题继续保留。不将本批时间一致性修复当作全部界面完成。


## 2026-10-05 共用 K 线坐标与读数批次

补齐 KlineCoordinatePlot 的 HTML 价格／量能／KDJ／MACD 坐标，文本保持11px而不随SVG拉伸；左右共用gutter及中间390投影，桌面镜像、手机仅右轴。价格域含选中MA，量轴整数股刻度投影后转手，零量只显示零；移除与投影不一致的重复背景网格。时间标签取实际游戏日序与槽位，短历史不拉满，周/月仍是聚合起始游戏日，手机减少中间标签。点选时间黑底提示与三图参考线同步，原鼠标、Shift滚轮、触屏路径不另写一套。

实看奇数股中间刻度曾把8403682.5股传入格式化器触发UI_RENDER_FAILED，补SSR红绿修复，不弱化formatTradeLots整数股校验。新增时间轴一度在900×740周/月看盘交易中裁切底部按钮，既有E2E红测复现后把副图切换及KlineViewportControls合入共用footer。又补压缩副图首尾文字越界/相邻刻度重叠红测，向内摆首尾坐标，短高度仅减少中间刻度而不隐藏全零单刻度。

独立review发现高位平价±.01不可表示导致零域NaN，以及微小指标全部显示0、非零基数科学计数有效位不足。全部补projection/pure/SSR红绿：留白考虑数值浮点间隔，极窄价格域按midpoint留白，不放大MA累加误差；step<1e-8用按value/step确定有效位的科学计数。动态gutter依据最终格式文本，高位价格不溢入图形。真实raw Cents、指标数据、MA值、撮合及存档契约不变。kline-coordinates-independent-review.md最终无未修复有效finding，独立20短测通过0.756秒。

最后62项相关短测全部通过1.12秒，case/进程树10000ms、concurrency=3；24项相关浏览器全部通过18.9秒，workers=3、共享300000ms外部deadline，覆盖图形对齐、现有鼠标/触屏、线宽、跨股/方向设置、显示菜单、902看盘下单、900×740周月及320窄屏。测试准备一度因E2E使用未包含在其tsconfig的SVGLineElement类型而tsc失败，移除不必要类型断言后真实重跑；未弱化几何断言。最后production build/release WASM验证通过，RAYON_NUM_THREADS=10；变更源码/测试lint、strict audit、git diff --check通过。CPU抽样返回时该18.9秒批次已完成，不冒称取得了运行中CPU采样；日志确认3个浏览器worker。本批未重复完整Webunit或公司报告全E2E，不称全量baseline。

真实IAB实看902×833与320/390/430×844，document.scrollWidth分别902/320/390/430，无横向溢出；三个手机画布宽255/325/365且x均0。桌面价/量/指标canvas均x265、宽360。源模块热更新数次返回启动页，重新启动本地游戏后最终暂停第1日09:15:54、600101日K、全部5MA/KDJ/72最新窗口、无对齐详情、交易栏关闭；未提交模拟委托。截图kline-coordinates-current-window.png、kline-coordinates-details.png、kline-coordinates-trading-dock.png与kline-coordinates-mobile-320.png保存本批实看，不声称热更新保留了先前局的日内行情。

完整目标继续在途：真实默认NPC局仍明确提示“日终存档不能包含未处理的日内请求”，此前quota问题未关闭；公司报告随机fixture的确定性E2E、剩余游戏/公司二三级阅读状态、行情排序统一和生成acl-manifests.json缺口仍需后续验证。仅本批共享坐标细节完成，不将它等同全部终端完成。

### 日界 NPC 请求与公共日终档修复（2026-10-05）

真实默认 20007 NPC 开局先结算休市自然日，原 GameSession::new 已提前生成请求；最后市场 tick 的 commit 也会提前生成下一日批次。两个入口均违背 ADR-0025 的公共日级档边界。现改为日界 pending_npc=None，首 tick 在隔离 TickShadow、ExpiryShadow 之前基于已完成自然日结的版本准备；日内继续消费前 commit 缓存。None 只在 tick 日界合法，日内缺失与非空公共待处理输入仍拒绝。

TDD 首轮编译因写错 Event 变体失败，不算行为红测；修正后休市用例真实因候选含待处理 NPC 请求失败。最初多账户 continuation 暴露并行请求顺序差异，改用确定性单 NPC fixture，保留完整状态精确断言；seed74 实际 NoSignal，因此改用既有 retail_quote_setup 与 seed8、正常玩家买盘信号，实际请求断言限定 NPC AccountId(1)，未让玩家单冒充 NPC 验证。独立复核 P1（底层 None 恢复校验）和 P2（首 tick due 观察分支缺测）均修复并复核关闭。新增 due=0、1000ticks/day 但只跑1tick的短 fixture，证明观察改变 attention、首tick失败完整回滚、成功时 attention 等于日界准备探针。

最终116个相关 Rust case全部通过23.38秒，4进程并发，每case harness1+Rayon1，普通case及命令10000ms外部deadline，定向验收共享300000ms；独立4项通过1.07秒。预编译与release WASM构建jobs10，实际采样rustc先有254% CPU，WASM单crate前端阶段约97% CPU，其依赖阶段不冒称可并行；production构建与release WASM verified。git diff --check通过。没有重跑全工作区或完整浏览器矩阵。

真实IAB重载并启动默认NPC局后，原pending错误消失，但实际出现LocalStorage quota错误。day-end-npc-before.png与day-end-npc-quota.png记录先后反馈。引擎档边界已闭环，默认局快速槽写入及刷新恢复尚未通过，下一独立Web批次继续处理容量；不宣称默认局保存全部成功或完整终端目标完成。

### 默认 NPC 局的 IndexedDB 快速槽（2026-10-05）

默认 20007 NPC 压缩档仍超过 LocalStorage 容量。快速槽改用 IndexedDB，沿用同一个 strict schema、gzip codec 和日终 writer；旧 LocalStorage 同格式槽只在 IndexedDB 缺槽时读取，不在启动复制写入，不以旧槽掩盖坏档或数据库失败。压缩、开库、put 后检查 generation，提交失效事务 abort 保留原档，成功等待 transaction.oncomplete。异常关闭清理对应连接缓存，后续正常重试可重新开库。App 延迟到旧槽确实需要读取时才访问 LocalStorage；无新依赖，无存档 schema 变化，无撮合或资金单位变化。

实际 quota 浏览器红测先复现保存失败。验收准备先出现 E2E Node 配置不支持 DOM/模块扩展问题，修正为纯常量模块和浏览器字符串 IIFE；第一次运行另有读取函数未调用及 E2E 明确跳过自动启动读档的两个失败，修正调用和实际读取操作，保留压缩字节、资金持仓、tick30 与继续推进断言。没有弱化这些断言。

最后 58 项相关短测通过 471ms，case/进程树 deadline10000ms、concurrency3；真实 WASM trading-workflows 6/6通过23.5秒，workers3、共享300000ms外部deadline，覆盖容量错误、日内不写、日终失效、刷新读取与继续运行。独立复核23项通过238ms，两个有效 P2（异常关闭与架构文档漂移）修复后复核通过，见 indexed-db-save-independent-review.md。最终 production Web 构建与 release WASM verified，RAYON_NUM_THREADS=10；变更文件定向 lint、strict premium audit、git diff --check通过。本批未重复完整 Web/unit 或全部公司资料 E2E，不宣称全量通过。

真实 IAB 默认 20007 NPC 局显示“日终存档已更新（浏览器快速槽）”，见 indexed-db-save-current-window.png。实际刷新后重新启动正常，随后从游戏管理点击“读取本地进度”，完成后明确显示“已读档（第1个交易日）”，时钟回到09:15:00并保持暂停，见 indexed-db-save-restored.png。该档是初始休市自然日的日终档，不能把随后日内09:16:29当作已保存进度；也没有仅凭刷新启动正常声称精确恢复。资金/持仓完整一致的精确断言由受控真实 WASM 浏览器用例验证，默认大 NPC 局现场验证了实际保存和读取成功。

完整终端目标继续在途：公司报告随机 fixture 确定性、剩余游戏/公司二三级阅读状态、行情排序统一及生成 acl-manifests.json 缺口仍需处理。本批只关闭默认局公共日终候选和快速槽容量路径，不等同全部菜单细节完成。

### 公司资料受控验收与查询旁路（2026-10-05）

原公司资料 E2E 将固定 seed42 的编号/日期/精确金额用于现在的随机开局；实际红测两项失败。仅固定主页面新局熵 [0,42] 后，编号/日期已稳定，但旧金额仍失败，默认NPC新局通知另有异步时序失败。改用已有 tradingE2E 的受控 seed42/零NPC fixture 后，所有公司页停在“正在请求已公开报告”；实际浏览器红测定位 App.queryCompanyReports 在测试模式直接 return。删除这项旁路，真正查询现有 WASM/CompanyQueryCoordinator，未替换 DTO 或伪造报告，不改任何原断言。固定熵同时用于手动新局，正常产品仍随机生成 seed。

公司5项最终4通过、1失败，30.8秒；完整浏览器45项最终44通过、1失败，约1.5分钟，workers3、共享300000ms deadline。原净利润断言12928574075.43保留，固定seed42实际净利12822166575.42；不能把差106407500.01元直接解释为随机fixture或直接换gold。独立只读溯源证实旧gold在6ccc4bd已有固定seed42历史证据，后续经营日结/到期支付实质变化尚未建立金额差额的因果链；仍需逐科目追查，不宣称财务全正确或全回归通过。平板报告编号/日期、移动表格局部滚动、无效日期/手动新局、真实公开查询错误均通过。全回归还确认原图表鼠标/触屏、线宽、菜单层次、看盘下单、共用设置及320px路径通过。

相关Node45项全通过432ms，case及命令10000ms、concurrency3；Reviewer独立30项通过291ms，首次错误cwd导致SSR路径解析失败、纠正后通过，未改断言。最终production构建及release WASM verified，RAYON_NUM_THREADS=10；变更App和E2E定向lint、strict premium audit与git diff--check通过。独立复核见company-fixture-independent-review.md。本批不扩大交易规则或财务公式，不把固定fixture五项当作默认20007NPC全公司路径验收。

真实IAB切入F10检查公开列表、报表与附注。App源码HMR重建了会话并恢复运行、生成新随机局；不能称保留刚读档局。随后重新暂停第1日09:18:35、600101日K、五条MA/KDJ、交易栏关闭，见company-fixture-current-window.png。新局实际日终快速槽显示已更新。实看还发现：恢复后WASM baseline缺公共自然日元数据，F10临时显示起始日期及等待状态，后续CivilDateAdvanced才恢复正确日历；日历不能以setup.start_date冒充已恢复当前自然日，列为下一批必要修复。

完整目标保持在途：精确财务gold差额、恢复自然日元数据、公司切换/横竖屏阅读状态、行情排序统一及全量Web生成artifact缺口继续保留。

## 2026-10-05 baseline 权威自然日修复批次

实看读档后的 F10 错将新局起始2030-01-01显示为当前日期。有效短测红证据4项收到null，期望真实日期；原生actor红证据序列化civil_date为Null。Worker初始/刷新/恢复与Tauri actor均在同一会话读取中交付snapshot及既有CivilDate；前端复用parseIsoDate拒绝缺失/不存在日期。未新增日历推导或改变A股规则。恢复后读取新handle，保持microtask重启前恢复回应与新baseline的既有顺序。Tauri保留generation/timeline先切换、无效解析保留旧baseline的错误边界，未修改旧净利gold。

58项相关宿主短测通过；最后新增日期fixture类型纠正后7项日期专测通过。原生32/32通过3.71秒，harness10线程/Rayon10，外部10000ms进程树deadline；编译以CARGO_BUILD_JOBS=10、外部共享300000ms完成。真实WASM三项浏览器验收全部通过22.9秒，workers3，包含读档后横/竖屏立即显示2030-01-03及平板/移动报告用例。第一次浏览器尝试因测试fixture的EventTarget端口TS类型不符合契约未启动；修正为显式WorkerRequestPort。第一次行为运行桌面日期已通过，手机需从行情进入个股才有财务tab，按真实入口修正新用例后通过；未弱化日期断言。

完整Web第一次与构建并跑触发普通10秒截止；独立运行发现旧Tauri源码契约断言仍要求旧参数/旧path。随契约更新精确断言并新增parseIsoDate检查，未删除旧语义。最终整批153文件817/817通过，8个进程，6195ms，总/每case10000ms外部deadline保持。此前缺失acl-manifests.json由真实Tauri build生成，未伪造fixture或跳过权限测试。production构建/release WASM检查、改动文件lint、目标Rust格式、strict premium audit与git diff --check通过。全库cargo fmt --all --check仍指出两处既有engine测试格式，本批未改无关文件。

独立复核初批52/52通过1.19秒，最终增量18/18通过192ms，无有效finding；记录civil-date-independent-review.md。真实IAB默认902×833读取现有日终档后F10立即显示2030-01-02，暂停第1日09:15:00；截图civil-date-restored-current-window.png。保留当前F10页面和本地服务，截图不加入源代码提交。

完整目标继续在途：公司切换/横竖屏阅读状态、行情排序统一及旧净利润gold差额尚未解决。本批只解除baseline日期及生成权限artifact/完整Web单测缺口，不声称完整浏览器gold或整个终端目标全部通过。


## 2026-10-05 每公司共用阅读选择批次

实看与有效浏览器红测复现：切到另一家公司会沿用前一公司的报表/精确金额，横屏转竖屏后所选半年度报告丢失。三项 CompanyReading（selectedReportId / selectedStatementId / exactAmountsVisible）集中到 company slice，readingByCompany 按公司隔离，两端 ConnectedCompanyPanel 与同一个 controlled CompanyPanel 共用。切股往返和双向横竖屏修改保留选择；loading/error 不清除，真正 empty 沿用原报告回退契约。新局、读档、显式 baseline 更换重置 UI 阅读状态，旧 generation 不写入新会话。报告内容、发布日期、财务公式与游戏存档契约不变，无新增依赖。

最初 E2E 猜测 seed42 半年度发布日期，locator 超时属于测试准备错误，保留 company-reading-red-e2e.log，不算有效红证据。按真实期间定位后，company-reading-valid-red-e2e.log 两项均复现行为失败；store 短测旧 reducer 5失败/1通过。绿阶段23项相关短测通过318ms，4项真实 WASM 浏览器专项通过11.6秒。完整 Web 154文件823/823通过3650ms，8个进程，case/命令进程树10000ms，见 company-reading-complete-web-unit.log。

完整浏览器第一次48项46通过/2失败：旧精确净利12928574075.43与实际12822166575.42不一致仍未解决；另一日终读档用例并发时5秒内没有已读档提示。错误页仍是第2日09:16:40且显示generic日终成功，无UI_RENDER_FAILED。随后日期+阅读3项重复3次，9/9通过38.4秒，不能抹除第一次失败或证明没有竞态。

查验测试前提：generic日终提示可能属于上一休市日，不保证目标tick30档已提交。将 trading-workflows 的只读 readQuickArchive 原样抽为 E2E 共用 helper，在原日期case读档前轮询真实 IndexedDB gzip槽的 snapshot.tick=30 / civil_clock.current_date=2030-01-03。原已读档、横竖屏日期与5秒等待断言全部保留，坏格式/解压/字段错误仍失败。最终完整48项47通过/1失败，1.4分钟，见 company-reading-final-complete-e2e.log；新阅读、日期以及原委托/保存/刷新用例均通过，唯一失败仍旧财务gold。该同步修正验证的是目标档已提交后的读取正确，不验证提交中立即读档竞态，不能据此关闭产品并发边界。

浏览器长验收显式workers=3、RAYON_NUM_THREADS=10、共享300000ms外部deadline；运行中观察到3个Node worker同时运行及多个Chromium进程使用CPU（抽样Node11.5%/12.1%，Chromium17.8%/10.7%），没有单核串跑。独立初次9项短测通过514ms，增量只读复核确认helper不改行为和新断言不弱化，并明确保留竞态边界；见 company-reading-independent-review.md，无剩余有效finding。strict premium audit无finding，改动源码/测试定向lint与git diff --check通过。最终production构建/release WASM验证通过，RAYON_NUM_THREADS=10，见company-reading-production-build.log。

真实IAB902×833实看：600101选择半年度报告/利润表/精确金额，切002156首次仍为资产负债表/缩写金额，返回600101恢复原三项选择；截图 company-reading-current-window.png。document.scrollWidth=902，无横向溢出。保留F10页面与服务；没有提交模拟委托，也不声称HMR保留先前游戏局。

完整终端目标继续在途：旧财务gold差额、行情排序共用核心与状态设计、写档过程中读取的实际并发边界仍未关闭。全回归trace另观察到AG Grid LocaleModule缺失导致console #200，需独立批次验证中文表格反馈。仅本批阅读状态完成，不把它等同所有终端细节完成。


## 2026-10-05 行情表中文提示模块补齐批次

完整浏览器trace发现AG Grid console #200：配置localeText却未注册LocaleModule。只导入并注册当前AG Grid36已有模块，复用既有MARKET_GRID_LOCALE；无新依赖，无排序比较器、市场数据、资金或交易单位变化。首次测试误认辅助文字位于aria-label，定位超时不算有效TDD证据；对照当前依赖源码，改为实际聚焦表头后的live-region，market-locale-valid-red-e2e.log明确收到Press ENTER to sort而非按 Enter 排序。随后两行生产修改使中文提示生效。

新增真实浏览器验收同时验证焦点中文提示、Enter排序ascending、空查询中文反馈及无LocaleModule错误；没有压制console或伪造文案。相关短测9/9通过1218ms，concurrency3、case/外部进程树10000ms；独立可访问性2/2通过157ms并复核完整diff，无有效finding，见market-locale-independent-review.md。行情suite首次8/9通过33.9秒，既有自选刷新case超过10000ms，新增locale当轮通过3.5秒；相同workers3/期限重跑最终9/9通过29.1秒，刷新case9.0秒。保留两次完整日志，不放宽或删弱断言，不把重跑通过当作所有并发不稳定已解决。

浏览器按长验收共享300000ms外部deadline，workers3、RAYON_NUM_THREADS10；生产构建/release WASM检查、变更文件lint、strict premium audit与git diff--check均通过。本批没有重复全量Web或完整49项浏览器矩阵：上一阅读批全量Web823通过，完整48浏览器47通过/1旧财务gold失败，不能合并成“本批全量全部通过”。

真实IAB旧AG Grid实例在HMR后仍保留英文Locale bean，刷新并通过正常启动入口重新连接本地游戏，重新暂停第1日09:16:03；聚焦代码表头后现场live-region已为按 Enter 排序。dev日志缓存仍有03:51:56的旧错误，不以历史日志声称新的回归；新浏览器用例已检查新实例没有该错误。该刷新会重建宿主并清空界面阅读选择，不声称保持旧游戏局或阅读状态。保留内置浏览器与本地服务，未提交模拟委托。

终端目标仍在途：既有精确财务gold差异、行情排序的共用设计及写档期间立即读档竞态边界继续保留，原有自选刷新用例10秒边缘耗时也不据本次重跑关闭。


## 2026-10-05 行情排序共用批次

有效 TDD 浏览器红测分别复现：桌面按现价升序后 Enter 进入 000812，而个股左列表仍以600101开头；桌面涨跌幅升序切到手机后涨幅按钮没有选中。新增 security-sort-model 统一代码/名称/现价/涨跌额/涨跌幅/昨收/买一/卖一比较器，useSecurityBrowser 作为唯一排序状态 owner，MarketGrid 仅镜像列状态与接收 uiColumnSorted 用户事件，API 镜像不回写。涨跌幅直接用原始整数分 BigInt 交叉乘法比较，盘口缺档保持 null；不改变价格、撮合、资金或单位。桌面表格、个股左列表、搜索Enter及方向键沿同一顺序；手机只投影可见的涨幅规则，降序/升序/原序循环和手机详情切股共用。隐藏桌面价格列规则不暗中驱动手机。无新依赖，无游戏存档变化。

相关11项短测全部通过154ms，case/进程树10000ms、concurrency3；完整Web155文件829/829通过5850ms，8进程。行情+排序专项11/11通过16.2秒，workers3。完整51项真实WASM浏览器49通过/2失败、2.0分钟：原净利润gold 12928574075.43 对实际12822166575.42仍未解决；desktop周期case达到10000ms截止。保留完整失败日志及 security-sort-period-timeout-evidence 的原trace/error-context，trace显示周期、MA和双向横竖屏全部断言已经通过，最后title断言触及整条期限；不能将这一轮记录为通过。参数、断言与期限不变，最终desktop19项+排序2项定向复跑21/21通过16.5秒。新排序case补902×833和375×812受控E2E截图，截图计入原10秒期限。重跑通过不关闭并发时限稳定性问题。

长验收共享外部300000ms期限、workers3、RAYON_NUM_THREADS10；完整批运行中观察到多个Chromium CPU52.0%/76.6%/25.8%同时运行。生产构建/release WASM验证、变更生产及新测试定向lint、strict premium audit0finding、git diff--check全部通过。独立review完整diff及最终截图增量均无有效finding，见security-sort-independent-review.md。原local-amount-render测试fixture的children-prop警告未在本批修改，不能声称全库lint无警告。

真实内置浏览器连接在中途暂时不可用，open_in_codex仅返回queued，不把该尝试写成完成；随后重新连接原tab2并通过正常启动入口重新建立游戏。没有保留旧局的声称。暂停第1日09:15:28后，现价升序Enter实际进入000812，左列表依次000812/600610/600101/002156/300260；902×833的K线、副图、MA及底部控件完整可见、scrollWidth902，截图security-sort-current-window.jpg。320×844实看桌面涨跌幅升序对应手机↑，点击清除为↕，再点为↓，返回桌面表头descending；scrollWidth320，截图security-sort-mobile-320.jpg。恢复默认902×833，保留000812日K、5MA/KDJ、已暂停游戏页面与服务，未提交模拟委托。原生参考模拟器的市场入口本批打开后下方空白，没有获得有效排序布局证据，不冒称从该页面验证了参考排序。

完整终端目标仍在途：原财务gold差额、写档期间立即读档的并发边界、完整浏览器并发时限稳定性需继续核实。手机截图还看到无游戏行为的禁用资金/资讯/资产/分析及工具栏装饰入口，与用户“不加入无关辅助工具”的要求需要下一批重新对照处理，不能把共用排序完成等同全部细节完成。


## 2026-10-05 手机行情能力范围清理批次

按用户明确“不加入无关辅助工具”的要求，移除原本无handler且disabled的资金/资讯/资产/分析四入口、无操作的编辑符号/多股同列文字及未建模的“模拟指数”。旧指数只是全部股票元价格均值×100，且用平均涨跌幅同时冒充点数变化，不能作为真实游戏市场指数。保留实际证券、迷你走势图、范围筛选、搜索、涨幅三态排序及进入详情。工具栏以名称 / 代码说明左列，没有新增占位功能。只删除对应DOM、派生计算及唯一CSS选择器，原股价、交易规则、engine、资金单位、图表、持久化不变；无新依赖。DESIGN同步现行能力范围。

有效浏览器TDD红测在320px真实WASM局期望辅助按钮0个，实际4个，6.3秒；实施后本批新case通过。相关14项首轮13通过/1失败：旧自选切屏case的全局“自选”定位在转换瞬间同时匹配股票范围与手机主导航，strict mode报2elements；原trace/snapshot保存在mobile-market-scope-watchlist-failure，snapshot实际自选状态为true、已呈现桌面。先限定股票范围owner，中间14/14通过13.3秒；最终再加等待桌面主导航可见，保留原aria-pressed=true、期限与其余断言，避免仅scope版在手机提前通过。独立复核对该加强后的前提及完整diff均通过，见mobile-market-scope-independent-review.md。

最终完整52项真实WASM浏览器51通过/1失败48.2秒，exit1，唯一失败仍原净利润gold12928574075.43对实际12822166575.42；没有改gold或财务公式。本批新case、最终自选定位、排序、菜单层次、原鼠标/触屏/轮廓、交易与存档等均在本轮通过，不将有限fixture等同所有边界。workers3、RAYON_NUM_THREADS10、外部共享300000ms；运行中采样3个Chromium CPU96.8%/65.9%/55.3%，并有3个Node worker。完整Web155文件829/829通过2259ms、8进程、case和命令10000ms。

生产构建/release WASM验证通过；新生产/测试定向lint、strict premium audit0finding、git diff--check通过。第一次全库lint命令因PATH缺corepack未启动，不算lint结果；补正确PATH后全库lint真实exit1，5项原有children-prop warning（mobile-component-render2、workspace-grid1、local-amount-render2）保持，没有关规则或改无关fixture。最终production成功与审查完整结果分别由实际日志确认。

实看内置浏览器320×844，删除无效内容后直接看到工具栏与股票行，没有残留空栏，scrollWidth320；截图mobile-market-scope-320.jpg。本次MarketGrid/App.css热更新前后均为第1日09:15:28暂停局，未重启宿主；不把这一局部HMR观察推广为任意源码更改都保持会话。恢复原902×833、000812日K、5MA/KDJ、交易栏关闭，保留游戏页与服务，未提交模拟委托。

完整终端目标继续在途：原财务gold的因果依据、写档中立即读档的并发边界，以及最终逐项需求审计仍需完成。此前周期10秒边缘超时已保留历史证据，最新完整52项周期通过也不能抹去该记录。

## 2026-10-05 快速槽读档等待已提交写入批次

有效短测红证据：首次/已有快速槽在日终压缩尚未提交时已调用repository.load，实际1次、期望0次；pending失败仍读取旧槽的两项边界均失败。新增beforeRead捕获调用前的tail/pendingWrite，先等已提交写入完成再读取。不会提前invalidate正在提交的候选，不等待调用后新排队日终；pending失败明确显示读档错误，本次不自动读旧槽，完成清理后显式重试可以读取上一有效档。重复点击、等待中换host、排队快照、旧repository晚到响应都有测试。文件读取/新局保持已有替换屏障，不据本批声称所有文件选择竞态解决。无engine、交易、财务、存档格式或依赖变更。

第一次绿测23项21通过2失败，原fixture复用旧commands和原notice期望未覆盖新增进度提示；修正fixture及精确notice前提。独立复核另发现P2：旧异步repository宿主替换测试因新await在读取开始前退出，失去原在途边界。新增entered deferred，明确等repository.load开始后换host再返回，原无回写断言保留。最终相关25/25通过193.16ms，concurrency3、case/整命令进程树10000ms；reviewer独立三文件24/24通过151.59ms，P2修复再次复核通过，见quick-load-barrier-independent-review.md。UX-CONTRACT记录快速槽行为，并纠正既已批准的共享排序/手机能力范围陈旧描述。

真实WASM/IndexedDB专项8/8通过14.6秒。两项新E2E在真实native gzip前设置测试流门闩，分别验证pending期间旧槽未动、显示等待、不提前恢复，以及受控压缩故障的可见错误/旧档保留；没有伪造engine或档案。quick-load-waiting-e2e.png是受控fixture等待现场，不是当前默认NPC局。最终完整Web155文件833/833通过2686ms、8进程、普通case/外部10000ms；完整54浏览器53通过1失败42.3秒，唯一仍是原净利gold12928574075.43对实际12822166575.42，未改断言或财务公式。浏览器workers3、RAYON_NUM_THREADS10、共享外部300000ms；该轮CPU采样时任务已经结束，不能冒称采到运行期多核CPU。production构建/release WASM验证、变更文件lint、strict premium audit0finding及git diff--check通过。完整lint仍exit1，5项原有children-prop警告未压制。

当前IAB旧页面HMR保留旧队列实例，新方法beforeRead不存在而明确报错；quick-load-stale-hmr-instance.jpg保存证据，不将新实例自动验收冒充旧页面可用。实际刷新并通过启动入口初始化后，暂停第1日09:15:22，再点击读取本地进度，最终显示已读档（第1个交易日）、09:15:00已暂停。quick-load-restored-current-window.jpg保存真实902×833成功读档现场，保留原tab2与本地服务。刷新重建宿主，不声称保留此前日内行情；未提交模拟委托。

完整终端目标仍在途：原财务gold因果差额和最终逐项需求审计未关闭。实看游戏管理“保存当前进度”和“另存为文件”文案与日终专用行为可能不符，需独立TDD批次核对；不能把快速槽屏障完成等同所有菜单细节完成。

## 2026-10-05 日终存档入口诚实文案批次

UserPanel 两端同一份DOM，将即时保存暗示改为日终存档说明/设置日终存档文件。常驻完整自然日结束自动存档、日内不保存、文件仅后续日终更新与首个日终前无档可读说明；两个按钮通过useId/aria-describedby关联。单条CSS控制说明字号/行高/主题色，保留四个原操作handler；仅更正useSaveCommands旧“降级下载”注释，不改保存行为。DESIGN/UX同步，既有三处E2E按钮名称随新文案更新，草稿、焦点及档案字节断言保持。

第一次红测桌面缺新入口、手机错误使用桌面入口，手机不算有效红证据；修正为实际“打开我的与存档”后，两端有效红均复现缺新入口。首轮27项25通过2失败：新case用原只适合已创建DB的readQuickArchive，open()先造空库后transaction无store导致promise未决，保留日志。新case改为只读indexedDB.databases，说明点击前后都断言SAVE_DATABASE不存在，比null档更强；没有改共享helper或产品。

第二轮27项19通过8失败2.7分钟，exit1；新两端case各4.4秒通过。原desktop7项超过10000ms，另返回行情case未出现quotes元素；不能据源代码改动小或前轮通过就称都是资源竞争。该轮未结束时主agent误启动production，发现后在tsc阶段终止父/子26206/26211/26250/26251，未进入vite；先保留save-control-labels-production-build.log和exit143，再确认E2E全部退出。不能省略验证顺序错误或据此假定8失败都有因果解释。最终参数与断言不变、workers3下定向重跑实际受影响5项，5/5通过15.2秒；见save-control-labels-affected-e2e.log。不将两轮拼成27项全绿。

相关存档短测24/24通过171.15ms，case/整命令进程树10000ms、concurrency3；变更文件lint、strict premium audit0finding、git diff--check通过。独立复核完整diff及新fixture增量通过，无有效finding，见save-control-labels-independent-review.md。浏览器结束后独立production构建348ms/release WASM验证通过，RAYON_NUM_THREADS10、外部共享300000ms；最终日志save-control-labels-final-production-build.log。本批不重复833全量Web，上一批全量833与完整54项53通过/原财务gold失败是此前结果，不混记本批。

IAB现场902×833与320×844核对常驻说明及四按钮，无横向溢出，手机document.scrollWidth/clientWidth均320。截图save-control-labels-current-window.jpg及save-control-labels-mobile-320.jpg保留。源码HMR重建宿主并恢复运行，最后重新暂停第1日09:15:34，不声称保留前批刚读档局；恢复原902×833与原tab2。未选文件、未提交模拟委托。手机两项暂停偏好的标签仍紧贴同一行，触控热区/换行需在后续独立批次确认，不混入本批文案修改。

完整终端目标继续在途：财务gold因果差额、完整浏览器时限/返回路径稳定性及逐项需求复验仍保留。

## 2026-10-05 手机暂停设置触控行批次

实看320px两项暂停偏好挤在同一行，新几何测试有效红两端label高度20、期望>=44。首次red因filter的has locator包含外层region前缀未匹配，不算产品红证据；修正为label内相对checkbox后取得真实红。仅增加.layout-mobile .mobile-game-state直接label/input两条样式：每项独立整行至少44px、12px间隔、16pxcheckbox、文字可换行；不增加handler，不改宿主确认pending、sessionStorage、自然日或交易行为。桌面与嵌套刷新方式不受scoped CSS影响，DESIGN/UX同步。

真实WASM新320/390两项及原移动暂停/倍率、两端存档说明共6/6通过8.5秒，workers3、RAYON_NUM_THREADS10、共享外部300000ms；新case10000ms，整行末端点击只改变对应选项、精确偏好JSON、两行不重叠及无横向溢出断言均通过。宿主暂停偏好短测实际6/6通过107.82ms，concurrency3、case/外部10000ms；调用参数里误带不存在host/pause-preferences.test.ts，Node未执行该文件，不冒称覆盖宿主全部测试。新E2E lint、strict premium audit0finding、git diff--check通过。浏览器退出后独立production359ms/release WASM成功，未并跑dist写入。独立复核完整diff通过，无有效finding，见game-settings-touch-independent-review.md。

IAB真实320×844两行height44、width309、y277/321，截图game-settings-touch-mobile-320.jpg。CSS HMR前后仍第1日09:15:34已暂停，未操作用户偏好；原生label行尾切换由隔离E2E验证。本批不重复完整Web或56浏览器，不以6项通过关闭前批19/27失败。诊断入口核查表明原App仅DEV时加载，正式production本来不显示；此前“正式页面还有灰按钮”的口述不准确，当前IAB为Vite开发页面，其后端未启用诊断所以仍出现disabled，开发入口能力过滤下一批单独处理。

## 2026-10-05 分时09:30接缝与默认NPC动态核对批次

在原IAB默认20007 NPC局从09:15推进并暂停于09:48:49，没有提交玩家委托。600101竞价最终显示11.20、连续首分钟采样12.32，两条独立polyline在09:30断开；连续首分钟采样不冒称开盘成交价（权威日线open仍11.20）。先增加两端SSR有效红，缺连接导致2项失败，再由auctionContinuousJoin复用既有intradayChartX：仅已有竞价末槽99和连续首槽0共用x16、且显示值不同时画普通竖连接。缺端点、竞价不完整、连续晚到、同价均不延长或补价；null参考值保持源null且不生成交易/量能/粗点。各端自身价格域和non-scaling-stroke保持，连续竞价不加点。

最终短测36/36通过892.82ms，case/外部10000ms、concurrency3；独立reviewer29/29通过802ms，完整diff复核通过无finding，见intraday-session-join-independent-review.md。完整Web155文件837/837通过2475ms，8分片并发/10核，普通case/外部10000ms。完整58浏览器57通过1旧财务gold失败53.3秒，workers3/RAYON10、共享外部300000ms；原净利12928574075.43对当前12822166575.42未改。此前19/27的原菜单/返回/时限路径这一轮全部通过，保留历史失败不把多次结果拼作全绿。运行中实际采到多个Chromium进程71.5%/102.7%等CPU；首次ps误用macOS不支持nlwp已修正，不把失败命令当有效线程采样。

E2E确认exit1并结束后独立production480ms/release WASM验证成功，未并写dist。变更source/desktop及pure测试lint、strict premium audit0finding、git diff--check通过；mobile-component-render.test.ts保留两条原有children-prop警告，未冒称全局lint全绿。IAB320实际连接16,50→16,9.615、连续circle为0且scrollWidth320；恢复原902×833时连接16,50→16,0、竞价首点0,50、scrollWidth902。保存intraday-default-094849-desktop.jpg、intraday-session-join-mobile-320.jpg、intraday-session-join-desktop-current.jpg。恢复1x，保持09:48:49暂停、原tab2与服务。只针对该默认局/时刻核对，不声称全天所有行情或手机scale已经统一。

整个终端目标继续：财务gold因果、DEV不可用诊断入口、手机320实际成交量省略、信息tab能力范围和文件读取pending边界仍需处理。

## 2026-10-05 手机报价完整数值与列间重叠批次

原默认NPC局09:48:49、320px的当日成交量2893手在全局CSS下b.clientWidth19、scrollWidth31，视觉被省略。新mobile-quote-layout.spec.ts使用实际MobileStockDetail SSR及原CSS隔离报价几何，明确不是WASM成交fixture；320/390均验证289300股→2893手、999999股→9999.99手、0股→0手，所有补充字段值不裁切/不越所属格及页面无横向溢出。有效red的hiddenWidth实际320为9、390为7；早期口述13/7已纠正，真实IAB和隔离fixture字体差异不混为同一值。

测试准备的错误路径创建、React ambient DOM类型及SSR入口CJS加载失败不是产品red，保留diagnostic日志；首次tsc短deadline超时且监督器ps枚举超时，随后确认没有残余tsc进程，最终构建实际类型检查通过。使用普通runtime动态import React render依赖、原Vite SSR加载实际组件；未改全库类型配置或新增依赖。两条CSS将补充字段标签和值上下排、去数值ellipsis，首轮7/7相关浏览器23.5秒通过。

实际截图继续发现涨跌文本+1.12　+10.00%右边91.26，而高低开列起79.36；页面无横向滚动并不意味着列内不重叠。先加Range实测文本边界与所属列检查，有效red320越界9.25、390越界7.078；再仅改报价三列24/26/50→31/23/46，保留87px高、原字号及全部字段。最终7/7相关浏览器8.5秒，workers3/RAYON10/共享外部300000ms、新两case10000ms，未降低原断言。独立review完整diff及增量通过无finding，见mobile-quote-layout-independent-review.md。

IAB真实320全局CSS最终四值clientWidth=scrollWidth=63，2893手全文可见，涨跌textRight91.26<priceLeft99.59，scrollWidth320；保存mobile-quote-layout-320-current.jpg。CSS HMR保持第1日09:48:49已暂停，未提交玩家订单/修改偏好；恢复902×833与1x。相关E2E结束后独立production304ms/release WASM成功，新E2E lint、strict premium audit0finding、git diff--check通过。纯CSS增量不重复上一批837完整Web与58浏览器；上一批57/58、唯一旧财务gold失败仍开放。本批只解决报价完整可见与列重叠，不把其等同整个终端目标完成。

## 2026-10-05 分时中心轴及手机信息入口批次

用户最终明确“分时线0轴永远放在最中间”，替代旧高低贴边要求，K线不改。DesktopIntradayChart和行情迷你图延伸已有symmetricIntradayScale；手机保持既有中心投影。桌面计入有效OHLC高低点，价格域以昨收最大绝对偏离对称展开，中点直接昨收避免负零；空/唯一/单边行情均有中心轴。迷你图保留固定分钟位置与12%空间，axisY明确height/2，不变原始Cents、撮合、竞价粗点或量能。

手机四个无实际内容的信息占位入口删除，保留真实财务/盘口/资金并通过MOBILE_INFO_TABS同时定义类型和菜单，默认资金不变；财务继续CompanyPanel，资金仍权威日内统计。独立review未发现语义漂移。首次5项相关E2E两项Home失败揭示旧键盘owner仅左右键；同一moveTabFocus加Home/End，Enter/Space原生激活，新增聚焦不改选中、首尾循环断言后5/5通过8.7s。未删弱原失败断言。

完整Web841/841、155文件8分片、wall2609ms；完整E2E62/63通过1.1分钟，唯一旧净利gold12928574075.43对12822166575.42，未改gold或公式。所有普通case/进程树10000ms，长验收共享300000ms、3workers、RAYON10；浏览器完结后独立production1.74s/tsc/release WASM成功，没有并写dist。全库lint5原有children-prop警告exit1，定向新改源码/模型/E2E lint、diff-check、premium strict0finding通过，不混称全绿。初次lint缺corepack PATH失败后使用已有工具目录重跑，非产品故障。

IAB源码HMR重建宿主，实际重新启动后推进第1日09:34:48、暂停恢复1x；002156+10.02%桌面轴50%和手机轴141/282px实看。手机留白保持±10.42%，桌面±10.02%，不称设备范围完全相同。财务实际报告和四报表入口已检查，截图手机报表为滚动后内容，不冒称包含顶层菜单。保存两端轴截图，重置临时viewport保留原tab2及localhost服务；未提交玩家订单。两功能分开提交。全goal其余边界按requirements-completion-audit保留。
