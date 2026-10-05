# CompanyPanel Availability Timeline 独立复核

## 范围

仅复核 `CompanyPanel` availability 查询中 timeline generation 变更处理，以及 `company-report-selection.test.ts` 新增 deferred success/error 用例。工作区有大量其他改动，均不在本次复核范围。

## 结论

前轮发现的 loading 卡死已修复：timeline change effect 现在将 `availabilityLoading` 设为 `false`。result、error、loading state 都有对应的 timeline owner generation，并在 render 时过滤 owner 不匹配的旧值；`currentTimelineGeneration.current` 在 render 阶段同步更新，旧 timeline promise 的 result/error/finally 会立刻失配。effect 另递增 `querySequence`，因此同公司 timeline 切换后的旧响应不能覆盖新 timeline 状态。

此前发现的表单首次 render 窗口已修复：owner key 为 `JSON.stringify([companyId, timelineGeneration])`，`formOwner` 不匹配时 render 直接输出空的 `periodEnd`、`kind`、`scopeKind`；effect 提交前的 timeline 切换与同 timeline 公司切换用例都断言三个控件为空。旧请求的同步 owner ref guard 同样比较完整 owner pair，避免同一 generation 换公司时迟到响应提交。

## 测试与证据

当前限定复核冻结版的七个相关用例：新增 query payload、timeline deferred success/error、effect 前 timeline 隔离、同 timeline 公司切换 deferred success/error 与 completed result，以及原有三例报告选择回归。effect 窗口由显式 `renderWithoutEffects` / `flushEffects` 控制；completion promise 取代固定微任务冲刷，时序确定。同 timeline 公司切换的 pending deferred 用例在 effect 前按精确查询按钮文本查找按钮，并断言存在；同时断言文本不包含“正在查询”，所以空表单造成的 disabled 不会掩盖 loading 检查。

已阅读正式 deadline 执行日志 `.tmp/checklist-wave4/host77-panel-owner-pair.log`：7 tests、7 pass、0 fail，`duration_ms 872.39945`。另一 author 日志 `.tmp/checklist-wave4/company-panel-owner-pair-final.log` 同为 7/7。前轮旧 red log 已由 author 标无效，不作为 TDD 红测证据，也不编造或推定失败结果。

## A 股语义与范围

本次审查的 timeline generation 与异步 UI 状态隔离属于 web 层生命周期管理，不改变 A 股交易制度、报告期间含义或金额单位。该修复与任务要求一致，范围保持在 `CompanyPanel` timeline 状态失效及相邻测试。
