# 手机范围标题与玩家拒单通知范围

2026-10-05，基线 d22de32。用户要求继续完成同花顺层次及股票模拟器看盘交易的细节；本批处理最终实看时发现的两项反馈漂移，不修改交易规则或账户身份。

## 问题、唯一 owner 与 TDD

IAB 原会话第1日09:27:19暂停，从自选入口切范围为全部后，五只股票及范围按钮已更新，顶栏仍自选，保存 mobile-scope-header-before.png。mobilePrimaryTitle 过去仅读取入口 mobileTab；现在同时接收既有 securityBrowser.view，对行情/自选列表使用实际范围和 SECURITY_LIST_VIEW_LABELS。交易/持仓账户页/我的仍保持自身标题，不新增状态、不改变底栏入口或过滤规则。

同一旧画面已有“委托被拒：000812 - 09:25-09:30 不接受新委托”，本批前没有新增玩家委托。protocol.effects 原来把所有 IntentRejected 作为玩家通知。根据 GameSession 既有玩家 account 0 和 docs/architecture.md 对普通业务拒绝与 StepFatal 的区分，现只投影玩家拒单 notice；先执行原 rejectionMessage，未知原因仍抛错。NPC 事件和 facts 未删除，全部游标、重试、成交与自动委托投影保留；所有 SettlementError 仍显式显示，包含非玩家账户故障。没有按设备或宿主分叉。

两个新增单测先得到有效行为红：范围 watchlist 期望自选、实际模拟行情；NPC 拒单实际多出一条玩家 notice。日志 feedback-scope-red-unit.log 为11通过2失败。实现后首次12通过1失败属于新测试误将 canonical facts 顺序当输入顺序；规范本来按 factIdentity 排序。修正预期为同一 canonical 顺序，仍逐字段精确比较所有 facts，不删或弱化事件保留断言。该次失败保留 feedback-scope-green-unit.log，最终13/13、116.30ms，case及整命令进程树均10000ms。

## 验证与适用范围

完整 Web 849/849、155文件、8分片、wall2371ms，命令树与case10000ms。新增真实release WASM浏览器case覆盖320顶栏、自选→全部5行→持仓空态、1440→320保持、我的/持仓账户页及行情入口恢复全部；完整65/65、44.0秒、workers3/RAYON10、共享外部300000ms。本次浏览器CPU采样发生在进程已结束后，没有获得live样本，不宣称测得多核饱和。完整批次结束后独立 production 成功，具体Vite耗时与WASM校验见 feedback-scope-production-build.log。

变更六个源/测试文件 oxlint exit0，strict premium0finding、git diff --check通过。完整 lint 仍五条既有 children-prop告警exit1，不顺带修改无关测试，不能宣称全库lint通过。

IAB 源码HMR回到启动页，已重新选择本地并启动，不冒称延续09:27:19；新会话第1日09:15:51暂停1x，未提交玩家委托、改名单或读私人档案。320下自选入口切全部，标题模拟行情和5行一致；切持仓标题/空态一致，902横屏保持持仓股票。截图 mobile-scope-header-all-after.png、mobile-scope-header-holdings-after.png；现场只验证标题，不把新局早盘无通知冒称经过09:25的NPC拒单现场证据。NPC反馈的完整协议验收来自新增短测，三个非玩家account各包含NPC拒单/玩家拒单/非玩家SettlementError/成交，校验事实、游标和重试保留；既有真实WASM玩家拒单E2E在65项回归中继续通过。

本批 Independent review 必须另建全新 gpt-6.1-sol high；记录待该reviewer写入，不沿用前批结论。整个终端goal须在需求审计与最终复核后才能关闭。
