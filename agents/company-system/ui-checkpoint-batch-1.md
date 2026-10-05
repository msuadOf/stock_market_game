# UI checkpoint：batch-1

- 复核基准：HEAD `8d07a77956e21f8540ebe8e38ae9e8db69c134a6`；2026-10-06。
- 范围：`.tmp/company-system/ui-review/batch-1.txt` 的30个文件。每份 current source 均逐行读到 EOF；tracked 文件另逐份核读 `git diff HEAD`，untracked `RemoteLoginScreen.tsx` 已全文读取。大型 `App.tsx` 按连续范围 1–180、181–360、361–520、521–760、761–977 读取 source，并分段核 diff。
- 当前 hash 记录于下表；长度为完整文件行数，EOF 即该路径最后一行。
- 结论：未发现 P1；首轮报告的 1 项 P2 已在当前冻结版修复并关闭。未改实现，未运行测试（本 checkpoint 为只读独立复核）。

## Findings

- **P2 已关闭 — 公司切换时短暂展示前一公司的可用性查询报表**：当前 [CompanyPanel.tsx](/data1/baiyifan/workplace/stock_market_game/apps/web/src/components/company/CompanyPanel.tsx:61) 为 result、error、loading 与表单分别保存共同 owner `JSON.stringify([companyId, timelineGeneration])`。每次 render 同步更新 `currentAvailabilityOwner`，旧 promise 的成功、失败与 finally 均要求 owner 和 sequence 同时匹配；render 先按 owner 屏蔽旧结果、错误、loading 与输入值，effect 再清理并安装新 owner。因此公司切换首轮 render 到 effect 之间没有跨公司数据窗口。更新后的 [company-report-selection.test.ts](/data1/baiyifan/workplace/stock_market_game/apps/web/src/components/company/company-report-selection.test.ts:178) 覆盖同 generation、effect 前切换公司时旧请求迟到成功和失败，以及已完成旧公司结果在切换首轮 render 隔离。经当前 diff 与 source 定向复核，关闭本项；另有 `review_panel_generation_fix` 完成 owner-pair 复核。Root 运行定向 deadline 验收，记录为 7/7 通过（0.872s；`.tmp/checklist-wave4/host77-panel-owner-pair.log`）。
- 大 A 语义：此项不改交易制度、单位或财务数据，仅隔离查询结果主体；本次确认的 UI 用语没有引入交易规则变更。
- 必要性与范围：只有此项是 P2；其它检查过的相关改动对需求范围保持一致，未提出 P3/样式偏好或额外 tooltip 要求。
- 边界检查：新增同时间线公司切换 before-effect 请求/旧结果覆盖用例，补齐原缺口。测试由作者加入；root 实际执行定向 deadline 验收 7/7 通过。

## EOF 与 SHA-256

| 文件 | EOF 行数 | SHA-256 |
|---|---:|---|
| `apps/web/src/App.tsx` | 977 | `6c3805922e01381c32ab9a0c4121fe2fed082b0a4cf16be95470c70aa6074e32` |
| `apps/web/src/app/RemoteLoginScreen.tsx` | 193 | `99f932d5b7f198c712054269695cdce321c32029b333d403bfdb764fca07f7a6` |
| `apps/web/src/app/company-config-commands.test.ts` | 103 | `74e3fa2db2f75beb49d9cde350b429c3b0e15575f918a99da4f621c50df595db` |
| `apps/web/src/app/money-wire-ui.test.ts` | 38 | `d92dddbd69dd1c8193b80cf416541192a1990fc835bd0e3a4e15ae1ab695ce43` |
| `apps/web/src/app/private-history-query.ts` | 15 | `2babb2cc8445d69537651a4b613bf9e6e9bcc922103dec19b08d401fa53d3da2` |
| `apps/web/src/app/remote-login-screen.test.ts` | 10 | `e7d7b2350a856ff086e5d43f4d87c1213106c2d6b9113fbb4250cc61f7abd3ac` |
| `apps/web/src/app/session-control-commands.test.ts` | 53 | `3c8c174ea32c156623ed797cec61f690eff7b4d09a15674a956c50b5e8725678` |
| `apps/web/src/app/trading-commands.test.ts` | 103 | `6b856b6620285e93493268158c7d0f7b9f0746795c83f1fee5e39c537733ce0d` |
| `apps/web/src/app/useTradingCommands.ts` | 212 | `b72ef0cba642fe47568659cbdab1abe34fd655d9c2e81ef0f2f9896110d06e7a` |
| `apps/web/src/components/ChartDisplayMenu.tsx` | 84 | `624dae417fd28d1c7a29ec7275e9427ff26ade5b6588526e545ff69f4016ea12` |
| `apps/web/src/components/KlinePeriodSelector.tsx` | 11 | `6b8199dc1e1a54ed543123af430771a04bcff3fc4fd5abdbcd7ca4903f13cf8d` |
| `apps/web/src/components/MinuteKlinePanel.tsx` | 183 | `b6c597e5f323f3e2328798600027c4290708f1c4c1ecd837b1073bc169bb75db` |
| `apps/web/src/components/ReportFrequencyInput.tsx` | 71 | `1825a8226ceabf457820e9c00ac87e2bcf6326b40972a3fa55f4b74cb2fe37f1` |
| `apps/web/src/components/company/CompanyPanel.tsx` | 180 | `86797039ab1388658a1025ef5c8908b8fd9b04d977686e2be633d601a8e4ea2f` |
| `apps/web/src/components/company/company-report-selection.test.ts` | 202 | `322ec7f1498050ccb47a4143da6347ac9f3ce38761d23a835738b22d28a2cab4` |
| `apps/web/src/components/company/public-report-fixture.ts` | 65 | `c3111742caae02c15206dc8534abceee56ee445bbc824ddd19edd478cc5d40f5` |
| `apps/web/src/components/desktop-intraday.test.ts` | 145 | `87fb145337a00e819d39fc805d06cab4c3533883b5cbdd9e20196064d2723084` |
| `apps/web/src/components/kline-periods.ts` | 21 | `535a545d870a4032f616ee2a1a2a3b90ebb4179a0fc44d96eed7eaa462774b4b` |
| `apps/web/src/components/moving-average-settings-ui.test.ts` | 164 | `3905e016dff92d3703aa58f064b0dd716238da6df8c066e7785b0d06e58b93d3` |
| `apps/web/src/components/player-orders.test.ts` | 95 | `99e377d76b2e0afedce6833976239d8c0160fe6006926f5d058d0787f3060fa9` |
| `apps/web/src/components/report-frequency-input.test.ts` | 125 | `6210d6e76c4fc77e6cb231e70ad337080e22c33a614df4308fe4dc18b4b83127` |
| `apps/web/src/components/trade-confirmation-table.test.ts` | 50 | `a048dfad3cb20cf18e3181c9404a82f03031d8ff8df765388dc418d6172bbba1` |
| `apps/web/src/config/company-initial-preset.test.ts` | 34 | `6516d959949f9fcc11c5ce3974044564a3c07a2764461b00ee07236c163cf2fb` |
| `apps/web/src/config/moving-average-settings.ts` | 25 | `8626e5b4d4f057f75663b9c46471129655d587f38553daaf7aa7ed9377ed0499` |
| `apps/web/src/dev/npc-decision-inspector.test.ts` | 52 | `11b84886c7eb979f8f55bbea5e539a5b216bf8674ccb9c1f35d670f78bc884e3` |
| `apps/web/src/mobile/calendar-candles.test.ts` | 50 | `1136889ce1cb2ad64d57089d4c01fea65a037538a99ba0e00e1c6c0fa2662879` |
| `apps/web/src/mobile/mobile-color-consumer-ssr.test.ts` | 144 | `e77e35e5ac67cb2c5472396deb8d73f5d24c5a27e66cd8a334edff022717370e` |
| `apps/web/src/mobile/mobile-ui-state.ts` | 90 | `a82d8f0ab424d1e42c915142c7c3faa8a7836fc245bd71eab59cccbf2ad12fee` |
| `apps/web/src/store/portfolio-selector.test.ts` | 20 | `657076059bd2a333ab9ee7ca028dba2c7a7e4926dfbc5b74f1d007d7e2e16ae2` |
| `apps/web/src/utils/format.test.ts` | 62 | `a1bf9ec302d84a311bdf61b540b99350f4f12e2d77d8324435ea0cafdb8c0d43` |

## Scope notes

- `App.tsx`: new-game seed/config flow, remote identity and market controls, user-facing confirmation/permission state, report correction and connected UI wiring; no HTML `title` tooltip expectation added.
- `RemoteLoginScreen.tsx`, login tests, `private-history-query.ts`: explicit authentication, remembered credential scope, stale private query isolation.
- `company-config-commands.test.ts`, `money-wire-ui.test.ts`, `trading-commands.test.ts`, `useTradingCommands.ts`, `player-orders.test.ts`, `portfolio-selector.test.ts`: validation and account ownership/identity wiring; existing A-share quantity and T+1 checks remain visible in code/tests.
- `ChartDisplayMenu.tsx`, `KlinePeriodSelector.tsx`, `MinuteKlinePanel.tsx`, `kline-periods.ts`, `mobile-ui-state.ts`, `calendar-candles.test.ts`, `desktop-intraday.test.ts`: chart menu/settings entry, calendar/minute periods and real-trade rendering. No transaction-rule changes found.
- `ReportFrequencyInput.tsx` and its tests: explicit monthly schedule, next-month dates, delay and retained draft validation; text distinguishes game monthly reports from statutory reports.
- `CompanyPanel.tsx`, company selection tests and public-report fixture: public report availability/scope, timeline result isolation, correction access, fixture source marker.
- Remaining test/config files cover MA preference validation and synchronization, mobile rise/fall consumers, trade confirmations, initial company presets, NPC diagnostics, and display formatting.
