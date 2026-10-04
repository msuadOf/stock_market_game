# Web 回归失败修复

## 范围与根因

本轮只修改五个 Web 测试文件，不改生产代码、金额格式或 A 股撮合规则。
原日志的 `8 Web test shard failed` 包含首个真实失败触发的在途取消，不能算八个真实失败。
第一轮独立短测复现三个 case：字段错误期待旧价格文案；两个 Worker owner 场景发送
不含 `generation/requestId` 的旧控制消息，现行 Worker 正确拒绝，因此没有启动 timer。
补齐真实请求后，继续运行八分片发现另外两个过时契约：Tauri source contract 未考虑
异步 `await host.stop()`；Worker request 测试仍期待发送失败资源泄漏直到 timeout，
而生产实现已即时清理。最终共五个真实失败，不是八个。

## 改动依据

- ADR-0031 要求元输入精确解析为分字符串，并保留错误上下文。字段错误测试核验
  当前精确 parser 的完整错误文本，价格／数量字段关联和禁用价格校验保持不变。
- Worker owner 测试显式携带本人 generation 与 request ID，检查 started/stopped
  确认；原来 restore 回应、baseline、microtask restart、旧 generation 拒绝、
  暂停屏障和资源释放断言均保留。
- Tauri 初始化 source contract 核验等待 start 和隐藏页 stop；另增加运行时场景，
  在异步创建期间切入后台，确认暂停 IPC 完成前不得 ready。
- 同步发送异常仍以原错误对象拒绝，立刻清理登记和 listener；增加后续请求和模拟
  timeout 的隔离断言，不能通过恢复旧泄漏行为满足测试。

以上不改变申报价格、股数、T+1、费用或优先规则，也不引入数字 Money 兼容和存档版本。

## 验证

- 第一组修改前短测：4 case 中 3 失败；修改后 4 通过。
- 第二组短测：22 case 通过，外部 10000ms deadline 和单 case 10000ms 上限。
- `node scripts/run-web-tests.mjs`：141 文件、8 并行分片、735 case 全部通过，
  0 失败、0 取消；共享 batch wall-clock 2766ms。
- 日志：`.tmp/main-regression-2026-10-05/web-fix-tests-final.log`。
- `apps/web` 的 `tsc -b --pretty false` 通过，外部 10000ms deadline，实际 7465ms。
- 非作者独立复核另运行上述五文件的26个 case，全部通过；使用外部10000ms deadline、`--test-timeout=10000`、`--test-isolation=none`。同一进程中的 Worker 全局替换与 mock timer 需要隔离执行，因此该代表性短测文件内不并发；主任务八分片验证仍使用多核。

## 独立复核

非作者 `review_web_civil_server` 完整读取五个改动测试、相关 lifecycle、Worker 消息处理、`WorkerRequestScope`、价格输入实现及 ADR-0031 原文。结论：通过。消息补齐当前 generation/request ID 不改变 restore 的真实顺序；新增确认断言与创建期间切后台的运行时场景加强检查。同步异常测试转为核验立即清理，并保留原错误对象和后续请求隔离，不以资源泄漏作为正确契约。价格文案保持精确 parser 的完整上下文，不放宽金额输入；没有 Money 数字兼容、新存档版本或 A 股交易规则变动。

同时完整复核 `apps/server/src/routes/auth_tests.rs`：仅将 `initial_price`、`tick`、`retail_cash_median` 三个 Money fixture 叶子改成相同分值字符串；非金额数值和全部认证／拒绝断言未改，属于 ADR-0031 的遗漏 fixture 同步，不改变认证权限。此独立复核不另启动 Cargo，Server case 的真实编译／执行证据由主任务补齐；不声称仓库全回归通过。
