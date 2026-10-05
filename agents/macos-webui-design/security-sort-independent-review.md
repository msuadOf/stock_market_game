# 共用证券排序独立复核

2026-10-05。读取完整 tracked diff 和新增 security-sort-model.ts/.test.ts、security-sort.spec.ts，核对 MarketGrid、useSecurityBrowser、TerminalStockList、MobileDetail、行模型及 SSR fixture。未参与实现，未修改产品代码或提交。

## 三项门禁

1. 大 A 语义与精度：排序只改变显示/键盘导航次序，不修改价格、证券选择本身或订单。元价仍由原 Cents 展示；金额使用 compareMoney，涨跌幅按原始分值差除昨收的比率交叉相乘，BigInt 避免 Number 及显示舍入损失。昨收非正明确拒绝，买卖一空值保持 null，不把无报价改成零价。未新增交易制度假设。
2. 必要范围：将排序 owner 提至既有 SecurityBrowser，统一桌面行情与个股列表的可见顺序、手机版涨幅与相邻切股，是已有界面不一致的直接修复。沿用 AG Grid 的排序 UI，多列按 sortIndex 投影；手机只提取 changePct，保留隐藏桌面价格排序不驱动手机的既有契约。没有新依赖或引擎变化。原生参考应用未得到有效画面，不将本次行为宣称来自参考软件观察。
3. 边界与复杂度：setter 校验规则、复制输入并比较相等避免 echo；只接收 uiColumnSorted，API 镜像不回写 owner。所有行情列复用同一 comparator，数组稳定排序保留完全相等时输入顺序。报价改变时 mobile 与列表重新计算，AG Grid 沿原有 row transaction 更新；筛选空态和相邻循环使用过滤后的序列。规则清空恢复输入顺序，多列优先级、空盘口、超安全整数金额、报价刷新及非法输入均有短测。手机切换显式产生仅涨幅规则，属于其现有能力边界。没有发现有效阻断问题。

## 独立验证与边界

Reviewer 从 apps/web 执行 security-sort-model、market-grid-rows、security-browser-model：11/11 通过、140ms，显式 concurrency=3、case timeout=10000ms、外部进程树 deadline=10000ms。git diff --check 通过。未重复正在运行的 E2E 或构建。

新增 E2E 分别覆盖价格排序后左列表/方向键以及涨幅方向跨屏双向同步，未改旧精确财务断言。现有 BigInt 比率测试使用同昨收，未来可加不同昨收的近值对比扩展证据；静态交叉相乘公式本身正确，不构成当前 finding。

结论：独立静态与短测门禁通过，无有效 finding。主 agent 的11项真实浏览器最终结果尚待记录，不据此宣称全终端目标完成。

## 最终源码与文档增量复核

MarketGrid 解构 sortRules/setSortRules 后，applySortRules、onSortChanged、mobileRowData 和手机按钮均直接读写相同值；依赖数组对应值未变，只消除对 browser 对象属性的 lint 提示，没有改变排序或 echo 行为。DESIGN 新增单 owner、多列精确排序、手机仅涨幅、取消/报价刷新/过滤不切股及不入存档规则与实现一致。git diff --check 通过，未发现新 finding，无需为纯解构再跑同样短测。

主 agent 报告完整 Web 829/829 通过（155 文件、8 进程、5.85 秒）、11 项真实 E2E 通过（16.2 秒，发生在最后纯解构之前），变更 lint 已 clean。完整 51 项 E2E 尚在运行，不提前认定其结果；原 finance gold 精确断言保持。后续截图只可作为现场证据，不替代行为断言。独立审查通过结论保持，整个终端目标未据此完成。

## 最终截图与验收结果复核

只读核对 security-sort.spec.ts：第一个 case 明确使用 902×833 横屏，仍验证现价排序、Enter 进入、左列表首项及 ArrowDown 选择；第二个 case 保留桌面→375 竖屏→1280 横屏的双向排序断言。新增 testInfo.outputPath 截图不替换断言、不修改应用状态，输出按 case 隔离；10 秒 case 截止不变。902×833 仍为桌面布局，只把该 case 的覆盖视口收窄为明确窗口，不改变其排序语义。截图会消耗少量同一 case 时间，不能据此忽略超时。git diff --check 通过，无新 finding。

主 agent 最终完整 51 项结果为 49 通过、2 失败：旧财务 gold 和既有 desktop 周期 case 的整体 10 秒截止。trace 据主 agent 核对为周期/MA/横竖屏断言已通过，最后 title 断言碰 deadline；该 case 仍属于失败，不因前面断言通过而记作绿色。证据保留于 security-sort-period-timeout-evidence。保持 workers=3 和原 10 秒期限重跑 desktop 19 + sort 2，最终 21/21 通过、16.5 秒。production build/release WASM 通过，strict audit 为 0。

排序批次独立结论保持通过。完整 51 项并非全绿，原财务差额以及并发批次时限稳定性不因定向重跑通过而关闭；不宣称全部终端工作完成。此次未实施生产修改或重复运行验收。
