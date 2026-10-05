# UI checkpoint batch 3

## 审查范围

- 按 `.tmp/company-system/ui-review/batch-3.txt` 顺序完成 30 项逐文件审查；逐项读完工作区文本到 EOF，再对照 `git diff HEAD -- <file>`。新增文件读完整 current source；删除文件读完整 HEAD source 与删除 diff。
- `docs/principles.md` 已读。检查了 `apps/` 下的 `AGENTS.md`，没有额外文件。
- 不含 Cargo 或 index 文件。重点检查 UI、styles、tests、helpers 与 A 股概念、单位和阶段语义。
- 文件 SHA-256（删除项标记 deleted）：

| 文件 | SHA-256 |
|---|---|
| `apps/web/src/app/MarketRuntimeProvider.tsx` | `02f9b7a96d114cacea2a0f04032ec5d9bff4ab958a9928416612bc1cfca944c5` |
| `apps/web/src/app/app-startup-wiring.test.ts` | `7a51500f9ac07f80cc060e9e915f91e50189bad664b4e25291d0fc0fd1a3d6b8` |
| `apps/web/src/app/market-chart-projection.test.ts` | `4b3a2a1f8051e47f7b2cc93bf0f803265fa89f5218524bd4194bd089d2708318` |
| `apps/web/src/app/portfolio-selector.ts` | `cac2e09e019ed1bf6089d9bc1f346e99855d96326be58210f6d2ddd4ba630e8f` |
| `apps/web/src/app/quick-trading.ts` | `aab2c356bb45c1a5bb06e5bbd54a4f074938f710afbe7e622ae6ce19e1a0c335` |
| `apps/web/src/app/remote-logout.ts` | `676a14df7fda1c2747454297c2be7ef1035d4fc4b8602a599de7465cca6078b5` |
| `apps/web/src/app/session-host-lifecycle.test.ts` | `361d5cec8d2db7ab88136930439f2785b6cbe95673302a5e3f01db159f2f3977` |
| `apps/web/src/app/useSaveCommands.ts` | `4836ff51a31e69fb8361a7d2b46dd145fb6ea68bc0f1bb0d11a12b32b1b99780` |
| `apps/web/src/auth/credential-store.ts` | `4b986e2bfbc4d52632ab117b57eb318b5346c4e755a7841e8ba43c99adf30060` |
| `apps/web/src/components/DesktopIntradayChart.tsx` | `55cd001e50ab0afd0924f83b1233d09b495563de17c86b6c21652e57532e4aee` |
| `apps/web/src/components/MarketKlinePanel.tsx` | `3bc54402a983bab5898888d7df9eea79795a6f822076d363ba75f1346c9743ba` |
| `apps/web/src/components/PersonalTradeHistoryPanel.tsx` | `5c4649962e3bd5c8514365ef5df7b6441dc844caf7b7f87734a455cc41b293d9` |
| `apps/web/src/components/RetainedHistoryPanel.tsx` | `cc6a13fd8e6ed0752d50fa9be95f568f15e9733c7af150ed658733017167771d` |
| `apps/web/src/components/company/ReportCorrectionPanel.tsx` | `8d4f41df51f279c07fadf5cafa432758bb6122206c25448e64eda9586ee21451` |
| `apps/web/src/components/company/company.css` | `af2c5732dcc39bb70797e21dc8939fdb3156a6395f8c010b04b4e2406fa074c7` |
| `apps/web/src/components/company/report-availability.ts` | `6445ddfe1ef9def1f4578cb6d392f974b53de6c9221e486768c29676860d68b5` |
| `apps/web/src/components/indicator-source-policy.ts` | `029e5f91a55c843ab2f7d2df939871c1eaacf68d4e50c1a8f5645ec0539ca8c0` |
| `apps/web/src/components/minute-kline-panel.test.ts` | `168d3661e2a61d3750c29e6348641bf65bbd9a6a7f2abaa04217e8bc37d172ef` |
| `apps/web/src/components/moving-average.ts` | `2e5c25f6fb35f72cc1b539abde18504af929f1922078799224a643d1448df045` |
| `apps/web/src/components/price-chart-runtime.test.ts` | `cb1af61d45403c3d55c9868c64f7b4e0ce30ff6c5da197bfc5003595e984afee` |
| `apps/web/src/components/retained-history-model.ts` | `4b536e8ddfd82e4d82a0cd9f558edd63f9a91a146e95daf98f56715ad27646b1` |
| `apps/web/src/config/candle-aggregation.test.ts` | deleted; HEAD SHA `e8e49cb4` |
| `apps/web/src/config/defaults.ts` | `55ce3c3bb97e9ba2d4b7e7f268bfd462004a5503de5cb0875687935039aab548` |
| `apps/web/src/config/seed-draft.ts` | `bc97f58d3e8a82cac5c5f133642eab4da75738327e1d092acc1a996bf93f4729` |
| `apps/web/src/mobile/MobileSpeedSelect.tsx` | `8f37d3f45ebfa7bb08adafaae24ce63ae8c9b4dceb33650f2fdc5beb51913792` |
| `apps/web/src/mobile/market-model.test.ts` | `0c30710aa89ebb3995c871ad849afd5c255fb0ebdf672d7d990e31837441f533` |
| `apps/web/src/mobile/mobile-intraday-projection.test.ts` | `3351d790dc79e8df919edd49f3f2853b8ae7222753ba151c6da3e96a9db19dbe` |
| `apps/web/src/store/chart-settings.test.ts` | `a91dedfdc0a94aa8fa64da03c962a2e4557d5a9871033f02076df68f53ada73a` |
| `apps/web/src/store/remote-membership.ts` | `2ba877c257f2847d15e43363264b3ac69e7caae8d604b0e8774c4e695203e86e` |
| `apps/web/src/utils/turnover.ts` | `69d9de567d868897037c59f811499fd9a792d9d2ae829a7e4b2f6fbd2f2daf71` |

## 结论

- 未发现有效 P1/P2。当前范围的 A 股图表语义保持明确：连续分钟成交量与集合竞价累计量分开，竞价指示不冒充成交；红涨绿跌方向测试保留；周期 K 按自然周/月/季/年；均价从权威日累计成交额与股数计算，缺数据时不合成。
- 图表指标来源缺能力时明确显示 unsupported，没有静默切换来源；远程账户 selector 与 generation guard 不再假设账户 `0`。
- 种子/公司设定由用户预览及编辑并传入新局；Natural-day 归档和账户交易历史文本未发现概念混用。
- `candle-aggregation.test.ts` 已删除，HEAD 中完整内容审阅过；删除与本批自然周期语义移除一致。
- 按用户提供上下文，typecheck/Node 代表用例已通过；本审查未另行执行测试。未改实现。
