# Task 29 / 3 / 30 全文 EOF 复核

复核人：未参与实施的独立 reviewer（luna29）
产品目标：`08e4fc75b52a71a3262a8a938c57b44f8b5b4960`（当前 HEAD `a7c7ce3`，目标是其祖先；merge 等价）
日期：2026-10-03

## 全文读取范围及章节矩阵

三份输入均从首行读取至 EOF，没有按片段代读。行数：`task-29-review.md` 256 行、`task-3-review.md` 21 行、`task-30-review.md` 58 行。本文未改产品文件、Git 状态或测试证据。

| 输入 | 全章节覆盖 | 复核结果 |
| --- | --- | --- |
| `task-29-review.md` | Final Re-Review、两项 P1 关闭证据、Public Query/defaults/scope、Final Verification、Git receipt、Residual risks、Final Disposition；Reopened Civil and Disclosure Event Contract、Missing Event、Authority/Ordering/Privacy、Transaction/Sequence/Restore、reopened gates、Web/format limits、Git receipt；Re-Review Current Working Tree、V finding、nested K7 P1、具体 reproducer/additional holes、Integrated Parser、Re-review Gates、Generated-Baseline Safety、Disposition；Scope Audited、Original Findings Status、Confirmed Non-Blocking Results、Isolated Git Baseline Audit、Independent Gate Results、LSP、Required Disposition | 两个历史 REJECT 中的 K7 parser gap 已被最早 final section 记录为修复并由严格解析器与 mutation 证据关闭；V-era finding 同样已关闭。Civil/disclosure 事件契约证据一致。未将较晚保留的历史 REJECT 误作当前状态。 |
| `task-3-review.md` | Method；Findings 1–5（行为保持、re-export、replay、serde、scope）；Tests；VERDICT | 历史 PURE MOVE/行为等价审查及其测试结果完整；本次不是重做旧提交双向源码比较，未发现其结论与当前日历/协议实现冲突。 |
| `task-30-review.md` | 初始 findings/confirmed、两轮 restore-generation follow-up、WASM report-period repair、cross-host blocker、central cross-host period repair re-review | 早先 report-by-ID 路由缺口已有修复证据；WASM 与跨宿主 period DTO 修复结论可由当前实现确认。另发现 Worker restore caller 对 `nextGeneration` 缺少严格单调守卫，见下文。 |

## 追踪：当前日历、统一帧与日界

- `packages/engine/src/session/civil_clock.rs:386-455` 的 `end_day` 校验重复/回拨/跳日、运行上限、过期 due，再计算次日状态并推进自然日；周末/休市并不增加 market session 数。当前实现显式区分自然日日界与交易日，不将二者混作 tick。
- `packages/engine/src/session.rs:2073-2145` 的 `end_civil_day` 先核对已完成交易 session，再以 checkpoint 包裹自然日日结；执行顺序为 civil clock、经营、封账、披露、事件记录，最后调用观察者。披露 event 只在 immutable library 插入后生成，`CivilDateAdvanced` 最后生成。
- `packages/engine/src/session/protocol/civil/mod.rs:20-63,165-219` 定义统一 `CivilUpdate`/`EngineUpdate` DTO。AfterClose 更新须带完整当日 `TickFrame` 序列，验证日内帧数、末帧 tick/seq；休市日只发 `CivilAdvance` 且不附 intraday 帧。`CivilUpdate::validate` 对 next date/status、sequence、snapshot、证券集合与 publication IDs 作校验。
- Worker 的 `endCivilDay` caller（`apps/web/src/host/wasm-worker.ts:251-256`）从 WASM 取得 engine `CivilUpdate` 后经同一个 `loop.publish` 协议通道发布，随后才回 `civilDayEnded`。常规帧由 `WasmTickLoop.stepOnce/frame`（`wasm-tick-loop.ts:55-97`）承载，没有独立的 Worker 侧自然日算法。这里没有发现 tick/civil 更新绕过 DTO 或重复推进交易语义。
- 上述日期/收盘 DTO 不改 A 股撮合、价格、交易时段或结算规则；这批实现语义是模拟自然日日结边界。就本审查关注范围，没有发现交易所规则差异被合并或由宿主伪造。

## 追踪：Public DTO 与宿主

- `packages/engine/src/company/query.rs:462-505` 由同一个 engine projection 生成 `PublicReportSummary`；`period` 调用共享 `period_end_date` 转 ISO 自然日。`information/publication.rs:181-188` 使用 civil date 算月末。Server 与 Tauri actor 直接携带 engine DTO；WASM 也调用同一 query（`apps/web-wasm/src/lib.rs:420`），不再本地补算 period。
- Web Worker 通过 `normalizePublicReportPage` / `normalizePublicReportById`（`apps/web/src/host/serde-normalize.ts:23-29`）解析公开 DTO；公开查询不把 books、journal、NPC 私有 belief/plan 带到响应。Decimal 会计金额及 opaque ID 保持字符串。旧 task-30 “server/desktop 仍返回 YYYY-MM” finding 在当前 engine projection 已被消除。
- 旧 task-29 中披露事件顺序、事件载荷隐私、SaveSlot 回滚等描述，与当前 `session.rs` 及 Civil DTO 验证一致。任务 3 的 extraction replay review 是 session 执行代码搬迁历史，与日期 DTO 没有额外跨层语义漂移。

## Restore generation：发现与反证边界

**新候选（P1/P2 未定级；建议按协议正确性修复后加短用例）：** `apps/web/src/host/worker-host.ts:80-95` 的 `restoreWorkerSlot` 只调用 `generation(response.nextGeneration, ...)`，验证它是正安全整数；没有要求 `nextGeneration > currentGeneration`。`createWorkerHost.load`（同文件 353-363）直接把返回值设为 `currentGeneration` 并建立 baseline。task-30 follow-up 所称 “rejects a non-advancing generation” 在当前代码中并未成立：`worker-host.test.ts:101-110` 只测试 `1 -> 2` 成功，没有相同/回退响应测试。

**可复现反证：** 直接调用 `restoreWorkerSlot(scope, validSlot, requestId, 4)`；响应满足 request correlation（`generation: 4`）并给出合法 snapshot，但 `nextGeneration: 4`。当前 helper 会成功返回 `{ snapshot, nextGeneration: 4 }`；传 `nextGeneration: 3` 也会通过正整数验证。生产 Worker 的 `WasmSessionSlot.restore`（`wasm-session-slot.ts:39-56`）本地递增 generation，因此正常实现不会发出这类值；但宿主是协议边界，malformed/stale Worker 响应不该被采纳。非单调回执可令恢复前后的请求代际相同，失去 generation 用于隔离旧响应的承诺。

修复最小点是在 `restoreWorkerSlot` 比较响应值与 `currentGeneration`，拒绝 `<=`；再以 helper 或 host-level 测试锁定 equal/lower 两种响应。Worker 内 `generation += 1` 也没有安全整数溢出保护，但这是需经过约 `Number.MAX_SAFE_INTEGER` 次生命周期才触发的不同边界，非上述可现实触发候选，本次不建议扩展范围。

## 旧 REJECT 结论复核

- Task 29 nested K7 shape REJECT：复核为**已修复**。当前最终段列出 exact parser chain，工业库存、chart scalar、四行业 params 都经结构化 parser；对应测试与旧 reproducer 的拒绝证据被列明。当前没有重新跑测试（本复核任务禁止长测，且无需启动测试才能静态确认链路）。
- Task 29 V-era finding：复核为**已修复**，最终段说明 host parser/UI/generated type 均移除 V，负向 fixture 专门断言拒绝。
- Task 30 report-by-ID unreachable finding：复核为**已修复**，当前 worker case、host caller 与 WASM API 路径存在。
- Task 30 restore `nextGeneration` finding：先前结论声称关闭，但**本次反证仍通过**；应在修正并复核前保留为开放项。
- Task 30 public report period cross-host finding：复核为**已修复**，共用 engine `PublicReportSummary::from` 构建日期，宿主不独立转换。
- Task 3 的 APPROVE 只针对历史纯移动提交与该提交的 replay/结构证据。本次没有重新比对该提交完整 diff；不把历史批准扩大解释成当前所有行为都已重新执行验证。

## 结论

审查范围内自然日/统一帧/DTO 路径与旧 task-29 findings 的关闭证据相符。当前确认一个 Worker restore response 单调性缺口：调用者缺少 `nextGeneration > currentGeneration` 守卫，且历史声称的负向测试不存在。除此之外未从所要求的日历、日界、DTO、restore caller 路径找到额外语义遗漏。未运行测试，未修改产品代码。
