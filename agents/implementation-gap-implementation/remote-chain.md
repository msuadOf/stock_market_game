# Remote 连接链补齐记录

## 范围与领域边界

- 对应 G01、G02、G03、G04（仅 Remote，Tauri 由宿主控制 owner 处理）、G05、G66。
- 开工已读 AGENTS、principles、ADR-0005/0010/0027、open-questions、architecture、reaudit-host 全文与总账原始证据。
- 不改变 A 股交易规则、价格/数量单位、委托语义或撮合；`CommandQueued` 仍仅表示权威队列入队，不表示委托受理或成交。不自动重发确认中断的委托。
- 保留私有路由鉴权、URL 凭据拒绝基线；不新增公网账号、证书或部署验收能力。

## 实施

- 浏览器 WS 以 `stock-game` 与 `stock-game.auth.<UTF-8 hex token>` subprotocol 提供凭据；Server 仅回显公开协议，Bearer 原生客户端继续支持。URL 不再携带 token。
- speed GET 添加会话 Bearer；start/stop/setSpeed/dispose 返回 HTTP 权威确认的 Promise。
- pull 每 16ms 最多发一个未完成 GetFrame，响应后继续；无响应在 5000ms 后恢复连接，避免发送队列无界积压。
- start 仅对尚未交付的缓存 baseline 交付一次；暂停继续不重放旧 baseline。读档、显式同步与断线恢复仍交付新权威 baseline。
- Server 在 Ping 发出前发布 10 秒 Pong deadline；独立 supervisor 监护整个 connection future，超时直接取消并释放 socket，能中断发送背压或 actor await。只有匹配 Pong 清除期限。
- Remote 旧 socket message/error/close 均按连接 identity 隔离；意外断线以 100/200/400ms 重试，三次失败后明确 fatal。恢复必须取得新 baseline 才开放协议及写入。
- HTTP 请求有 5000ms 完成出口和生命周期取消；WS 写请求超时、dispose、resync、切模式均显式结果未知。并发 resync 共享 Promise，waiter 替换明确拒绝旧请求，settle 前清槽保留重入请求。
- 真正已发送而取消的请求，其迟到 queued 不重新确认、不破坏健康同步连接；未发送或重复完成的 ID 仍显式协议错误。应用层同步拒绝更新时立即隔离传输，不覆盖其原始结构化 failure。

## TDD 与验证

- 先增加 Remote 生命周期/单槽 waiter 测试，红阶段 8 case 中 6 失败；实现后绿。Server subprotocol 集成 case 在旧 binary 明确因 HTTP 401 失败，然后才重编新实现。
- Node 定向命令：`timeout 10s node --test --test-isolation=none --test-concurrency=8 --test-timeout=10000 --test-force-exit apps/web/src/host/remote-{host,startup,state-contract,request-scope,lifecycle,publisher-state,command-registry}.test.ts`，49 case 全绿，最近运行约 0.55 秒。
- 从 apps/web 运行 remote-host-token Vite SSR 定向命令，3 case 全绿，约 0.74 秒；从错误 cwd 首次执行失败已纠正，不冒充首轮成功。
- Rust 独立 `--no-run -j16` 编译；测试 binary 使用 `timeout 10s` 与 `--test-threads=8`。首次编译约 77 秒；后续曾被其他 owner 正在实施的 engine 字段/模块接线中间态阻塞，未替他修改。
- heartbeat unit 两 case 全绿：匹配 Pong 清期限；虚拟时钟恰 10 秒取消阻塞 await 并断言资源 Drop。
- WS 回环握手 subprotocol case 已绿；首次完整 WS 短套件中 9 case 绿、1 手工性能 probe 忽略，新虚拟时间 heartbeat fixture 时钟接线竞态导致命令达到 10 秒 deadline，被 supervisor 终止；已缩小/固定 fixture 并等待重编验证，未把该次报为通过。
- `git diff --check` scoped 验证通过；tsc 曾发现并行 EngineHost consumer 接口中间态，Remote 自身类型错误已修。
- 未执行完整回归、真实浏览器网络或公网部署验收。普通测试 command/case 均不放宽 10 秒；编译与测试分离。

## 独立复核

- 未实施本轮改动的 reviewer 已审完整 scoped diff，发现 Pong deadline 无法中断分支 await 的 R1。
- 已根因修复为外层 supervisor，补阻塞 future 取消测试并再次复核通过，见 [remote-chain-review.md](remote-chain-review.md)。
- 后续控制 owner 要求的同步拒绝更新增量已补测试；最终证据将随短测完成追加，未自行改总账或 Git index。

## 后续定向证据

- Remote 与 Vite token 合并短套件 53 case 全绿，包含 cached baseline 拒绝不得发 `running:true`；wall 1.24 秒。完整 TypeScript 静态检查 5.85 秒、exit 0。
- WS 虚拟时间 fixture 必须让真实 OS driver 完成 baseline flush 与 heartbeat 装配，再推进虚拟时钟；用真实 5ms 加虚拟 31 秒固定启动，所有 socket 读取均有 500ms 硬界。修订后心跳 loopback case 0.155 秒通过，未删除或放宽关闭断言。
- 此短 fixture 揭示迟到 Pong 可在 supervisor 轮询连接分支时清掉已过期 deadline。现只允许匹配且 `now < deadline` 的 Pong 清期限，并补恰 10 秒边界测试。
- 独立复核继续发现 runtime 调度迟滞可让下一 Ping 覆盖旧期限。现 supervisor 每轮先检查过期；`WsHeartbeat::ping` 在任何 outstanding 存在时拒绝替换，主循环直接终止连接，并补虚拟 30 秒后不延长期限及过期状态禁止再 poll 的短 case。以上修复不改变 heartbeat 周期或交易制度。

## 最终 Rust 证据

- 最后重新 `timeout 300s cargo test -p server --test ws --lib --no-run -j16`，exit 0，缓存稳定后的 Server 定向编译 5.26 秒；此前分钟级日志仅为编译，不伪装成普通测试。
- 最终 `server-2ecbfc8548baf826 heartbeat_tests --test-threads=8` 外包 `timeout 10s`，5 case 全绿，wall 0.041 秒。
- 最终 `ws-c03c03900ac86c65 --test-threads=8 --nocapture` 外包 `timeout 10s`，10 case 全绿，1 手工性能 probe 按既有标记忽略，wall 0.56 秒。panic-cleanup case 的预期 panic 被 fixture 捕获，case 成功且完成资源清理。
- 回环监听的初次 sandbox 拒绝已如实记录，最终验证通过 sandbox escalation 仅开放本机回环测试，不是公网部署。
- 独立 reviewer 最终复核 R1/R2 与同步消费拒绝增量通过；未执行完整回归、真实浏览器 WS 或公网授权验收。G04 的 Tauri 证据不归本记录。

## 联合核销依据：G40 与 G04

- G40 Remote 不再 fire-and-forget：start、stop、setSpeed 均 await `requestJson`；`RemoteRequestScope` 仅在 `remoteJson` 完成 fetch、读取 body 且确认 HTTP 成功后 resolve。失败、超时和生命周期中断明确 reject，UI 不以该 Promise 冒充已应用。
- Server `api_running` await `SessionHandles::set_running` 后才返回 204，`api_speed` await `set_speed` 后才返回 200。两个 handles 方法各建 oneshot，投递 `SessionCommand` 后继续 await reply，而非只等待 channel send。
- actor 对 SetSpeed 先 `apply_speed` 修改 pacing，对 SetRunning 先 `pacing.set_running`，之后才 reply Ok；fatal 状态 reply Err。此控制确认与 `SubmitIntent` 的 `CommandQueued` 入队语义明确不同，不表示订单接受或成交。
- consumer `SessionControlCommands` 在 await start/stop 后调用 onRunning，在 await setSpeed 后调用 onSpeed，并核对 host identity；App 对应 ports 才 dispatch Redux running/speed。Remote 本身不维护乐观 speed/running 缓存，因此实际 UI 更新链是 actor applied → oneshot → HTTP → Remote Promise → consumer UI。
- G04 Remote 的缓存 baseline 只在尚未交付时由 start 交付；暂停继续只请求 running，不重放旧 baseline。Tauri generation guard 与原生 resume 链由宿主 owner / hostreview 独立核销，不能用本报告替代。
- root 最后只对 routes.rs/ws.rs 执行定向 rustfmt。原 reviewer 已再次审 formatted 完整 scoped diff、确认无行为差异，并将最终 binary 证据收敛到 [remote-chain-review.md](remote-chain-review.md)；保留初次失败、未完成状态与修复历史，未冒称 reviewer 自行重跑。
