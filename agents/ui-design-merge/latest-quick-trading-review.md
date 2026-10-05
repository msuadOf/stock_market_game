# 最新快捷交易界面合并独立复核

## 范围与证据

非作者复核 `6c42c69..0110e9db` 的全部 production 增量、对应短单测、E2E 断言变更、正式交易契约和参考观察记录。基础界面校准已合入 `2c3b3bb6`；本记录仅评价新增快捷交易和此次整合，不将旧分支完整回归结果作为当前 main 的证明。未检查截图像素，不执行复杂回归、Cargo 或网络操作。

`QuickTrading`、`QuickTradingPanel` 和 `QuickTradingContext` 共享草稿、资源预检查及自动任务；Desktop 与 Mobile 只调整呈现方式。已逐项核对真实盘口价格和累计股数、手到整数股转换、合法余股、T+1 和卖单占用、买入费用和跨证券待提交预留、板块单笔数量、symbolic 限价、游戏时间自动任务、暂停及重新同步、本人全证券撤单和保留自动任务。

新增代码未改变 engine 制度与费率。最小／最大仍发送 `Lowest`／`Highest`，不冒称市价；B1／S1 无真实报价时显式不可用。比例按钮仅填整手，不删除手工合法零股能力。请求提交反馈没有冒称受理、成交或撤单成功。日终持久化仍经 `DayEndPersistence.completed`、明确 reference 和 `validateDayEndCandidate`，没有增加日内保存。费用来源访问失败、统一市价和自动采样精度等既有简化保留诚实说明。

独立复核发现的可用现金显示 P2 已修复：`QuickTrading.availableCash()` 统一以 BigInt 精确扣除权威 `reserved_cash` 与本地全部待受理买单预占；买入 `capacity()` 和面板金额共用此结果，UI 明示“已扣待受理买单预占”。新增单测覆盖大于 JS 安全整数的精确分、跨证券额度和失败释放；SSR 用例核对 `1994.99元` 与可买 `0手`。

本人独立执行 `quick-trading.test.ts` 与 `symbolic-limit-order.test.ts`，初次 15／15 通过；上述 P2 修复后重新执行当前版本，16／16 通过。两次均 Node 并发 2、case 和进程树 deadline 10000ms，分别约 0.29／0.31 秒。覆盖费用极大整数、盘口价量区别、草稿隔离、零手、合法余股、暂停期间异步任务、跨证券预留和旧会话撤单停止。root 报告定向 22／22 与 28／28 短测分别约 1.8／1.95 秒通过，最终类型检查 session 33741 实际 exit 0、无诊断，日志 `.tmp/ui-design-merge/latest-web-tsc-final.log`。这些证据不证明像素布局或多人集成已完成。

## 整合门禁

- 当前审查基线仍是单玩家 `AccountId 0`，与现有 App、LocalRefreshViews 一致。另批多人代码被 root 暂存，恢复后必须让 controller 与 Panel 使用真实本人账户；不能把非零玩家拒为无账户，更不能使用其他玩家资金。该发现已交 root，列为恢复暂存后的融合门禁，不误报为当前单玩家基线回退。
- 正式交易文档已恢复自然周／月口径，不保留每 5／20 根选项；季／年、分钟周期和可编辑 MA 仍明确为后续实施，不把需求登记冒称已经完成。UX-CONTRACT 保留 main 真实累计成交额／股数的 VWAP 契约。
- 最终暂存增量已复核：`App.tsx` 的 `hostRef` 仍由外层生命周期持有，`activeSetupRef` 只供稳定回调读取配置，股份分配接线保留；`LocalRefreshViews.tsx` 保留图表历史查询 `useEffect`，只移除旧表单 owner 与不再使用的 import。工作文件无冲突标记，index 的三个 `UU` 已清，`git diff --cached --check` 通过，类型检查与相关短测门禁已关闭。

## 结论

当前单玩家基线的快捷交易增量及合并整合限定 PASS：有效 P2 已修复并独立复核，未发现新的有效 P1／P2；改动为用户要求合并的 UI feature 所必需，未见无关 engine 修改或弱化既有领域断言。恢复暂存后的多人适配仍须单独关闭门禁，不据当前 16 个短测宣称全 checklist 完成。
