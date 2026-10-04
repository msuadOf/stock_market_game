# 桌面看盘交易栏独立复核

## 范围与结论

审查本批完整未提交diff：App、DesktopTerminal、LocalRefreshViews、WorkspaceGrid、desktop-terminal.css及desktop-workspace E2E。reviewer未实施功能。新增侧边报价买卖入口、底部非模态交易区域，以及原委托/查询组件重排，未修改engine规则。当前有两项需要处理或明确的发现，暂不宣称最终完成。

## 有效发现

1. P2：隐藏页仍处理交易栏Escape。DesktopTerminal按dockOpen而非dockVisible判断；从已展开交易栏进入settings后，栏位hidden但Escape仍清open状态并调用旧入口focus。需仅在实际可见的非全页交易栏处理Escape，恢复焦点时验证目标可见；否则选择当前页可见fallback，不能仅检查isConnected。补settings下Escape不改变隐藏栏状态/不抢焦点测试。
2. P2：底部委托股票下拉与上方盘口可以分叉。下拉改tradeCode与priceText但不改chartCode，用户在同屏下单看的是A盘口、委托则B。原完整交易页中的独立选择现在进入看盘下单场景，需同步图表证券，或在该场景显著明确两者不同；建议桌面选择委托证券时同步selectChart，保留移动既有行为，补A→B证券及报价/委托一致性测试。

## 语义与最小范围

- 买卖入口只打开面板并选择方向，不直接提交订单。提交仍调用既有submit，股票规则、资金冻结、可卖股数等不在本批改写；手数/股数展示延续原组件。
- 同证券重新打开/切换买卖保留已有价格数量，跨证券入口同步报价；业务draft持有在App，横竖屏切换不会复制账户或创建第二份draft。
- tradingPanels始终位于同一交易region下，切换完整交易页与dock不重复挂载原委托组件；不引入新依赖，复用边界合理。
- 窄/矮窗口CSS提供区域滚动与紧凑布局；新增E2E包含上下几何和横竖屏草稿，最终实际运行与截图由主agent完成。需按新行为同步正式UX/设计约定。

## 独立验证

在apps/web目录运行local-amount-render及workspace-grid短测：13/13通过，退出码0，约2860ms；整命令进程树deadline与case timeout均10000ms、concurrency=2。git diff --check通过。当前验证不覆盖以上隐藏焦点/标的分叉问题，修复后需再次复核。

## 首次修复复核

- Escape现使用实际dockVisible，settings隐藏页不接管；恢复焦点检查isConnected与getClientRects，原隐藏焦点finding关闭。新增E2E验证settings的保存按钮焦点保持、返回后dock和草稿仍在。
- 桌面看盘页下拉直接切股同步selectChart，重复点当前股票保留自填价格；这两条路径已修复。移动及完整交易页仍维持独立委托选择。
- 标的一致性finding尚未完全关闭：A股票打开dock→完整交易页选择B（故意不同步chart）→侧栏返回个股/行情，desktopTradingOpen仍true，dock自动显示B委托而图表仍A。该路径无需再次点击买卖入口即可提交，所以“入口可纠正”不能覆盖。已要求返回看盘时同步图表到draft标的且保留draft，或收起dock直到显式再开；补回归序列。
- git diff --check通过；本次未重复上一轮13项短测。主agent当前11项E2E仍在运行，不能将未取得结果描述为通过。

## 完整返回路径修复复核

- 所有页面的委托股票下拉统一selectChart，完整交易页切股→返回个股/行情的图表标的现与draft一致，且返回不改自填价格与数量；新增E2E保留27.50/300等值断言。初审两项P2（隐藏Escape与同屏标的分叉）均关闭。
- 独立重跑金额render与workspace：13/13通过，退出码0，约1048ms，10000ms进程树deadline/case timeout、concurrency=2。git diff --check通过。主agent报告桌面12项E2E通过；其他公司、暂停偏好及交易验收仍有失败，不宣称全回归通过。
- 发现新增低严重度身份文案漂移：移动详情A打开交易下拉选B后，selectChart使可见详情变B，但useMobileUiController的document.title仍由mobileUi.detailCode(A)生成。已建议标题依当前chartCode或同步detailCode。此问题不影响实际委托标的/撮合，但建议在本批统一选择规则时修正。
- 除上述浏览器标题同步项，未发现新的阻断性实现问题；范围仍属本次同屏交易与统一选择需求，正式DESIGN/UX已同步。

## 最终结论

- 移动标题同步项已修复：useMobileUiController显式接收共享chartCode，detailCode仅控制详情是否打开；标题effect依赖当前chartCode。App先读取共享选择再传入hook，未创建第二份证券状态。fixture在detailCode不变时切chartCode验证标题更新，新增移动E2E覆盖A详情→委托选B→标题B→关闭后B详情。
- 本批所有有效发现均已关闭。完整最终diff未见新的A股语义、价格/数量单位、委托draft或组件复用问题；仅统一UI证券选择和显示，不修改撮合、资金、存档或订单规则。native select双箭头通过layout-desktop下隐藏额外Blueprint图标解决，未新增依赖。
- reviewer独立运行ui-contract-wiring、local-amount-render、workspace-grid：20/20通过，退出码0，约5819ms；进程树deadline与case timeout均10000ms、concurrency=3。git diff --check通过。主agent报告13项桌面/移动专项E2E全部通过，本reviewer未重复该浏览器批次。
- 最终独立复核通过，本批可进入提交/交付。限制保留：完整回归的公司信息、暂停偏好、交易验收仍有失败，不将专项通过表述为全回归通过；失败清单与归因由主agent记录。
