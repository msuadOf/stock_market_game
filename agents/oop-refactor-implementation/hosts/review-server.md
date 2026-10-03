# server / WASM 独立复核

2026-10-03。复核者未参与本批实施；未修改源码，未运行 Cargo、普通测试、长测试或 Git 写操作。
复核范围为相对 `b89afb3346743a4b4fccf26c9ac9ff108595f696` 的七文件完整 diff：
`apps/web-wasm/src/lib.rs`、`protocol_tests.rs`；`apps/server/src/web_ui.rs`、`routes.rs`；
`apps/server/tests/deployment_cli.rs`、`api_contract.rs`、`ws.rs`。
同时核对未改动的 `deployment.rs`、`deployment_routes.rs` 和 `SessionHandles::subscribe_events/shutdown`、
`SessionManager::remove` 的真实 caller 及资源语义。已读 AGENTS、principles、testing、architecture、
open-questions、trading-rules 与 ADR-0010/0017/0027，以及本批 actions/status。

## 发现

### R-SERVER-01：flush 策略的边界断言遗漏（低优先级，已修复并复核）

`routes.rs:1815` 的新增 `flush_actions_leave_incoming_update_unaccepted_until_send_succeeds`
对 MetadataTransition 和 Push capacity 分别构造动作，但 `routes.rs:1837` 只执行 `let _ = all`。
buffer 也仅存一帧。因此，若误将 `routes.rs:1175` 的单帧策略改为 `all=true`，或将
`routes.rs:1188` 的完整 backlog 策略改为 `all=false`，该测试仍通过。实际 loop 在
`routes.rs:1348` 附近依据该标志决定继续取 backlog 还是结束 flush，这是本次状态机抽取的明确契约。

建议在两组输入分别断言 `all=false/true`。保留当前发送成功前不接受 incoming update 的断言；
不要求新增 65,536 units 压力 fixture 或 socket 失败注入架构。当前生产实现的标志值正确，
本项属于边界测试缺口，没有发现已存在的业务回归。

增量复核：实施者将两组输入改成 `(error, expected_all)`，分别固定 false/true，
`routes.rs:1829` 新增 `assert_eq!(all, expected_all)`，删除丢弃标志的语句；原 incoming update
断言保持。该修改仅增强本次新增测试，无产品代码变化，R-SERVER-01 已关闭。

## 三项门禁结论

1. **大 A 语义与依据：静态复核通过。** 本批不修改交易制度、单位、费用、T+1、撮合顺序或存档格式。
   Registry 将原样步骤移入 owner，保留 `civil_day_ready → end_civil_day_update` 屏障、
   `step_frame → tick_batch` 顺序与 fatal 的 `at_session` 定位；构造/恢复成功才注册，
   未知 remove 仍幂等而 access/step 仍显式失败。API fixture 仍先执行自然日日结再取得 SaveSlot，
   seed 的 HTTP 编码仍为十进制字符串；cash/generation/pending-player 等原断言保持。
   WS 的 CommandQueued 仍只表示入队，resync 屏障不阻止 SubmitIntent。领域依据沿用
   trading-rules 的官方来源及已有核对日期、ADR-0010/0017；本复核没有重新访问官方网站，
   不将本次工程复核日期冒充新一次制度核验日期。
2. **必要性与范围：通过。** 六个对象各自对应明确的注册表、路径约束、连接协议或测试资源生命周期；
   未引入继承、通用 service、额外依赖或 engine 权威状态副本。StaticAssetRoot 保持 namespace、
   字面路径、method、canonicalize、containment/hidden、metadata、ServeFile 的顺序，
   不缓存请求路径、不改变 symlink/TOCTOU 政策。RejectedCommandFixture 保持 500ms/5ms、
   真正 child 与原拒绝断言，没有借抽类新增 Drop 或进程树承诺。
3. **边界、漂移与复杂度：通过，R-SERVER-01 已修复并复核。** WS 保持 failure→generation→barrier→covered→push，
   failure 成功发送后才 latch；Resync 保持 subscribe→clear→baseline 请求，失败只保留已发生的 clear，
   成功 snapshot 写 cursor，成功发送后才更新 latch/barrier。flush 未接受输入在原 socket loop 完成发送后
   才 push，发送失败仍退出；Pull capacity 与 Lagged 清 buffer 后显式 resync。
   ServerFixture 的清理范围可准确描述为 abort+await listener、remove 注册 session 并请求 actor shutdown，
   正常返回和被 spawn 的测试 panic 路径均显式执行此流程。不能据此宣称已关闭所有 Axum 内部连接 task、
   任意逃逸 Arc、取消外层 future、进程终止或跨进程资源；status 已登记外层取消边界。
   ApiTestSession 只有正常结束与已完成身份建立后的 domain fixture/restore 错误显式 shutdown，
   不能将它描述为所有 panic/协议响应损坏分支的资源保证。

## 验证与完成条件

当前结论为“完整静态复核通过，无未关闭有效发现；运行门禁待父 agent 统一执行”。
status 如实说明未运行 Red/Green，因此无法由本轮记录确认运行中的 TDD Red 证据；不能将
rustfmt 解析或 diff 检查当作编译/测试成功。父 agent 应按 status 的代表性短测列表完成定向编译和
进程外 10000ms 普通测试门禁，再结合复核修复结果报告整批完成。

增量修复后已执行七文件 `git diff --check`，通过；仍未运行 Cargo 或测试。
初次 routes.rs SHA-256：`36df32599b93689e497955bee07814e749c281f7b994a1adc84416fa51907968`。
增量复核后 routes.rs SHA-256：`fbddfd6e20ebb7f734eaa2ebc87150c207ec3df3de9aff69a554ae1544668c7e`。
