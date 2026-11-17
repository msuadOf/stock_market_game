# ADR-0046：共享会话权威与异步观察、归档

- 状态：accepted（用户确认的架构合同；实现及三端实跑证据分别核验）
- 日期：2026-11-17
- 决策者：用户；AI 记录
- 关联：ADR-0010、0025、0032、0033、0043、0045

## 问题

共享 `runner::run_cycle`、`Cadence`、`manual`、`CapturedCheckpoint`、`CheckpointOrder`
和 `ObserverPump` 仍不足以统一产品行为。Server、Desktop、WASM 曾分别决定整轮失败分类、
可恢复暂停、永久终止、归档收据、恢复换代与观察流重同步。Native 将乱序收据和过期收据一起
忽略，Browser 对乱序收据报错；Browser 的异步归档失败还可绕过 Rust pacing 和发布顺序。
这些属于业务合同差异，不能以传输不同解释。

## 决策

### 一份 Rust authority

`engine::session::authority` 拥有整轮自动和手动推进、调度采样、暂停及故障锁存规则。
命令拒绝、可恢复暂停和永久终止必须明确区分；永久失败后不再推进或执行经济写命令，保留
读取诊断、可用查询、恢复、新局和 shutdown 所需的控制能力。已提交的成功前缀须先交接，
无法交接时显式描述缺失范围；不能重新执行已提交交易。错误语义、脱敏因果链、恢复动作
只有一份，目标 ABI、传输故障及底座死亡诊断保持真实来源。

撮合、T+1、价格时间优先、账户范围和 receive receipt 的既有领域合同不变。本 ADR 不
定义新的交易所规则，不改变交易单位、税费或日结业务事务。

### 归档独立于观察

成功日结捕获不可变 sealed checkpoint，在继续推进前通过共享归档 authority 完成顺序
检查和非阻塞交接。Native SQLite lane 与 Browser IndexedDB Worker 只实施 I/O。
每 tick 不新增存档复制、编码或同步等待；UI ACK 与是否存在观察者都不参与归档判断。

`queued`、`in-flight`、`durable`、`failed`、`blocked` 是不同事实。入队不等于保存成功。
失败不得回滚已提交日界，不得丢弃失败候选后假装后续日终已补齐它。保留同一 sealed
checkpoint：若协议能够识别同 key 的独立 attempt，可明确重试；否则维持共同 blocked
状态，禁止继续跨越日界。当前没有 attempt fencing 的端点不得盲目自动重试。
旧 generation/writer epoch 或已处理重复收据可丢弃；活动窗口的未知、损坏或乱序收据
必须报错。迟到归档错误不能覆盖已锁存的永久首错。真实 writer 必须独立验证 lease。

shutdown 在有界期限内排空并确认收据；剩余排队或失败候选必须明确报告，不能把关闭成功
当作耐久成功。Browser 使用跨 Worker 共享的绝对 deadline，归档退出预算包含在整个
关闭预算之内，并保留 Rust owner/reclaimer 清理时间；已知未耐久且退出已确认时继续安全
清理并返回归档错误，退出未知时不声明 Memory 可复用。授权文件 mirror 保留权限和 latest-only/best-effort 边界，不替代独立快槽。
分时只保留当前日内，日K独立归档；按需从 DB 读取日K仍延期，沿用 ADR-0045。

### 恢复有明确提交点

共享 replacement 事务先 prepare：验证新会话、下一 generation 和所需资源能力；prepare
拒绝保留旧状态。commit 才替换 ingress、会话所有权和 generation，并使旧流、旧写入与
旧包失效。commit 后发生 I/O、旧句柄清理或 baseline 构造失败，必须报告新 generation，
不得假装回滚。Server 的成员绑定、Native writer lease 和 Browser handle/Memory 是适配
边界；Browser 单槽归档 consumer 不允许假设新旧 writer 能同时注册。

Server 的主 restore 响应和后续 catalog 查询可能分开失败，Tauri 可以在同调用 commit
后构造 baseline 失败；两者都服从同一提交合同，不强求相同物理响应位置。

远端能力 `restoreOutcome: commit-receipt-v1` 对应显式请求头，响应头区分 `rejected`、
`committed`、`unknown`；提交响应携带 `CommitReceipt`。派发前拒绝不能标未知，派发后
失去回执不能假定回滚。客户端只有实际接纳有效新基线后才解除写入围栏；本地旧基线落后
于其他 controller 不会抹掉已收到的合法提交事实。未协商的旧端保持原 Snapshot 协议。
Browser prepare 成功进入不可逆退役之前封闭真实 ingress 并撤销 token，然后才关闭旧归档
consumer；未证实 Worker 退出时不在同 Memory 上继续析构或重新挂接 consumer。退役后
未换代故障必须明确为 fenced，与已提交新代故障分别报告。

### 观察流与客户端接受

每个观察者使用 generation、observer lease 和 publication ordinal 的 typed 身份。
同 generation 重新挂接也换 lease，旧 ACK、release 或 cut 不能影响新观察者。
hold → capture → fence → install → release 是共同事务；stale、gap、closed 明确区分。
terminal 只交付一次，成功前缀先交付或显式报告缺口。各观察者独立，慢或断开的前端不能
阻挡市场与归档；真实存储 blocked 可以明确暂停。

Server terminal 使用独立于普通 bounded queue 的首错槽，由共同 `TerminalDelivery`
决定成功前缀与显式缺失区间，队列满不再把 terminal 降格为泛化 lag。WS 显式协商观察
协议 v2 后，普通帧和可恢复错误携 lease/ordinal；release 确认同时验证切面与观察身份。

transport sent/flushed 和应用 accepted cursor 是不同事实。socket flush、Worker 消息
和 Tauri IPC 不必在同一物理时刻 ACK，不能用其中一种证据假装另一种。客户端共享接纳、
故障分类与重同步策略；可恢复归档失败不等于网络断线。兼容行为必须由明确能力驱动。

### 保留适配边界

LibOS 只提供通用线程、队列、唤醒、时钟和其他真正必要的机制，不知道股票、会话政策或
ACL。业务不检测平台、不直接依赖目标平台库。Server 鉴权、多用户和一对多属于部署能力，
保留在 Server；Worker/WS/Tauri 编码、入口、I/O 与 ABI 保留在适配层。

## 验收

静态门禁限制宿主残余策略、Cargo target/features 依赖与 TS AST 平台机制；它不代替行为
证明。同一 scripted commands+failures workload 应记录 day/tick/seq/generation、暂停、
terminal、queued/durable 和 observer 身份及两个 ACK 游标。不同并发到达轨迹的整局成交
字节不用于强行证明相同。

Native、真实 Chromium shared WASM、真实 Linux Tauri 分开记录实跑证据和产物指纹，
仅编译、mock actor 或旧 benchmark 结果均不得标记为当前三端验收通过。普通测试每 case
及命令硬限 10 秒，构建及必要长验收整个进程树硬限 300 秒；最终完整 diff 独立复核后才
运行本批验证。实现状态与实际结果由任务台账记录，本 ADR 不预先宣称通过。
