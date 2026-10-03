# Batch 151 独立复核

## 范围与来源

按 scan plan 分配完整读取三份历史 web 审计材料，并与基线 `43b1aa5`、当前 `AGENTS.md`、`docs/principles.md`、实现审计台账及现行产品调用链对照。源文件共 188 行，三份均逐篇读至 EOF；行数及 SHA-256 与 scan plan 一致。未将历史 review 中 agent 的停止/后续工作指令当作本轮约束。未运行测试或修改产品代码。

## 结论

历史 review 的对象边界判断大体仍成立，但其中的调用路径需按现状修订：`createTimelineEventGate` 仍只有测试引用，生产 Tauri host 的两个 listener 独立按 session、timeline 和 disposed 过滤（`apps/web/src/host/tauri-event-coordinator.ts:6`、`tauri-host.ts:113`）；没有证据表明提取 gate 已接入生产。此项应记为测试辅助函数/生产实现并存，不构成产品缺口。

两个历史候选仍值得准确保留：

- Remote pause preference 在 publisher baseline 尚未建立时仍以 generation `"1"` 发送（`apps/web/src/host/remote-host.ts:192-194`）。现行调用链由 `useSessionHostLifecycle` 在 `host.start()` 前 await `host.setPausePreferences`（`apps/web/src/app/useSessionHostLifecycle.ts:135-142`），因此初始新会话确有触达该路径的可能。Server 对此 generation 的契约含义仍未在本次所读证据中确认；登记为协议待核实，不推断接受/拒绝策略或缺陷。
- `WorkerRequestScope.request` 在注册 listener 和 timeout 后直接调用 `postMessage`，同步抛错时 Promise 会拒绝，但没有同步执行 `cleanup`；pending 项和 listener 留至 timeout 回调清理（`apps/web/src/host/worker-request.ts:43-73`）。现行 WorkerHost 生产路径直接使用该 scope（`worker-host.ts:21`），故这是可达的资源清理候选。代码注释明确称保留既有时序，但没有给出端口不抛错保证；旧 review 的缺陷候选未被消除。

旧材料涉及分时图 Collector、存档 parser 等文件的结论与本批分配内容无关；不将其作为本批的实现发现。当前 `implementation-audit-2026-10-02.md` 的 G01–G68 产品缺口清单中未见上述 remote generation 与 Worker request 清理边界被明确裁定或替代。它们不涉及 A 股撮合、报价或计量语义，本批不提出交易规则结论。全局章节族状态仍以实现审计台账为准；本批仅补充宿主/协议历史证据，不改变其余章节状态。

## 旧结论复核

| 历史结论 | 当前状态 |
|---|---|
| web-04：baseline 前设置 pause preference 会使用 generation `"1"`，Server 含义待确认 | 再证；现行 lifecycle 仍在 `host.start()` 前 await 设置，Server 契约仍待核实 |
| web-04：`postMessage` 同步抛错时 Worker request 清理延迟至 timeout | 再证；scope 仍保留该时序，且 WorkerHost 生产调用可达 |
| web-04：Tauri event gate helper 未接生产，生产 listener 单独过滤 timeline | 再证；当前全仓调用仍只有 helper 测试，生产 listener 过滤逻辑仍独立 |
| web-04：已识别 Remote 消息的 generation 检查存在协议顺序不确定性 | 本批未取得足够端到端 Server/Publisher 顺序证据；保持待协议核实，不擅自判定接受旧 baseline 是错误 |
| web-05：Collector 构造默认值区分，生产 runtime 不直接实例化这些 Collector | 该材料主要讨论 mobile market model；本批没有重读完整产品实现，沿用旧证据且不外推为新复核结论 |
| web-06：普通 engine `Snapshot` parser 与 strict save snapshot parser 边界不同 | 该材料主要讨论存档 schema；本批没有重读完整 parser 调用链，沿用旧证据且不外推为新复核结论 |

## 局限

未检查 Server 对 pre-baseline pause preference 的处理实现及 WebSocket 消息顺序端到端契约；未重审 web-05、web-06 所覆盖的全部产品文件。本文是三份历史材料的限域复核，不替代完整产品 diff 复核。
