# 分时居中参考轴与信息菜单独立复核

2026-10-05。只读复核HEAD461bf29后当前12个tracked文件与新增intraday-reference-axis.spec.ts、mobile-info-navigation.spec.ts；未参与实施，未build、写dist或提交。两个需求分别提交合理；需按各自范围拆分DESIGN/UX。

1. 大 A 语义：昨收0%是参考轴，未生成成交样本；对称边界为显示刻度，并非权威高低成交价，UX已明确。DesktopIntradayChart继续计入volume>0的当日OHLC，零成交占位排除；参考线固定50%，中心刻度直接昨收，避免浮点负零。连续量、竞价累计量、null参考值及竞价粗点资格不变。K线和原始Cents不变。此为用户最新显示要求替代旧贴边规则，无新交易制度假设。信息菜单删除四个无实际内容入口，保留财务CompanyPanel、盘口FiveLevelBook、资金权威统计，不删交易功能。
2. 必要范围：桌面和迷你图复用已有symmetricIntradayScale，未另建尺度算法或状态；手机原居中实现不改。MOBILE_INFO_TABS同时定义类型及可见列表，状态owner和默认资金不变，删除不再使用的占位CSS，范围小且直接对应需求。
3. 边界与测试：桌面更新的是新坐标契约的具体期望，没有移除权威极值、零成交、重新渲染、null、固定时间槽、量能、接缝与无连续粗点覆盖。新增单边上/下、空、唯一点及OHLC变化中心断言；迷你图验证不同高度、原数组不变及固定分钟位置，手机追加原中心契约。状态测试由已删除资讯替换合法财务，仍验证证券、周期和信息状态互不复位。新增菜单E2E精确三个入口、默认资金、Home/Enter进入财务、周期及返回保持；轴E2E检查两端及返回桌面。未发现有效finding。

独立从apps/web执行desktop-intraday、market-model、mobile-ui-state三文件，显式concurrency3、case和进程树deadline均10000ms，53/53通过291.76ms。该调用前两条只读命令误用了根目录相对路径，sed/tail报告不存在，随后从正确目录重读；不计作验证成功。未重复浏览器或构建。最终浏览器、生产构建和现场证据待主agent补齐；本记录不关闭旧财务gold或整个终端目标。

## 信息菜单键盘补丁复核

主agent报告初次相关E2E为3/5通过、2/5因Home不能聚焦财务失败；这揭示原moveTabFocus只实现左右键，不能记作浏览器已通过。当前补丁在同一函数加入Home/End，仍仅focus，不调用onInfoTabChange，Enter沿原生button click激活；三项菜单与查询到的按钮顺序一致。左右循环公式保持，防止Home/End滚页面的preventDefault只对支持键生效，其他键保持原行为。无A股数据或交易命令变化，也没有另建键盘状态。

新增E2E保留原进入内容、周期与返回状态断言，追加Home/End、左右首尾循环及Home后盘口仍selected，确实检查手动激活契约，未以删除失败断言绕过问题。完整最新diff无新增有效finding。此时重跑结果尚未提供，静态通过不等同浏览器或全量通过；最终验收待后续补齐。本轮未重复测试或构建。

## 最终验收复核

已读取最终日志：相关E2E修复后5/5通过8.7秒；完整Web主agent汇总841/841，日志155文件、8分片、wall2609ms；完整E2E63项62通过、1项仍为company-information原净利gold，exit1，未修改原精确金额断言。production日志built in 1.74s、release WASM verified，与主agent报告tsc成功一致。变更文件lint排除既有SSR两条告警的口径明确，全库lint仍5条原有告警exit1；premium无finding和diffcheck通过不能代替全局lint全绿。

最终UX已直接替换旧贴边坐标及信息占位描述，键盘契约明确只移动焦点、Enter/Space激活，与当前实现一致。主agent实际IAB新局09:34:48暂停1x：002156桌面中轴y50及对称±10.02%，手机282px价格图中线141px，原留白使范围±10.42%。这两端显示范围差异符合各自padding，未改变0%居中；现场证据不冒称本review独立浏览器观察。源码HMR重建host的事实明确，不宣称保留前批09:48:49。

最终结论：两项需求的当前完整diff独立门禁通过，无未解决有效finding。保留初次Home失败及修复记录，不宣称全部回归通过；旧财务gold及整个终端目标的其他边界仍开放。未追加测试、构建或产品修改。

最终仅工作文档增量已复核：requirements-completion-audit撤下旧贴边要求并明确手机留白未统一；terminal-fidelity-plan与mobile-info-scope-validation记载的5/5、841/841、62/63旧gold失败、production成功及全库lint5告警exit1，与已核实结果一致。HMR重建host、09:34:48新状态及截图范围均明确，没有声称延续前批局进度或截图含未显示区域。无新增finding；可按已授权计划分别提交两个产品需求，整体goal边界继续保留。
