# UI checkpoint batch 2 独立复核

## 范围与结论

对照 `.tmp/company-system/ui-review/batch-2.txt` 中 30 个路径，逐文件读到 EOF，并检查每个 tracked 文件相对 HEAD 的完整 diff；untracked 文件按当前完整源码审阅。未检查 Cargo 或 index 类文件，未修改实现。审核主题包括本地/共享市场账户隔离、公司 Simple 参数与 seed 草稿、日历及分钟 K 线、均线偏好、指标来源、存档/交割历史展示、移动端账户控制、格式化及关联测试。

结论：未发现可确认的 P1/P2。当前改动中检查到的账户金额使用元、股数/成交量使用股或手、沪深证券代码与交易时段展示没有发现明显跨层语义漂移。Simple 配置 UI 明确说明是虚拟简化模型，不冒充真实市场统计；市场图表按公历周期聚合并保留精确 wire 金额。此审核不构成对清单外文件、官方交易制度依据或整个产品实现的复核。

## 逐文件读取证据

行数为读取时 `wc -l`；SHA-256 为读取时完整当前文件的摘要。每个路径的源码均已读到文件末尾；所有 tracked 项另读了 `git diff HEAD -- <path>` 至 diff 末尾。untracked 项无 HEAD 对象，按完整新文件审阅。

| 文件 | EOF 行 | 当前 SHA-256 | HEAD 状态 / diff scope |
|---|---:|---|---|
| `apps/web/src/app/LocalRefreshViews.tsx` | 315 | `964f65a683016ee36ae6c8d9d43a605db00edb3061cb5cb9017d06e195a36e76` | tracked；本人账户选择、图表历史/交割查询与图表指标源接线、控制权限 |
| `apps/web/src/app/StartupScreen.tsx` | 44 | `a499c75aceb6c6ad5e1603e4e543cbde97931e109ff134af98856a1cec5ab525` | tracked；启动文案和本地创建设置入口 |
| `apps/web/src/app/local-amount-render.test.ts` | 230 | `4b112a8a4ed16e44dbfc6d5943dd97dff7e957041a85bfe940b183b38ca61b3e` | tracked；SSR 账户归属、金额/数量单位和盘口渲染断言 |
| `apps/web/src/app/portfolio-selector.test.ts` | 34 | `0bde0810fe38a083b4d3315594381b141987a4bdfa4d93fcaeb241988a90fb2d` | tracked；修正测试 state fixture，覆盖无行情持仓拒绝 |
| `apps/web/src/app/quick-trading.test.ts` | 161 | `ca4669fd24aca3206b55211b3ea72131b494c30b9acc53cc6c379f981f0829e6` | tracked；本人非零账户快捷委托测试及 fixture 账户端口 |
| `apps/web/src/app/remote-logout.test.ts` | 16 | `df487a191aa688f2a3af18d690dda6fa9f326ed5421b5c5c1491814943cd38de` | untracked；完整新文件，退出与错误传播测试 |
| `apps/web/src/app/session-control-commands.ts` | 65 | `f3c0b809a2ac1eb104e19d6fd0a359f07c8510d32a12fea3a1785f48b747deb4` | tracked；按 generation 防止旧宿主动作污染新会话，远程跳过可见性暂停 |
| `apps/web/src/app/usePausePreferences.ts` | 78 | `163b35d00418c763c3d5c2a53953930afb1868c7f1da0270c4afd3800521842c` | tracked；偏好异步同步增加 generation 守卫 |
| `apps/web/src/auth/credential-store.test.ts` | 95 | `551beb838b77143f7e2753378ebbe78336889c662fb9e20d791302887bfe8c64` | untracked；完整新文件，credential 隔离、事务与取消测试 |
| `apps/web/src/components/ChartPeriodTabs.tsx` | 31 | `619ccbf89d7ce91304e7766392ae720c92c5fd8868d4d89cc932388d3753ba8c` | tracked；五日周期和周期 selector 入口 |
| `apps/web/src/components/MarketKlinePanel.test.ts` | 127 | `94ea9df60f381373d2a8b32ee24d0a3dfd0a5435dce01dee55ef2da51e43f9fb` | tracked；自定义均线设置与自然季/年 K 渲染断言 |
| `apps/web/src/components/MovingAverageSettings.tsx` | 23 | `649758844a4c169c65feba61ad62f5362174da81dfae38534da754e57adf3168` | untracked；完整新文件，均线周期/可见性编辑 UI |
| `apps/web/src/components/RetainedHistoryPanel.css` | 8 | `d16212f26c2594488cf0a46b5157e84f34e663a52fff258982ff95525536cc07` | untracked；完整新文件，历史面板样式 |
| `apps/web/src/components/company/CompanySystemInput.tsx` | 49 | `f9a240321084fd82119d458e20a3323f148b13f8ad74e5f0e7e504115ff265e9` | untracked；完整新文件，Simple 公司参数、seed、周期与单位说明 |
| `apps/web/src/components/company/company-system-config.test.ts` | 52 | `2b0711519f812f31f4408e7d9677961a5923a2445aecb177618b83bc1b2d8de3` | untracked；完整新文件，Simple 配置校验和预设测试 |
| `apps/web/src/components/company/report-availability.test.ts` | 14 | `6af56cd94c46b05759145813d44c8d743e2a4f38a791feefe88bf0c98bb48979` | untracked；完整新文件，自然月末与不可用原因测试 |
| `apps/web/src/components/indicator-source-policy.test.ts` | 24 | `3eea7d134223430f5b325e217e0aff2e1565423c4567353db835ff286cc0c50b` | untracked；完整新文件，指标源不支持时不回退测试 |
| `apps/web/src/components/minute-kline-model.test.ts` | 98 | `8529372227c2f85f1d1d9868c03d06b4381816ff9157cab99e70a9284f496b10` | untracked；完整新文件，分钟 K 聚合、时段和整数范围测试 |
| `apps/web/src/components/moving-average.test.ts` | 15 | `be9e7fcb7818ec974d6f18b72884b3701fb9455c5eaa80cd41fd0ca688ee61ce` | untracked；完整新文件，精确分均线测试 |
| `apps/web/src/components/player-orders.ts` | 69 | `eaf6c2cbbf4871a7f11c40a272e42dbd83f55e5678c186d91735b4a88522b46c` | tracked；PlayerWorkingOrder 类型共享和 AccountID 字符串语义 |
| `apps/web/src/components/retained-history-model.test.ts` | 30 | `9b7c889d7183acfe61ef905c0de4104a082acf4cdef64ef39799254c85b861cf` | untracked；完整新文件，五个已结束交易日/分页测试 |
| `apps/web/src/components/useMovingAverageSettings.ts` | 43 | `185a7e03af081af057fd4cbcf53ef4b6d40fc172c8de3dfa66a69551ef64157d` | untracked；完整新文件，本地 MA 偏好读取、保存和错误呈现 |
| `apps/web/src/config/company-initial-preset.ts` | 65 | `c52d4ffb0a93261748597c355321e5ade4d149ddbba62566f3731d38d61f1530` | untracked；完整新文件，按 seed/周期生成虚拟期初财务 |
| `apps/web/src/config/seed-draft.test.ts` | 59 | `b313fcfe237cb0c75b9ba75286845ead71897fe74acb30e70db2110afa556a1e` | untracked；完整新文件，seed、周期切换和自定义草稿保护测试 |
| `apps/web/src/mobile/MobileRunToggle.tsx` | 24 | `ef71fb283bc49ef96ce27eed29ae753fb7adb6fab2b8c4d1e312fc0de40830a1` | tracked；新增 disabled 控制状态 |
| `apps/web/src/mobile/calendar-candles.ts` | 52 | `28ef23b1d14d12036d2eca1336f6438447c89fe517023bafd6407b414103d9de` | tracked；按公历季/年聚合并传递权威成交统计 |
| `apps/web/src/mobile/mobile-component-render.test.ts` | 305 | `e61c453dede28b762edfbd66e0164d6393a8770988752cc7725a08cafdae2e88` | tracked；指标源能力、成员控制、盘口单位及移动图表渲染测试 |
| `apps/web/src/store/chart-settings-slice.ts` | 50 | `1198bb04b3a4f11ee1c0c5a57b91f8857993d22c1e36e14ea57bf9a94ce6b2a8` | tracked；动态 MA 周期偏好写入 Redux |
| `apps/web/src/store/remote-membership.test.ts` | 30 | `6bfbdab6fcaf396ed27bf654c928f13ed7eb40698a3f73a34a89008595263431` | untracked；完整新文件，本人 AccountID 选择与 baseline 等待测试 |
| `apps/web/src/utils/format.ts` | 191 | `c5d95105ced61812bef8258d9d43a605db00edb3061cb5cb9017d06e195a36e76` | tracked；新增 ExchangeClosed 拒单文案 |

## 复核记录

- 行情/账户视图取本人 AccountID；缺席本人时给出显式提示。持仓仍按股展示，可卖量继续扣除 T+1 锁定及预占股数；资金以元展示。
- `ChartPeriodTabs`、`calendar-candles.ts` 与分钟 K 测试体现自然日历周期和 A 股午间时段边界；未发现把游戏日序冒充公历日或把分钟历史从日 K 合成的行为。
- 均线配置拒绝重复/非安全整数周期；指标来源测试明确不支持时不静默换源；凭据测试不把秘密写入错误信息。
- UI 明确称 Simple 为虚拟简化模型，标示金额单位与 Simulation 未实现；对未实现功能没有发现伪装成真实公司财务或交易制度的明确问题。
- 没有运行测试；本任务是独立 diff review，且委托说明上游已有 typecheck/Node 验证结果，本记录不重复宣称执行过验证。
