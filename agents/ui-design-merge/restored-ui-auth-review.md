# UI/Auth 恢复暂存融合复核

日期：2026-10-06。审查基线 `cd595607`，审查对象为恢复 stash 后当前工作树中已稳定的 `App.tsx`、`StartupScreen.tsx`、`CompanyPanel.tsx`、`store.ts` / `remote-membership.ts`、`mobile-ui-state.ts`、`quick-trading.ts`、`QuickTradingPanel.tsx` 与对应单测。按主代理说明，不签核仍由叶代理修改的 `LocalRefreshViews.tsx`、`local-amount-render.test.ts` 和 MA 新实现；工作树其它合并冲突不作为本次代码发现。

## 结论

- `QuickTrading` 使用注入的 `account()` 获取当前本人账户；买卖资源预检查、现金、持仓和预留卖出量都从这个账户读取。缺席时显式失败，Panel 禁用下单与自动开关；新增非零账户测试证明不会借用 account 0 余额。A 股手数/整数股 Intent、费用、T+1、零股与类别限制沿用已复核实现。
- `remote-membership` 按成员 `accountId` 选账户，并以 `ready` 与快照 generation 拒绝旧 baseline；成员缺席返回 `null`，不回退账户0。AccountId 不存在于已就绪快照会抛显式协议/状态错误，不静默伪造账户。
- App 的市场控制权限与资金账户分离：公共成员可读市场；暂停/倍速/建局/日终读档/自然日推进和报告更正按 `canControl` 守卫。无账户成员需本人明确 rejoin；会话同步或成员/generation 变化会重置 QuickTrading epoch、自动单及活动委托本地视图；disconnect 清理 context 订阅与会话草稿。远程读档、文件读档和重置有全市场影响确认。桌面 QuickTrading 双向面板仍是唯一快捷委托 owner。
- ArchiveManager 从宿主声明的 browser repository 或 archiveStore 打开；远程控制路径先守卫并确认再调用原存档命令。页面声明存档/登录身份独立，未见把登录凭据写入日终存档的接线。
- 财报更正控制只向获准控制者下传，组件 key 包含 timeline generation 与公司 ID，切换时间线/公司时重建查询面板；交易报告频率草稿从 startup setup 初始化，并随远程 market context、存档命令端口同步。
- Q08 source gate：Desktop 和 Mobile 调用点传递实际 source 与宿主 capabilities；`resolveIndicatorRoute` 在 Rust 能力缺失时走明确 unsupported，不改用前端结果。Mobile MA 的现有渲染断言仍检查 MA5/MA60 真实曲线及历史不足提示；未审查正在改动的 MA 算法/日期组织。

## 验证

- 定向单测：`quick-trading.test.ts`、`remote-membership.test.ts`、`app-startup-wiring.test.ts` 共 22/22 通过，Node case timeout 与进程树 deadline 均 10 秒，耗时约 0.14 秒。
- `mobile-component-render.test.ts` 单独从 `apps/web` 目录运行，21/21 通过（约1.54秒），覆盖移动 source gate 与 MA5 历史不足提示。
- 一次将 `MarketKlinePanel.test.ts` 与 mobile renderer 同进程运行的尝试有 MA 测试失败，错误集中于图表 fixture 的 `rawPrices` 缺失及 MA 消费路径；当前 MA 正在另批修改，本审查不据此对新 MA 实现签核。运行路径/fixture 在分开运行 mobile renderer 后通过。
- 未运行 E2E、Cargo、完整回归；未更改 index、提交或处理其他冲突。

## 待最终复核

等待叶代理冻结 `LocalRefreshViews` / `local-amount-render` 查询接线，以及 root 结束剩余冲突后，再对整批最终工作树复核。当前结论只覆盖本文件开头列出的稳定 UI/Auth 文件组，不表示 573 文件融合或 Git index 冲突已完成。

## LocalRefreshViews 最终追加复核（2026-10-06）

按 root 要求复核冻结的 `LocalRefreshViews.tsx` 与 `local-amount-render.test.ts`：

- `selectPlayerAccount` 用于资产、持仓、股票列表、移动详情和市场筛选；缺席账户显示明确 notice，不呈现其他账户或伪造零资产。用户界面上的全市场偏好/存档操作由 `selectCanControl` 禁用。账户变更或 generation 变更使 ChartPanel/MobileDetail 子树和私有历史 `scopeKey` 失效。
- 财报更正 control/generation 正确透传至桌面、移动公司面板；自然周期历史调用仅对 `calendarCandlePeriod` 有效周期发起，五日、分钟周期分别路由到 retained-history/MinuteKlinePanel，不再错误画成日K。日期历史与五日查询带 scopeKey；MinuteKlinePanel 当前活动分钟用必需 runtime action，按 tick 更新并校验返回证券/日期/实时标志，查询失效由组件序号和卸载边界处理。
- 个人交割确认入口在查询函数内检查当前 AccountId，ChartPanel/MobileDetail 也按 generation/account key remount；市场分钟历史查询在 runtime action 内复核 host、generation 与 AccountId。没有发现把他人交割或资产用于 UI 的跨账户路径。
- 有效发现 P2：日期交割历史的 `PersonalTradeHistoryPanel` 被无条件渲染，且本地 query callback 只检查宿主与能力，没有检查当前 AccountId。缺席成员仍可展开日期表单并点查询，届时才依赖 host/backend 拒绝或返回错误；这与其他私有调用方先检查本人是否已加入不一致。建议在 LocalRefreshViews 对 `accountId === null` 显示“尚未加入市场”并不挂载查询面板，或显式传 `enabled` 并禁止查询。位置：`LocalRefreshViews.tsx:204`（桌面）和 `:311`（移动），查询动作：`MarketRuntimeProvider.tsx:82-89`。未观察到后端数据泄漏，但前端 private caller 边界不完整。
- SSR 测试明确覆盖 account null 不显示别人资产/零资产，非零 AccountID 资产及 CanControl 管理权限、跨股票 pending cash、元/股单位和低价格式；12/12 单测通过，deadline 10秒，约1.91秒。测试当前没有覆盖上述未加入时日期历史入口的禁用/不查询行为。

## 缺席账户历史入口修复复核（2026-10-06）

- `LocalRefreshViews.tsx` 已在桌面与移动视图中仅对 `hasPlayerAccount` 挂载 `PersonalTradeHistoryPanel`；缺席账户显示“尚未加入市场，无法查询本人交割历史。”公共市场历史入口不受影响。SSR 用例现已覆盖桌面/移动均无私人日期表单且有明确提示，前述 UI P2 关闭。
- `MarketRuntimeProvider` 的 `queryMarketHistory` 与 `queryCurrentMinuteHistory` 在 await 后复核 host、snapshot generation 与当前本人 AccountId，未发现切换后接收过期响应的问题。
- `queryPersonalTradeHistory` 仍只复核 host identity，没有调用前拒绝 `AccountId === null`，也未在 await 后校验 snapshot generation/AccountId。UI 已不再给缺席账户暴露入口，但 runtime API 边界仍开放，应由 Provider owner 补齐 null-owner 与 generation/identity late checks；本项保持 open，不签核该 API。
- 验证：在 `apps/web` 执行指定 Node 短测 `local-amount-render.test.ts`，12/12 通过，耗时约 1.84 秒；未运行 E2E、Cargo 或完整回归，未更改 index。

## Provider 私人历史 API finding 关闭（2026-10-06）

- Provider owner 冻结修复后，复核 `private-history-query.ts`、其测试与 `MarketRuntimeProvider` 调用接线。`queryPrivateHistory` 在调用 Host 前要求 host、generation、本人 AccountId 均有效；成功和失败路径都在返回/传播前复核三者未改变。AccountId 按 opaque decimal string 比较，未转为 JS `number`；账户缺席时不调用 Host；上下文未改变时保留 Host 原始错误。
- `queryPersonalTradeHistory` 已接入 helper，Provider 从 store 的 snapshot generation、`selectPlayerAccountId` 和 `hostRef.current` 读取实际上下文。独立复核记录 `agents/retained-history/private-provider-independent-review.md` 报告 PASS；本次亲读源码/测试后同意关闭前述 API finding。此次未扩展为对 engine history 实现或 Remote 授权的签核。
- 未运行额外测试，遵循请求不跑完整回归、Cargo、不改 index；该 helper 的 3 个短测试由 Provider owner 提供的独立记录说明已通过。

此追加仍不审签共享 K 线 MA 定制算法；既有移动 MA5/历史不足断言在前次独立移动组件测试中通过。该发现已发给 root，待修复后复核。
