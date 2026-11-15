# 共享市场 runner：未验收工作树交接

基线是 dev `cce075a`。仅 dot 云电脑工作树，没有 commit、push、运行应用、编译或跑测试。设计见 migration.md。实现和契约测试是整批候选的草稿，不能按已经统一或可发布交付。

## 已写入的候选代码

- `engine/session/runner`：三端驱动固定倍速的绝对 deadline、晚醒 Skip、MAX 14 ms / 100,000 tick、已可见的控制优先、日界共享推进及暂停。按旧协议在权威线程上冻结已验证 TickBatch，并在后续故障时保留成功前缀与首错。补写了受控时钟、真实 ProtocolSession 小 fixture 的行为测试，均未运行。
- `engine/session/publication` 与 LibOS：有界、非阻塞交接，独立 ordinal、超载缺口、同代基线游标、观察者 ACK 窗口；libos 提供 native 调度等待、Mailbox、WakeEpoch、第三个浏览器发布 Worker 的预登记 tombstone；权威推进不会等这些 ACK。
- Native Server / Tauri 现有 actor 调用共同 runner/wake API；Tauri 驱动独立输出工作，携带 generation / ordinal + 基线更新。Native Server 已有独立 WebSocket 连接任务：不为“异步”再加一次复制的 relay。
- Browser WASM 引入发布 sibling，自动 `WasmAuthorityRuntime` 仅复述 Rust WakePlan，原 JS 自动 tick loop 已移除。Rust staged TickBatch/CivilUpdate 在单独 Worker 取出后再 serde；阈值、确认和顺序缺口不回流权威推进。启动/恢复/显式刷新握手携带独立 ordinal、工作委托、generation；终止错误走同一发布 lane。新增启动、待确认慢观察者和错误测试代码，均未运行。
- `ProtocolSession::observer_baseline` 在同 owner turn 收集快照及活动委托，不清全局增量和存档候选；`take_save_candidate` 交接冻结 Arc，需要由宿主异步 I/O lane 校验并写盘。

## 全量联调的阻塞点

1. Native 已认证观察者的基线/增量/日界过滤 must be coherent。原 `routes.rs::run_ws` 丢弃鉴权主体，连接全局 `PublicBaseline` 调用会触发 `prepare_public_baseline` 并清掉所有观察者的增量基准和候选；旧 `ClientFrameBuffer` 还拥有重复流控逻辑。把主体贯通并完成权限隔离、按观察者 ordinal fencing + 安装完整委托后，才迁移并删掉旧业务发布路径。不能用未筛选的 account 0 视图冒充远程成员。
2. 平台套件尚未完全合并。仍需核对跨宿主绝对线程语义、采样、observer refresh pacing、Notice/Recoverable 和归档错误的公共策略。浏览器常规 tick/civil 编码在独立发包线程，但现有显式新局/恢复/重同步的 baseline 仍通过权威 Worker 同步编码；改为冻结基线、返回 ticket，由同发布任务序列化。未证明真的可在慢观察者且饱和时继续稳态前行。
3. 持久化要独立于任何观察者数量和生命周期。普通 Native day-end 异步提取及接线不等于完成所有 restore/renew/archive selection 的 writer epoch/事务围栏；还需验证存档失败时保留先前有效档与已提交日终成功前缀，跨代旧写不可提交。Browser/Tauri 前端历史保存实现仍须合入共有游戏策略，观察队列溢出不得丢唯一候选。
4. 未跑全部接口测试。真实 WASM 生成 bindings、编译、正式产品 Browser/Linux/Tauri 测试和匹配负载计量都必须在整组代码对接完之后统一执行。完整 diff 必须经未参与实施的 subagent 审查；已有静态检查不等于产品验收。
