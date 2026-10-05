# Q23：统一财报更正市场控制 UI

## 本批职责与边界

`EngineHost` 提供 `submitReportCorrection`、`cancelReportCorrection`、
`queryReportCorrections`，三客户端只消费相同的
`ReportCorrectionStatus { pending, completed }`。`completed` 为按
`operation_id` 索引的对象；请求、结果与 `JournalEntry` 复用现行严格 parser。
Native 的 mutation 只确认 `{ generation, value: null }`，并不表示完成公开；
Worker 的 mutation 只确认请求 ID 与 generation，查询完整状态。

更正区复用 `CompanyPanel` 的公司与已公开报告选择，两布局共用组件，不增加账户功能
或管理员角色。当前单玩家只能由合法 owner 使用；未来共享市场须由真实组合市场控制
能力授权，不能据此认为普通交易玩家已获得权限或多人授权已完成。

UI 的 `JournalEntry[]` 输入初始为空，不能由公开报表总额反推出实际账务，更不预填添钱
示例。`amount` 明示为 `AccountingAmount` 元字符串而非 `Money` 分；提示原始凭证
不得修改结构化子账。合法公司、原公开报告、实际 source、凭证允许范围及账务勾稽最终
由 Engine 权威验证，不用前端成功解析冒充日终过账成功。

页面明确：只在成功日终过账和公开，失败没有部分账务／部分公开；待办可以取消，完成
结果只能用新更正继续修正；换档／新局丢弃日内队列，不持久化日内请求。
Host 在请求前捕获 generation，返回时检查当前局和响应 generation；查询沿用各宿主
已有 baseline epoch，不能让旧响应进入新时间线。UI 更正区以 generation／company
为 key，卸载后的响应不更新旧页面；自然日改变后重新查询权威结果。

仅 `REPORT_CORRECTION_REJECTED` 且 `recoverable: true` 被识别为可取消重试的
日终域错误。Worker 保留健康 host 与 ingress，Remote 保留连接，Tauri 保留会话；
App 暂停并明确显示错误反馈，同时保留更正控制区。其他故障继续既有 fatal 处理，不按
错误文字猜测可恢复性。Rust 停跑与 typed error 由 Native／Engine 作者接线。

## 短测试实际证据

- 严格状态／确认 parser 两项和实际组件 SSR 一项先运行失败（新增模块缺失），再实现。
- 实际 Worker recovery case 先红：旧 generation failure 被当作当前故障；实现后确认
  旧故障隔离、健康查询及取消能力均保留，Worker 没有终止。
- Tauri mockIPC 与 Remote fetch／WebSocket harness 各一项使用实际 Host adapter，
  验证 payload、Bearer、generation、请求／取消／查询、可恢复日终错误和换档隔离。
  这两项是实施后的补充测试，不能把它们称为先红 TDD。
- 六项定向 case 并行度 `--test-concurrency=4`，全部实际通过，最近执行约 0.64 秒。
  每个 case 配置 10000ms timeout，整个命令由 `run-with-deadline.mjs 10000 --`
  外部 supervisor 执行。不是全量回归，也不是真实 Rust／WASM 产物验收。
- `tsc --noEmit -p tsconfig.app.json` 在十秒外部 deadline 内实际 exit 0。
- 首次定向 lint 找到新组件漏列 `refresh` 依赖，已使用稳定 `useCallback` 修复。
  App 同时报告三个本批未修改的旧 `hostRef` dependency warning，不顺手改动。

Remote 首次补充测试预期换档后的 generation 错误，但现有 `RemoteRequestScope`
在换档发起重同步时已主动中断 pending request；断言随后精确固定真实的“远程基线
重同步，确认中断，结果未知”语义，旧响应仍不能完成查询。没有放宽成功／拒绝边界。

没有运行 Cargo、生成绑定、完整回归或操作 Git。root 安排的独立非作者
`review_q08_final` 已对本批相关完整 diff 及修复最终复核 PASS；本记录不替代真实
Native producer、完整经济边界与完整回归的最终验收。

## 独立复核后收口

非作者 `review_q08_final` 发现初始化时 `slot.create` 已推进 generation，但 Worker
还没发出 `created` 即失败时，原过滤会吞掉真实启动错误。新增实际启动短 case 先红：
Worker 未立即终止，进程最终由十秒外部 deadline 终止；修复后启动 failure 的非负安全
generation 先严格校验，仅已经 initialized 且真实旧代才忽略。未知未来代、缺失、负数
及字符串 generation 均显错；created 前合法 generation 1 立即返回真实错误。

补充 `STEP_FATAL` 即使消息含 `ReportCorrection`、`recoverable: true` 仍 fatal，及
`REPORT_CORRECTION_REJECTED` 但 `recoverable: false` 仍 fatal，两项负例实际通过。
Native 作者确认错误带权威 `context.generation`；Remote／Tauri 仅对 typed 可恢复更正
错误严格读取该字符串并匹配当前 owner generation，旧故障不得暂停新局，缺失和非法
context 不猜默认值。这些边界已加入实际三客户端 harness，原非作者最终复核 PASS：
初始化过滤修复、typed fatal 负例、Native scope、空凭证 UI、单位和 confirmation
边界与最小职责范围均可接受；复核为静态检查，不冒充 Native 端到端或完整回归。

收口后的 13 项聚焦 case 实际 13 passed／0 failed，约 1.06 秒；仍使用四进程并行、
case 10000ms 与外部整进程树 10000ms deadline。相关修改文件定向 lint 实际 exit 0，
scope／初始化修复后的 TypeScript 检查也已实际 exit 0。
