# UI merge 后恢复宿主接线独立复核

## 范围与结论

复核 `engine-host.ts`、`remote-host.ts`、`remote-lifecycle.test.ts`、`tauri-host.ts`、`tauri-host.test.ts`、`tauri-initialization.test.ts`、`worker-host.test.ts`，并核对自动合并的 `worker-host.ts` 和 `wasm-worker.ts`。**有条件通过**：未发现恢复接线改变 A 股交易撮合或结算语义；Tauri 回调时序的实际缺陷已在工作树修复并有红测证据。短测仅有一个旧 fixture 缺字段导致的 setup 失败，故不能宣称本批短测全绿，也不签核相关完整验收。

## 复核要点

- `EngineHost.load(slot, archiveSlotId?, onRestored?)` 保留三个参数，接口文档规定 callback 在恢复提交后、发布 baseline 或恢复运行前触发。Browser `worker-host.ts` 显式保留第二参数槽位但不把 `archiveSlotId` 转发给 Worker（browser archive metadata 由 Provider 独立的 IndexedDB `ArchiveStore` 管理），并将第三参数作为 callback；Worker 收到并校验恢复响应后才调用 callback，再发布 baseline。Native/Tauri 与 Remote 宿主则将实际 archive slot ID 透传到各自恢复接口。
- Remote 恢复以单一 `pendingRestore` 阻止重叠请求；HTTP失败不会被误判为恢复未提交，而是保留待权威 baseline 判定状态。成功响应才通知 callback，再请求 resync 或重新连接获取 baseline。generation/cursor 校验仍保护响应不被旧会话消费。
- Tauri 现在先解析恢复响应并由 `timeline.replaceRestoredBaseline(restored)` 校验 generation、snapshot、日期并提交 baseline，再调用 `onRestored`，最后交付更新。测试曾先红：坏 generation/snapshot/date 提前触发 callback（确认次数为 1）；修复后顺序正确。保留 timeline 既有部分状态更新行为。
- `wasm-worker.ts` restore 先 `slot.restore`，取得新 handle 与 `civilDate`，发布新 generation ingress，再发旧 generation 的 restored ACK 与新 generation baseline；重启被排入 microtask，避免同步确认/基线发送之前启动。常规查询仍通过 generation 校验，`stockHistory` 等旧代查询不能混入恢复后状态。
- 改动属于前端宿主恢复通信和状态时序，不改变沪深市场规则、单位或领域交易行为；本次不需要交易所规则依据。

## 验证限制

- 红测证据：`.tmp/ui-design-merge/restore-callback-red.log`。修复后 Remote/Tauri 目标短测记录 `.tmp/ui-design-merge/restored-remote-tauri-short.log` 为 41/41 通过。
- 旧 `.tmp/ui-design-merge/restored-hosts-short.log` 曾记录 71/72；唯一失败是 `worker-host.test.ts` fixture 缺必填 `retained_market_history`，在目标断言前失败。之后 root 安装了符合现行契约的 fixture，并提供 `.tmp/ui-design-merge/restored-hosts-fresh-fixture-short.log`；我已亲读完整日志，75/75 通过，包含此前受 setup 阻断的 case。该 fixture 失败限制已关闭，没有修改 fixture 或弱化断言。
- 未运行 Cargo 或全量回归；此结论只针对列明的 host 恢复接线，不代表 live/stash 后续集成完成。
