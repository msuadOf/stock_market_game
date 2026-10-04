# Remote / WS 链路独立复核

## 范围与方法

- 日期：2026-10-04；复核者未参与本批实现。
- 阅读工程原则、开放问题、架构、ADR-0005 与 ADR-0010，核对本批完整 Remote diff：`remote-host`、`remote-request`、`remote-publisher-state`、新增 `remote-request-scope` 及相关测试；服务端仅核对 `routes.rs` 的 WS / auth / heartbeat 改动、`tests/ws.rs`，不将其他 owner 的指标改动算入本批。
- 本次复核采用代码与测试审阅，未重复执行测试。实现者报告的 Node 45 项及 token 3 项结果不作为本人独立执行的证据；Rust 编译与 loopback 运行结果待实现者登记。真实浏览器网络与公网部署未验收。

## 首轮三项门禁回答

1. **大 A 语义**：本批不改变价格、股数、费用、T+1、撮合、日界或存档事实；`CommandQueued` 仍只表示入队，不伪装为委托接受或成交。断线、切模式、重同步、超时后明确“结果未知”，不自动重发委托，避免重复交易；保持 ADR-0010 的确认语义。未新增交易制度，因此无需据此新增交易所规则或扩大游戏简化范围。
2. **必要与最小范围**：浏览器 subprotocol 凭据、speed GET 授权、pull 连续取帧、baseline 不重放、恢复连接身份隔离和 Promise 终态均直接对应 G01–G05 / G66。`RemoteRequestScope` 集中管理 HTTP 的超时与中断，避免每个 API 重复实现，未引入依赖。服务端仅协商公开 `stock-game`，不回显 auth subprotocol；URL query 凭据原拒绝边界保留。代码没有新增凭据日志，但不能把这点宣称为任何外部代理都不记录 header。
3. **边界与复杂度**：旧 socket 的 message / close / error 隔离、并发 refresh 共享 waiter、同步 send 异常、dispose / delivery 中断、HTTP 迟到响应等已有定向测试。以下 deadline 阻塞缺口必须修复并再次复核；恢复重试耗尽、WS 写确认超时与真实浏览器 subprotocol 握手仍建议补代表性短测试。实现复杂度整体与需求相称。

## 有效发现

### R1：Pong deadline 不能中断正在执行的分支（P1，首轮发现，现已修复）

`apps/server/src/routes.rs` 的 `run_ws` 在外层 `tokio::select!` 增加 10 秒 deadline，但各分支中的 `sender.send(...)`、`public_baseline().await` 与 `enqueue(...).await` 在选中后会独占控制流。Ping 已发送后若 peer 读取缓慢导致发送 backpressure，或者 actor 请求阻塞，deadline future 不再被轮询，不能保证 10 秒内终止。到期分支自身还会无超时地等待发送 Close，因此即使发现到期也不保证连接任务结束。当前 loopback 测试只覆盖无其他阻塞工作的 idle peer，不能排除此路径。

修复应使 outstanding Pong deadline 独立于连接工作分支继续被轮询，且到期终止不能再无界等待 Close；增加阻塞写入或阻塞工作 future 的代表性短测试。修复不应改变 FIFO、命令受理事实或自动重发写请求。

## 第二轮复核

- 保留首轮 R1 证据。修复后的 `run_ws` 把完整 `run_ws_connection` future 交给 `supervise_ws_connection`，通过 watch 发布 outstanding deadline；Ping 发送前发布期限、只有匹配 Pong 才清除。监护层可在工作分支 await 阻塞时到期退出并 drop connection future / socket 两半，不再等待 Close 发送，因此 R1 的结构性根因已消除。
- 新增虚拟时间测试 `pong_deadline_cancels_a_connection_blocked_inside_an_await` 使用 pending future 与 DropMarker，断言恰 10 秒退出并释放资源；loopback 测试只接受明确的 EOF / Close / closed transport 结果，未将任意错误算作成功。本人核对测试代码，但 Rust 尚待实现者完成编译与运行，不宣称 Rust 测试已绿。
- 追加核对 pull 单 inflight、5000ms 响应期限及 timer 清理：不再每 16ms 无界排队 GetFrame，协议响应后才能请求下一帧，连接身份变化会清除 timer。
- 追加核对取消后的迟到 `CommandQueued`：`RemoteCommandRegistry` 仅记录曾 pending 且被中断的 ID，迟到确认不能改写已拒绝 Promise，也不关闭仍健康的重同步 socket；真正未知 ID 仍抛出协议错误。新连接清理 interrupted ledger，旧 socket 消息由连接身份隔离。对应集成测试覆盖同 socket resync、写请求 5000ms 超时以及未知 ID。
- G66 再核对：HTTP 请求在超时、dispose、重同步、切模式时有拒绝出口；并发 refresh 共享同一 Promise；替换 baseline waiter 时先明确拒绝旧 waiter，完成时先清槽，不遗失 load waiter；dispose 的 DELETE 响应失败或超时会 reject 并显式报告，不自动重发委托。三次自动恢复耗尽已有新增短测试。
- 实现者报告更新为 Remote 48 项及 token 3 项通过；本人没有独立执行，真实浏览器网络与公网仍未验收。以上修复没有改变大 A 规则，新增状态仅用于传输背压和已发送请求的结果未知语义，属于必要最小范围。

## 第二轮结论（历史）

首轮 R1 已修复并完成代码级再次复核；所审完整 scoped diff 未发现剩余阻断性缺陷、交易语义漂移或不必要范围扩张。独立代码审查通过，但 Rust heartbeat / loopback 运行证据仍须补齐，不能把本结论等同于测试或真实浏览器 / 公网验收通过。

## 第三轮增量复核：同步消费拒绝

- 新增 `deliver` 尊重应用层 callback 的显式 `false`：清 timer、失效连接身份、进入 baseline gate、关闭 transport、拒绝 pending 请求及 waiter。该路径不另发 generic fatal，保留 coordinator 已产生的原结构化 failure；未确认写请求仍只报告结果未知，不自动重发。
- WS baseline、protocol 与 start 的 cached baseline 均使用该交付边界；cached baseline 拒绝时 start reject，且代码不会继续发送 `running:true`。新增测试覆盖 WS baseline 拒绝、refresh waiter 拒绝、无覆写 failure 与写请求 gate；cached baseline 的启动拒绝路径建议增加直接断言。
- 复核确认 Remote / Worker / Tauri callback 字段均为 `void | boolean`，与 `EngineHost` 一致。曾基于旧 snapshot 提醒核对字段类型，该提醒不构成本增量有效发现；实现者确认在同一 patch 已同步字段，并报告全量 TypeScript 检查 5.85 秒 exit 0。Node strip-types 测试不能代替 TypeScript 类型检查。
- 本轮增量独立代码审查通过，不影响大 A 委托受理语义，范围属于共享宿主错误边界的必要接缝。实现者报告 Remote 49 项、token 3 项、Rust unit 2 项及 handshake 9 项通过；本人未重复执行。heartbeat loopback 的 OS readiness fixture 竞态仍在修正，不能把未完成的运行验收记为通过。

## 第四轮增量复核：期限不可续期

- 实现者通过 loopback 发现匹配但迟到的 Pong 可在 supervisor 轮询 connection 时清除已过期 deadline；`WsHeartbeat::pong` 已增加 `Instant::now() < deadline` 守卫，并新增恰 10 秒时匹配 Pong 不清除期限的虚拟时间测试。该小增量逻辑正确，属于严格期限边界，不改变游戏交易语义。
- 再次独立复核提出 R2：如果 runtime 长时间迟滞，过期的 supervisor 仍可能先 poll connection；下一次 30 秒 heartbeat 可覆盖旧 outstanding deadline。建议在 supervisor 每轮读取期限后先显式检查已过期并退出，禁止过期后再 poll connection，补已过期期限不能被 connection 更新续期的短测试。R2 当前待修复与再次复核；因此第三轮通过仅适用于同步消费拒绝增量，不代表最新 heartbeat 完整期限边界已核销。

### R2 修复再次复核

- supervisor 已增加过期前置检查，测试 `an_expired_deadline_prevents_connection_poll_from_renewing_it` 断言过期后根本不 poll connection。独立复核进一步指出：同一个 select 已挂起后 runtime 迟滞，恢复时不能重新执行前置检查，仍须禁止 connection 内部重新发 Ping 覆盖 outstanding。
- 最终 `WsHeartbeat::ping` 返回 `Option<Vec<u8>>`，只要 outstanding 存在就返回 None，不再覆盖原期限；连接 heartbeat 分支收到 None 立即退出。结合 Pong 的严格截止守卫，任何分支均不能把已过期期限重新延长。新增 `an_outstanding_ping_cannot_be_replaced_after_runtime_delay` 在虚拟 30 秒后断言二次 Ping 被拒绝且原 deadline 不变。
- R2 根因已修复，最终增量独立代码复核通过，无剩余已识别阻断发现，不改变 A 股规则或扩展产品范围。实现者已报告前一版 heartbeat loopback 通过（0.155 秒）；包含最后 Ping 守卫的最终 Rust binary 尚待编译与运行，不将旧 binary 结果冒充最终版结果。

## 最终完整复核与证据收敛

- 重新核对 formatted scoped diff，`routes.rs` / `tests/ws.rs` 的定向 rustfmt 仅调整排版，不改变已审行为。本人再次执行 scoped `git diff --check`，exit 0。
- 最终实现版的运行证据已在 [remote-chain.md](remote-chain.md) 登记：`--no-run -j16` 编译 exit 0、5.26 秒；最终 heartbeat binary 在 `timeout 10s` / `--test-threads=8` 下 5 项通过、0.041 秒；最终 WS binary 在相同期限与并发下 10 项通过、1 个既有性能 probe 忽略、0.56 秒。loopback 仅获得本机回环权限，panic-cleanup 用例为预期 panic 且清理成功。这些为实现者提供并登记的结果，本人未重复执行，不再保留“最终 binary 尚待运行”为现行状态。
- Node 与 Vite token 最终合并短套件 53 项通过、1.24 秒，包括此前建议的 cached baseline 启动拒绝不得发送 `running:true` 用例；最后全量 TypeScript 检查 exit 0、6.47 秒。先前 fixture / sandbox / 并行中间态失败记录继续保留，不改写历史为全程成功。
- G40 Remote 权威确认真实链已逐层核对：`start` / `stop` / `setSpeed` 等待 `requestJson`；`api_running` / `api_speed` 等待 `SessionHandles::set_running` / `set_speed` 的 oneshot；actor 先应用 `pacing.set_running` / `apply_speed`，然后发送结果。因此 HTTP 成功不是命令 enqueue 确认，不冒充已经成交；`SetSpeed` 后主循环重建 interval。App 的 await 后 UI 更新由消费端 reviewer 联合复核，不将他方职责假称本人完整 UI 验收。
- G04 Remote 仍只首次交付未交付的 cached baseline，暂停后继续不会重放旧 baseline；load / 显式 resync / reconnect 新权威 baseline 正常交付。Tauri 边界不归本记录。

## 最终结论

R1、R2 均已修复并再次独立复核；最终 scoped 完整 diff 的大 A 语义、必要最小范围与边界一致性门禁通过，未发现剩余已识别阻断问题。定向测试与静态检查证据齐备；未执行完整回归、真实浏览器网络或公网部署验收，不扩大结论到这些未验证范围。
