# 共享周期与显示菜单独立复核

## 范围及初步结论

相对HEAD 37778d1审查全部追踪diff及新ChartPeriodTabs、ChartDisplayMenu、CSS，忽略webui-service.pid。reviewer未实施代码。当前未发现阻断性产品实现问题，但有一项本批引入的测试契约遗漏，修复后需复核；不宣称原完整终端目标完成。

## A 股语义与必要范围

- 周/月K沿用既有5/20游戏交易日分组，未新增真实公历周月规则；界面与trading-rules明确简化、末组不足也显示、历史首根改变可影响分组。MA N指所选周期N根收盘价，仍完整N样本才输出。聚合仅展示，不改变成交、委托、结算或权威日K。
- App统一使用同一chartPeriod，主导航不重置周期；两端共用周期入口和显示菜单。手机分时/K线稳定挂载保留局部指标/MA/窗口状态；不重复创建交易或金额来源。
- Popover复用已有Blueprint，子菜单多选/单选和原按钮共用状态。方向/Home/End及逐级Escape/左键返回明确，Tab关闭、外部点击关闭；IntersectionObserver在锚点不可见时收起，disconnect有cleanup，避免隐藏页portal残留。层级token介于普通sticky与移动导航/交易sheet之间，范围局部。
- Vite alias精确匹配@popperjs/core，不影响子路径/其他模块；本机独立解析确认指向该安装版本官方main dist/cjs/popper.js，不是修改依赖源码或新增依赖。生产构建结果仍须主agent验证。

## 发现与验证

- P2测试同步遗漏：ui-contract-wiring Q04静态断言仍期待document.title=mobileDetail?，新增orientation条件使测试失败。要求同步断言并增加真实hook的portrait详情→landscape应用名→portrait证券名行为验证，不只改regex。标题实现逻辑本身正确。
- 独立运行ui-contract-wiring、MarketKlinePanel、mobile-ui-state、mobile-component-render，35项中34通过1失败（上述Q04），退出码1；case与进程树deadline均10000ms、concurrency=3。git diff --check通过。
- 已检查旧断言按稳定实例要求改count1+hidden，仍验证隐藏状态；MA计数限定MA并另验显示菜单入口，非删除核心断言。新增E2E覆盖隐藏锚点、320px层级、周期/菜单状态与键盘回退，当前运行结果和手工检查由主agent补齐。
- 旧全回归限制继续保留；当前专项及静态结论不替代完整回归通过。

## 修复后最终源码复核

- Q04契约遗漏已关闭：静态断言包含portrait条件，真实hook fixture接受orientation，明确验证竖屏证券名→横屏应用名→竖屏证券名三步；未删除原切股/返回断言。
- CHART_INDICATORS与type移入纯options模块，组件与原直接按钮共同导入，无周期/指标行为变化。显示按钮显式onClick与受控Popover均设置同一个!open目标；本机Blueprint源码确认受控handleTargetClick使用!props.isOpen，非连续两次函数式反转，因此不会相互抵消。最终鼠标/键盘E2E仍由主agent完成。
- reviewer独立重跑前述四套短测：35/35通过，退出码0，约637ms；case/进程树deadline均10000ms、concurrency=3。git diff --check通过。主agent报告此前18项专项E2E、strict audit及变更lint通过，最后小改后的完整浏览器/构建验证继续由主agent记录。
- 完整当前diff独立源码审查通过，无未解决有效finding，未发现新A股语义漂移或不必要复杂度；交付结论仍须包含最终必需验证结果与既有全回归限制，不宣称原完整终端目标完成。

## 周月 K 同屏交易高度修复复核

- CSS仅修改desktop-trading-open下量能/KDJ两个副图的flex收缩：40px目标、24px下限、flex:0 1 40px，保留蜡烛min80与其余布局。用于给新增简化说明腾出高度，不改变图表数值、周期或交易语义，范围必要。
- 新900×740周/月两次E2E断言按钮底部不越出图表、MACD可点击和账户摘要可见。该测试对应主agent报告的红测443>428，保留几何断言而非删除功能。主agent现场确认两副图约33px、client/scroll均292及按钮底428.5；本reviewer未重复现场操作。
- reviewer直接读取chart-controls-desktop-e2e-final.log，最终19 passed (22.6s)，包含新增几何用例；git diff --check通过。CSS小改不重复已独立通过的35项逻辑短测。
- 最终独立审查结论保持通过，无新增有效finding。主agent此前完整30项E2E为25通过5失败；全量unit缺acl manifest、全局lint5告警，以及原完整终端目标尚未完成的限制均保留，不把专项19通过表述为全回归完成。

## 提交前事实核对

- reviewer直接读取最终完整E2E日志：31项中26通过、5失败，52.3s、退出码1；最终生产构建日志包含Vite成功与release WASM verified。此前19项专项全部通过，与工作表记录一致。
- terminal-fidelity-plan明确区分周期跨屏保持与局部MA/viewport跨实例未保存，记录Vite配置触发重载及重建暂停局，不冒称保留旧日内状态；已记录暗色量能/KDJ标题与手机资金布局等后续项。DESIGN副图40px目标/24px最低/蜡烛80px说明与CSS一致。
- git diff --check通过。最终源码及文档复核无未解决finding，可以按既有授权提交本批；仍不得宣称全回归或原完整终端目标完成，完整5项E2E失败、unit生成文件缺失、lint5项限制继续披露。
