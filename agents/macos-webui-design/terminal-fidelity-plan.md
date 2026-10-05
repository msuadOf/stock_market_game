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
