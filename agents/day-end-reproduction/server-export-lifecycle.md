# Server 文件导出候选生命周期

日期：2026-11-09。实现范围仅为 `apps/server` actor/routes 与代表性测试；spool 实现由独立任务负责，客户端由另一任务接线。

## 契约

- 自动主存档与显式文件导出复用同一次 `DayEndCandidate::capture`，随后释放 engine pending；主存档写失败不撤销已成功日结，也不阻止文件导出。
- WS `SubscribeDayEndExports { request_id, generation }` 只允许当前控制主体，返回 `DayEndExportSubscribed { request_id, subscription, afterSeq }`。`afterSeq` 是 actor 处理订阅时的权威 seq，租约只负责此后日终；旧 WS backlog 不授予历史文件导出权。
- WS `UnsubscribeDayEndExports { request_id, generation, subscription }` 同时核对登录主体、generation 与本连接身份，成功返回 `DayEndExportUnsubscribed { request_id }`。
- HTTP `POST /api/day-end-export` 与 `/api/day-end-export/ack` 都接受 `{ session_id, generation, subscription, candidate: { seq, settledDate } }`。读可重试且不消费；精确 ACK 只释放本订阅的候选。HTTP 仍核对控制权、主体及 generation，无 spool 候选时显式拒绝，不能 fallback 到最新一天。
- 每连接最多一份订阅；同日 payload 由各订阅共享，彼此 ACK 独立。无订阅时不创建临时 spool。
- 正常断连、心跳 supervisor 取消、重新同步、broadcast lagged、publisher 需要重新同步都清理本连接订阅；其他连接不受影响。Reset/Restore 清空旧世代并要求重新授权。
- 临时队列默认总容量为用户确认的 2 GiB，可通过 `SessionManager::with_day_end_export_limit(bytes)` 配置非零上限。本任务未增加 CLI 参数。
- 容量不足优先停止 pending bytes 最大的订阅并向其发送 `DayEndExportFailed { subscription, code, message }`，释放后重试发布；单个候选无法容纳或 spool 故障停止相关导出，不改变交易与主存档。断连及重新同步的取消 code 为 `DAY_END_EXPORT_CONNECTION_CLOSED`，容量为 `DAY_END_EXPORT_CAPACITY`，换局为 `DAY_END_EXPORT_TIMELINE_CHANGED`。

## TDD 与验证

先加入行为测试，实际运行 `cargo test -p server --lib --no-run --jobs 10`，因缺少导出 API/状态编译失败；实现后重新构建通过。构建使用进程外 `scripts/run-long-validation.mjs 300000`，普通测试使用 `scripts/run-with-deadline.mjs 10000`。

首批 7 项行为短测通过，随后扩充至 11 项。将全部 11 项放在同一普通命令中时，10 项已完成，但整条命令达到 10 秒被 supervisor 终止；没有把该次报告为通过。最终按每个 case 独立进程、最多 4 项并行重新执行，11 项均通过；最长 case 为周末两日多客户端、自动 archive 与 baseline 组合，4.33 秒。

覆盖：周末逐日候选、两客户端独立 ACK、重复读、错误主体/generation/连接、断连回收、真实 Reset/Restore、旧 WS backlog 的 `afterSeq` 边界、超限保持交易与主档健康、只取消慢订阅、主库写失败仍提供文件候选、HTTP 鉴权与 wire 字段。

真实 WS + HTTP 测试 `ws_file_export_retains_exact_day_until_ack_or_disconnect` 通过（1.02 秒）：真实 TCP WS 订阅、两个周末日结、重复 HTTP 读、ACK 后拒绝旧读、WS Close 后拒绝未确认旧读。测试初版误用了只接受成功响应的既有 helper，遇到预期 400 时失败；改用本测试的状态读取 helper 后通过，没有弱化预期。

旧回归：`civil_prefix_is_published_and_archived_before_the_next_real_month_end_failure`（0.48 秒）与 `archive_write_failure_keeps_the_previous_candidate_and_the_committed_civil_update`（2.54 秒）均通过，各自受 10 秒 supervisor 约束并行执行。

`git diff --check` 通过。未运行完整 Rust/Web 回归、真实大档磁盘压力测试、磁盘写满注入或完整浏览器文件授权流程。

## 独立复核

未实施此 Server 改动的 `fix_review` subagent 独立审查完整 Server diff，发现并促成两处修复：订阅应答与旧 WS backlog 的竞态、同 generation Resync 丢帧后的孤立 owner。补 `afterSeq` 及连接级重新同步清理后，复核未发现剩余阻断。其静态确认鉴权、subject/generation/connection 绑定、Drop guard、Reset/Restore、同次 capture 和容量故障隔离保持一致；未自行运行 Cargo。

本批不改变 A 股交易制度、现金/股份、T+1、费用、交易日与自然日划分，只改变已成功日终候选的导出持有与释放，因此沿用 ADR-0025、ADR-0033 的现行领域边界，不引入新的交易规则假设。
