# Web 共享市场新增批次独立复核

## 范围与结论

- 复核者：review_full_web，未实施本批生产代码；唯一写入为本工作记录。
- 依据：根 AGENTS.md、docs/principles.md、ADR-0030、ADR-0033、docs/open-questions.md、docs/architecture.md；已读 remote-ui-wiring、account-id-web-wire、public-event-projection 工作记录。
- 逐个读取下列 tracked 文件完整 diff；首次命令输出截断的部分均通过分组或直接源码补读，不以搜索命中代替审查。新增文件逐全文读取，清单见下。
- 最初发现 1 项 P2，作者修复后再次完整审查新增 helper、tests、App 接线和启动页错误展示。当前指定新增批次无未修复有效 P1/P2。
- 不将本结论扩大到整个工作树：税务／月报／Archive 既有批次的同文件 hunks 已读取以防跨层漂移，但其官方领域依据、全平台运行与完整验收仍由 Root 原有专门复核负责。

## P2 发现与修复复核

原 App.tsx 的 logout 先撤销 Server credential，再无条件依赖 IndexedDB remove，最后才断开连接。在“不记住登录”且 IndexedDB 不可用／删除失败的合法环境下，Server 撤销已经成功，但 remove 抛错使断开连接永远不执行；UI 留在失效的 RemoteHost，重试 logout 只得到 401。

作者增加 app/remote-logout.ts 与两项短测；现 App.tsx:466 调用 logoutRemoteIdentity，revoke 成功后 forget 失败仍执行 disconnect，并显式报告残留失效 credential；revoke 失败不冒称退出；双重清理失败保留 AggregateError。App.tsx:870 启动页消费 notice，避免切回启动页后存储清理错误消失。独立复核确认修复最小，不改变市场、成员、资金或控制授权。

## 三项门禁回答

1. **A 股语义与依据**：本批新增身份／成员／控制接线遵循 ADR-0030／0033 产品契约，不新增交易制度。Money 保留分字符串、quantity 保留股、订单／seq／tick 仍为 number；AccountId 完整 u64 字符串不经 Number。公开 Trade 量价不变，不以匿名成交重建本人资金或交割；T+1、撮合顺序、费用在 Engine 保持权威。没有用默认账户 0 冒充缺席经济成员，没有 refresh/reconnect 自动入场资金。此范围不需要另定交易所制度，亦未冒称重新验证全部现行交易所规则。
2. **必要性与范围**：认证、独立 credential 存储、market context、generation 请求、成员 selector、owner-scoped removed、PublicTrade／PrivateEventOmitted 及 Save／WASM AccountId 根修都是多人共享市场与完整 u64 当前契约所必需。无旧档兼容、无 legacy/session token 或 local-player fallback、无 generated 手工补丁路径。登录库与经济存档介质明确分离。
3. **边界与跨层**：REST body/endpoints 与 Server 当前 routes 对照，WS credential 不放 URL；auth 错误不回显密码／token／Server payload。Remote start／dispose／隐藏页不修改共享 running、不删除市场；can_control 与 member 独立，缺席 Controller 可公共读取与控制。换 generation 先更新 context 再 baseline；同 generation rejoin 账户也等 baseline 才显示金融资产。OwnerScopedRemovedOrder 仅删匹配 owner/id，PublicTrade 保留匿名量价、PrivateEventOmitted 保留 seq 且无金融 effects；Civil refresh 不重放历史 effects。OpaqueSubjectId 的空格、__proto__、constructor 在 Save／serde normalization 保留，membership 与 NPC 集合对照 Rust memberships.rs。

## 独立实际验证

执行 9 文件定向测试，Node file concurrency 8、case timeout 10000ms、外部 run-with-deadline.mjs 10000ms 进程树监督：27 passed、0 failed，Node duration 431ms，命令 wall 0.51s。覆盖 logout 修复、credential、auth/context、owner removals/public events、membership、完整 AccountId、WASM normalization、本人 selectors。

首次 deadline supervisor 调用少了 -- 分隔符，立即返回 usage、没有运行测试；随后按正确语法重新执行，以上数字仅来自真实第二次输出。不运行 Cargo、network、完整回归或 tsc；Root 报告的 tsc 数字不是本复核者自跑结果。没有将 mock HTTP/WS／SSR 交互测试计为真实多人端到端验收；旧 current JSON producer 更新仍属 Root 门禁，不手补旧数据。

## 全文阅读的新增文件与无 diff 文件

以下路径均在 apps/web/src/ 下：
- app/RemoteLoginScreen.tsx、app/remote-login-behavior.test.ts、app/remote-login-screen.test.ts、app/remote-logout.ts、app/remote-logout.test.ts。
- auth/credential-store.ts、auth/credential-store.test.ts。
- host/remote-auth.ts、host/remote-auth.test.ts、host/remote-market-context.ts、host/remote-test-context.ts、host/remote-request.ts（无本批 diff）、host/worker-request.ts（无生产 diff）。
- host/protocol-owner-removals.test.ts、host/protocol-public-events.test.ts、host/serde-account-id-contract.test.ts。
- save/account-id-contract.test.ts（任务中的 account-id-wire.test.ts 实际不存在）、save/market-memberships-schema.test.ts。
- save/schema/market-memberships.ts、save/schema/report-frequency.ts、save/schema/report-frequency.test.ts、save/schema/monthly-bindings-contract.test.ts、save/schema/personal-trade-confirmations.test.ts。
- save/schema/company/accounting/amount.ts、save/schema/company/books/income-tax.ts、save/schema/company/books/income-tax-owner.test.ts、save/schema/company/books/tax-owner-test-fixture.ts、save/schema/company/report-corrections.ts、save/schema/company/report-corrections.test.ts。
- store/remote-membership.ts、store/remote-membership.test.ts。
- generated 对照：AccountId、Event、EntityTag、OwnerScopedRemovedOrder、PlayerOrderDelta、PlayerWorkingOrder、MarketMembershipState、MarketMembership、AdmissionFunding、SaveSlot。SavedRuntimeState 由 SaveSlot 引用当前 Web schema 定义，不存在独立 generated/SavedRuntimeState.ts。

## 完整 tracked hunk 清单

下列 +start,count 为复核时 git diff --unified=0 的当前侧 hunk（count 省略表示 1，0 表示删除）。逐文件全部 hunk 已读取；清单不是只 grep 后作出的结论。

```text
apps/web/src/App.tsx:  +22 +25 +31,4 +49,3 +70,3 +96,0 +115 +127 +129 +150 +153,6 +173 +197,4 +256,23 +282,4 +291 +303,6 +315 +320 +329,4 +340 +363 +366,2 +392,7 +409 +416 +419 +421 +423,51 +491 +499 +510 +567 +569 +581 +602,2 +607 +610 +615 +621,10 +632,2 +652 +684,2 +716 +761,16 +781 +784 +802 +820,5 +841 +866,5 +878,2
apps/web/src/app/LocalRefreshViews.tsx:  +2 +5 +9,2 +17 +25,0 +28,5 +53 +82,2 +94 +107,7 +115,27 +146,6 +160,3 +175 +186,2 +205 +207,2 +211 +221 +225,7 +233,27 +263,3 +268 +279
apps/web/src/app/portfolio-selector.test.ts:  +13,4 +20 +22 +33
apps/web/src/app/portfolio-selector.ts:  +3 +11
apps/web/src/app/session-control-commands.test.ts:  +5 +36,18
apps/web/src/app/session-control-commands.ts:  +20 +22,2 +26,2 +29 +36 +38,2 +44 +49 +51 +57
apps/web/src/app/session-host-lifecycle.test.ts:  +36,2 +44,27 +77,19
apps/web/src/app/usePausePreferences.ts:  +72 +75
apps/web/src/app/useSessionHostLifecycle.ts:  +7 +41 +45 +50 +94 +106,3 +115 +124,2 +130,5 +161 +163 +198 +205 +229 +238,4 +257
apps/web/src/app/useTradingCommands.ts:  +5 +24 +51 +53 +59,2 +65 +96,4 +138,5 +163,4
apps/web/src/components/player-orders.test.ts:  +31,2 +41,2 +52,2 +58 +64,2 +71,2 +82 +85
apps/web/src/components/player-orders.ts:  +3,2 +12 +14 +27 +29 +61 +63,3
apps/web/src/dev/NpcDecisionInspector.tsx:  +62 +87,3
apps/web/src/dev/npc-decision-inspector.test.ts:  +31
apps/web/src/host/company-query-coordinator.manual.ts:  +6
apps/web/src/host/company-query-coordinator.test.ts:  +57 +60,11
apps/web/src/host/company-query-coordinator.ts:  +215,2
apps/web/src/host/engine-host.ts:  +7 +17 +34 +41,40 +85,8 +121,7 +130
apps/web/src/host/npc-decision-trace.test.ts:  +6 +29
apps/web/src/host/npc-decision-trace.ts:  +2 +54
apps/web/src/host/protocol/canonical.ts:  +43,6
apps/web/src/host/protocol/effects.ts:  +19 +35,2
apps/web/src/host/protocol/guards.ts:  +169,6
apps/web/src/host/protocol/normalize.ts:  +25,2
apps/web/src/host/protocol/parse.ts:  +2 +33 +38 +58 +87,7 +114 +117 +120 +123 +177
apps/web/src/host/protocol/runtime-delta.test.ts:  +12 +21 +28 +64 +117,2 +120 +133
apps/web/src/host/protocol/runtime-delta.ts:  +2 +17 +19 +28,3 +77 +85,6 +92 +117 +119 +123 +133 +135
apps/web/src/host/protocol/types.ts:  +68
apps/web/src/host/protocol/validate.ts:  +17,2
apps/web/src/host/remote-host-token.test.ts:  +4,2 +29,10 +40,21 +67 +69,5 +74,0 +77,19
apps/web/src/host/remote-host.test.ts:  +3,2 +13,6 +122 +125 +136 +147 +157,52 +214 +219 +246,2 +255 +271 +274 +276 +282 +287 +301 +305 +314 +318 +340 +344 +347 +362,5 +368,6 +375,15 +392,14 +407,2
apps/web/src/host/remote-host.ts:  +3,2 +7,2 +22 +24,3 +31 +45,8 +60,7 +83 +89,16 +133,5 +177,11 +209,13 +223 +235,2 +322,48 +379,0 +382,2 +386,0 +396 +399,3 +404 +409,4 +428 +473 +513,28 +542,2 +549,25 +591 +593
apps/web/src/host/remote-lifecycle.test.ts:  +3 +5,2 +19,2 +24
apps/web/src/host/remote-startup.test.ts:  +3,2 +18,2 +23 +48
apps/web/src/host/remote-state-contract.test.ts:  +3,2 +6 +30 +34 +38,2 +48 +66 +121 +165 +195 +199 +203,3 +208 +210 +213 +215 +217 +219 +234
apps/web/src/host/serde-normalize.test.ts:  +123 +166,4 +173 +175 +177 +179 +217,2 +227 +241 +243 +249 +255
apps/web/src/host/serde-normalize.ts:  +3 +41 +45 +54,2 +58,2 +61 +75,4
apps/web/src/host/startup-policy.ts:  +6 +48
apps/web/src/host/tauri-host.test.ts:  +8 +17,2 +47,29 +87,2 +155
apps/web/src/host/tauri-host.ts:  +2,3 +8 +21,3 +73 +75,7 +111 +121,3 +127 +158,4 +165,4 +217,8 +343 +352 +362,23 +393 +395 +409,21
apps/web/src/host/wasm-worker.ts:  +11,2 +18,3 +22,10 +36,3 +306,28 +372 +381 +407,42 +467
apps/web/src/host/worker-host.test.ts:  +4 +19,37 +293 +323 +326
apps/web/src/host/worker-host.ts:  +12 +26,4 +84,18 +188,2 +199,5 +275,6 +284,2 +337,4 +343 +369,7 +379 +386,2 +453 +455 +462,26 +552,15
apps/web/src/host/worker-request.test.ts:  +68
apps/web/src/mobile/MobileRunToggle.tsx:  +7 +10 +18
apps/web/src/mobile/MobileSpeedSelect.tsx:  +9 +12 +15
apps/web/src/mobile/MobileStockDetail.tsx:  +2 +6,3 +14 +19,2 +27 +36,10 +85 +95,2 +123,2 +127 +131 +134,61 +210 +230 +305 +307 +326 +328 +336
apps/web/src/mobile/market-model.ts:  +453,4
apps/web/src/save/current-save-fixture.ts:  +8,2 +15 +61 +81 +172
apps/web/src/save/schema/company/accounting/journal.ts:  +26
apps/web/src/save/schema/company/books/bank.ts:  +3 +7 +18,4
apps/web/src/save/schema/company/books/industrial.ts:  +1 +4 +13,0 +31,3 +35,0 +64 +72,0
apps/web/src/save/schema/company/books/insurance.ts:  +3 +7 +14,4
apps/web/src/save/schema/company/books/real-estate.ts:  +3 +7 +16,4
apps/web/src/save/schema/company/reports.ts:  +1,3 +9 +24,2 +42,51 +94
apps/web/src/save/schema/market.ts:  +7,2 +62 +77 +108
apps/web/src/save/schema/money-wire.test.ts:  +61
apps/web/src/save/schema/orders.ts:  +9 +18 +71 +103,6 +115,2 +118 +123,2
apps/web/src/save/schema/personal/beliefs.test.ts:  +6
apps/web/src/save/schema/personal/beliefs.ts:  +1 +6 +39
apps/web/src/save/schema/personal/common.ts:  +2 +12
apps/web/src/save/schema/personal/information.ts:  +1 +17 +29
apps/web/src/save/schema/personal/plans.ts:  +1 +6 +39
apps/web/src/save/schema/primitives.ts:  +86,10
apps/web/src/save/schema/root.ts:  +11 +15,3 +20,2 +57 +68,2 +73,15
apps/web/src/save/schema/runtime-state.ts:  +2 +20 +83,26 +112 +158 +261,15 +278,2
apps/web/src/save/schema/save-snapshot.ts:  +9 +53
apps/web/src/store/store.ts:  +20,3 +134 +142 +151 +173,3 +182 +250

```

