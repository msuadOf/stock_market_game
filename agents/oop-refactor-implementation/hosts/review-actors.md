# actors4 独立复核

日期：2026-10-03。复核者为未实施本批源码改动的 `review_actors` subagent。基线为 `b89afb3346743a4b4fccf26c9ac9ff108595f696`；本次只执行读取、检索、diff 检查及本记录写入，没有修改源码，没有运行 Cargo、长验收或派出其他 agent。

## 结论

本批静态独立复核通过，未发现有效的阻断或需修复问题。此结论不等同于编译、doctest 或行为测试通过；root 统一 Rust 验证仍待完成。

## 覆盖范围与依据

逐份读取以下 8 个文件全文，并审查相对基线的完整 diff：

- `apps/desktop/src-tauri/src/actor.rs`
- `apps/desktop/src-tauri/src/actor/failure.rs`
- `apps/desktop/src-tauri/src/actor/fatal_tests.rs`
- `apps/desktop/src-tauri/src/actor/protocol_tests.rs`
- `apps/server/src/actor.rs`
- `apps/server/src/actor/fatal_tests.rs`
- `apps/server/tests/actor.rs`
- `apps/server/tests/protocol_updates.rs`

另检索整个工作区的 Rust raw sender 调用，并核对 `apps/server/src/routes.rs` 初次连接及 Resync 的订阅接线；该文件其他 publisher 重构不属于本记录的完整复核范围，由所属独立复核负责。

已读取 `AGENTS.md`、`docs/principles.md`、`docs/architecture.md`、`docs/open-questions.md`、`docs/trading-rules.md`、ADR-0010、ADR-0017，以及 `hosts/actors/actions.json` 和 `status.md`。本批不新增或调整交易制度，故使用现有正式规则登记和两个 ADR 的宿主/事务契约核对语义；未访问官方网页，不能宣称重新验证了 2026-10-03 的交易所条文。现有交易规则登记明确区分沪深差异、游戏简化及中国结算费用表访问失败，未把缺口冒充已通过来源验证。

## 三项门禁

### 1. 大 A 语义与依据

符合本批所触及的 A 股语义边界。diff 未改变价格、股/分单位、申报数量、费用、T+1、交易阶段、委托有效期、交易所差异、订单簿受理或存档结构。`ProtocolSession` 仍由各自 `SessionActor` 独占写入，checkpoint/rollback、tick 提交、CivilUpdate 准备和发布完整保留。

ADR-0017 的 CivilUpdate 契约要求先交付完整屏障再按宿主偏好暂停，且显式恢复不得重复日结。桌面 `actor.rs:918`、服务端 `actor.rs:1190` 保留先 emit/broadcast 再 `pause_at_civil_boundary`；feature `host-parity` 的命令路径也保留同顺序。fatal 分支仍先恢复 checkpoint，再停止与发布结构化失败；没有把业务拒单改为 fatal。

ADR-0010 要求应用语义统一而保留宿主真实传输差异。此次收窄 raw sender 并提供 Receiver 订阅，未改变协议 payload、generation、timeline、seq 或命令确认含义；`CommandQueued` 仍仅表示入队。

### 2. 必要性、最小范围与 owner

四项动作与冻结清单一致。

- `SessionHandles` 的必要可见性改动限定为桌面 `cmd_tx`、服务端 `cmd_tx/event_tx`；`SessionCommand` 仍为 `pub`，既有公开方法签名保留。公开字段撤回确实产生源码 API 兼容成本，`status.md` 已如实登记。
- 桌面 `actor.rs:142` 与服务端 `actor.rs:308` 中的 Pacing 按值持有 `running`、`fastest`、`requested_speed`、`tick_interval`、`base_ms` 和 `SpeedMeter`。生产 `SessionActor` 不再保存第二份上述会话节奏字段，也不直接写它们，Pacing 是实际状态 owner。manager 的 `base_ms` 是新会话构造配置，没有成为会话状态的重复写者。
- `SpeedMeter` 继续作为 Pacing 的子对象；Fastest 14ms/步数预算、server semaphore、引擎事务、generation 和事件出口仍归 actor。未引入跨宿主基类、共享锁或新的 engine owner。
- `ActorHarness` 只在 `cfg(test)` 中拥有 mock App、被测 actor、原命令 sender 与按需 receiver，操作限于构造和订阅。`ReplayGuard`、ticks、故障注入及业务判断仍留在原测试场景，没有新建替代业务状态。

没有发现与动作无关的功能扩张。服务端 fatal 测试两处注释中文化不改变行为。

### 3. 边界测试、跨层语义与复杂度

未发现本批引入的语义漂移或遗漏的必要边界。以下边界静态核对与基线一致：

| 边界 | 核对结果与证据 |
|---|---|
| Desktop/Server Fixed 下限 | Desktop 仍为 1µs，Server 仍为 1ms；未用共同默认值抹平差异。新增测试分别覆盖下限。 |
| Fixed → Fastest → Fixed | Pacing 保留 Fastest 下的原固定 interval；回到 Fixed 时按各自函数计算。requested 与 fastest 一起更新。 |
| 初始/启动/重复启动 | `new` 先标记暂停；改变 running 才 reset，重复启动不清空采样。新增用例和既有 server 集成断言覆盖。 |
| Civil pause | 无条件停止并 reset；即使已暂停，也保留基线日界行为。发布顺序保持在暂停之前。 |
| Restore | 成功替换 ProtocolSession 后按新 tick 重置采样；运行中为 None，暂停中为 Some(0.0)。校验失败路径不 reset；错误优先级未变。 |
| Fatal | 只写 running=false，不新增采样 reset。原停机错误回执与后续拒绝顺序未变。新增 Pacing 用例与原 fatal 断言保留。 |
| Desktop 非法内部速度 | 仍显式记录并忽略；状态/interval 不变。SetSpeed 仍只提交 mpsc，不等待 oneshot。 |
| Server 速度校验 | 句柄仍先验证有限正数与 MAX_SPEED_MULTIPLIER，Fastest 保留 +Infinity 哨兵；actor 原 debug 不变量保留，未混入新错误契约。 |
| 命令 helper | send 失败和 oneshot receiver 关闭仍映射 ActorGone；engine 错误仍映射 Rejected；InvalidSpeed、restore 时钟配置拒绝及校验顺序保留。调用者停止等待不会撤销已投递请求，burst 测试完整保留。 |
| 订阅 | `subscribe_events` 直接调用私有 sender 的 subscribe；初次 `routes.rs:1270` 和 Resync `routes.rs:1415` 均先订阅再取 public_baseline，订阅时点不变。外部仅持 Receiver，无发布写权。 |
| Harness 生命周期 | App 持续活到场景结束；fatal/protocol 订阅在运行 actor 之前建立。fatal close 仍检查原 cmd channel；无重复 failure/events、business hash/save 回滚、日结保留分时点与恢复一次等断言没有删除或弱化。 |

新增测试是节奏状态转换边界，没有为 Harness 编写镜像实现测试。新公共 API doctest 路径对应实际公开 crate/module：桌面为 `stock_market_game_lib::actor`，服务端为 `server::SessionHandles`；Receiver 的正向 `no_run` 示例与三个私有字段反向示例均待 root 执行。

## 有限检查与剩余验证

`git diff --check -- <上述 8 个文件>` 成功。工作区 raw sender 检索只发现 actor 模块内部实际使用及 doctest 的故意非法访问。上述检查不是 Rust 类型检查或行为验证。

root 仍须完成统一构建、公共 API doctest、两宿主 Pacing 用例、desktop fatal/protocol、server fatal 与 actor/protocol_updates 的适用回归，并遵守任务统一 deadline/并发安排。未见需要实现者修复并二次复核的静态发现。
