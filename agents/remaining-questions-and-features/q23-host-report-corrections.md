# Q23 报表更正的三宿主应用接线

## 范围与契约

- 本项只接应用控制入口，不改变所得税、会计金额、Journal、公司经营、交易制度、calendar 或 ingress receipt 的经济与排序规则。
- `submitReportCorrection(request)`、`cancelReportCorrection(operationId)`、`queryReportCorrections()` 是市场控制能力，不接受任意 `AccountId`。当前 owner 是现有认证会话；不设计未来多人 admin 角色。
- Server 使用 `/api/market/report-corrections` 的 POST、DELETE、GET。Bearer 必须通过现有 owner 验证；`generation` 必须是当前代的 canonical decimal string。请求 DTO 拒绝未知字段。
- Desktop 使用 `submit_report_correction`、`cancel_report_correction`、`query_report_corrections`；参数为 `sessionId`、`generation` 及 `request` 或 `operationId`。
- Server／Desktop 成功响应为 `{ generation, value }`；mutation 的 `value` 是 null，query 的 `value` 是 `ReportCorrectionStatus`。`pending` 是数组，`completed` 是按 `operation_id` 索引的 record。
- WASM 使用同名 export，传入当前 owner `handle`；Worker 的 generation 检查由 Web host 作者负责。旧 handle 失效即拒绝，不序列化 `ReportCorrectionEpoch` 的 Weak。query 明确输出 plain record。
- Native actor 必须先检查 generation 与健康状态，之后才获取当前 `ProtocolSession.report_correction_epoch()`。换档后的旧请求不得取得新 epoch。
- 日内 `pending` 只在内存；完整日终成功后才提交公开更正与日级完成事实。不得以本接口生成日内存档、旧格式兼容或新 schema version。

## 错误与恢复边界

父实现者把可恢复的请求拒绝限定为 typed `SessionError::ReportCorrection`，保留结构化 cause。只有该 variant 使用 `REPORT_CORRECTION_REJECTED`、`recoverable: true`：暂停运行并明确通知，保留健康 actor／ingress，使查询、公开取消和重试仍可使用。

`StepFatal`、`CorrectionInvariant`、其他 `Information` 错误以及 rollback 失败维持既有 fatal 关闭与拒绝合同。不得靠文本猜测把未知内部错误降级；不得为了测试续行撤销 fatal close。

## TDD 与当前证据

root 的 `host-build-12.jsonl` 记录真实编译成功，耗时 43.32 秒。本项只执行编译后的 binary，不自行运行 Cargo；真实 `--list` 已确认三个新增 case 存在。

红灯以三个进程并行执行，显式 `RAYON_NUM_THREADS=8`、`--test-threads=8`，每条命令由 `scripts/run-with-deadline.mjs 10000` 限时：

| case | 实际红灯 | 日志 |
| --- | --- | --- |
| Server owner／generation／控制分离 | 404 不等于 401，0.38 秒 | `.tmp/checklist-wave4/q23-server-control-red.log` |
| Server 自动日终失败后取消／重试 | 提交 404 不等于 200，0.38 秒 | `.tmp/checklist-wave4/q23-server-recovery-red.log` |
| Desktop 真实 IPC | `query_report_corrections` 未注册，0.22 秒 | `.tmp/checklist-wave4/q23-desktop-control-red.log` |

上述真实红完成后才实施 production API 与恢复分支。新增补充测试覆盖：Server 完成事实的日终 save／restore、精确幂等与 completed 不可取消；Desktop 自动失败后保留 command receiver 并取消重试；WASM registry 旧 handle 拒绝、pending 拒绝存档及 typed 日终错误后取消重试。

当前源码已提交 root 统一绿色构建与非作者完整 diff 复核；本记录尚未登记绿色结果，不声称已经完成三宿主 runtime 验收、完整回归或 Q23 整体核销。WASM registry native 测试不等于真实浏览器 WASM runtime 测试。
