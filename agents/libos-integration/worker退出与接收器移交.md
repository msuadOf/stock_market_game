# Worker 退出与接收器移交：接入约束

日期：2026-11-11。本文是工作记录，不是已实施方案或 ADR。

## 接入位置与现状

`World` 目前将组与交易部统一存为 `JoinHandle<Result<(), RuntimeError>>`。默认入口拥有并在返回时销毁各自的 `std::sync::mpsc::Receiver`；测试中的 `WorkerHandoff` 可在普通返回或显式错误后交还 receiver，但真实 World 仅在 Native `cfg(test)` 使用共享观察槽，不通过 join 返回接收器。`World` 的正常 shutdown、fatal cleanup、prepared stop，以及 Native/WASM 构造失败清理均消费同类 handle；替换结果类型必须逐一覆盖，不能只修改常规 shutdown。

`GroupIn::Batch` 可携带交易部发出的资金和股份实体，`DeskMsg::Ops` 可携带组已实际扣除的资金或股份。排空队列只转移消息所有权，不代表交易成功、资产归还或可以重放。发生 fatal 后保留的原始消息须与账户/订单簿回滚记录一起界定，防止重复计算。

## 接收器存活会改变发送语义

真实组线程的 Native 显式失败测试在 worker join 返回后仍保留 receiver：此时再次发送的 `CashRefund` 批次返回成功并进入队列，但没有活跃 worker 消费它。sender 存活时清理方按队列顺序取得两个后续批次，随后释放 sender 并观察到断连。这证明单纯延长 receiver 生命周期会延长 send 可成功的时间，不能把 send 成功当作活跃 worker 已接收或已处理；也不能只看队列暂空判定清理完成。测试使用人工注入的批次，并未证明真实生产方会发送这笔退款。

接入设计必须明确：worker 退出与发送许可关闭的先后关系；与正在并发的 send 的竞态允许哪些结果；失败发送的原始载荷由谁持有；已成功入队的消息如何与 sender 停止、队列最终断连及 Final 对账。任何逻辑关闭机制还须实际测量热路径成本，且不能静默丢弃资源或把诊断消息重放为业务交易。

## 发送准入试验

`worker_admission.rs` 是仅 `cfg(test)` 编译的候选：共享原子字记录关闭状态和已准入发送数，关闭前取得许可的发送允许完成，关闭后不再接受新载荷；发送正常返回、显式错误和 Native unwind 释放许可。关闭且计数归零只表示该准入域内没有发送正在执行，不表示队列为空、sender 已释放、消息已处理或资产已对账。真实组 flush 的 Native 测试使用已关闭但 receiver 仍存活的候选 sender，验证原失败委托和尾部返回；它没有接 World，也没有运行真实撮合或资产回滚。

该候选仍未建立生产可用性：必须从构造接口保证同一队列的所有 sender 共享同一准入域，不能保留绕过准入的原 sender；`SendError` 目前不能区分逻辑关闭和底层断连。WASM hard trap 可能跳过许可析构，计数永久非零，清理需要 deadline 和显式 incomplete，不能强行复位冒充安全。Native unwind 计数归零不证明传入发送体的资源载荷获救。尚未测量原子操作的热路径成本或浏览器行为，当前没有更改生产通道。

Native 测试先因缺少候选类型编译失败，再完成实现；准入 5 项、组退出与 flush 10 项、mailbox 8 项均通过。两个 sender 克隆同时进入底层发送后才关闭，验证两者结束前不宣称收束；另有 32 轮并发关闭试验，逐轮比较成功集合与入队集合并核对失败载荷。该试验不保证覆盖所有交错，排序后比较也不证明多 sender FIFO。独立复核未发现本次试验新增必修问题。

成对构造候选现仅从 `FaultDomain` 获取已登记通道，并把原始 sender 封在私有包装内；同一配对的 sender 克隆共享准入状态。真实组入口的 Native 测试在人工注入错位批次后发生显式错误，返回时关闭准入，旧的人工退款批次仍可取回，新的退款批次被拒绝并原样交还。该测试不经过生产 World，不代表真实资金交易或回滚。附加测试覆盖槽满/故障注册错误、槽随引用存活、已登记等待的 Native 故障域广播，以及关闭时既有许可完成并移交其队列消息。

`domain.abort()` 本身不关闭准入，在 worker 返回并关闭 gate 之前仍可成功入队；上述测试固定验证了这一窗口，接入清理必须消费这些载荷。接收器 `Drop` 仅关闭准入并立即释放队列，不等待已获许可的发送，不可作为安全回收证明；Native unwind 或 WASM hard trap 也不能保证队列得到移交。成对构造只限制此候选 API，不证明生产其他通道不能绕行。WASM 宿主故障广播、host-ready、资源清理和性能均未验证。

本次复跑 Native 准入 9 项、组退出与 flush 11 项、mailbox 8 项、故障域 8 项，均通过；独立复核确认了上述边界。固定关闭竞态通过测试代码手动拆分许可获取与底层发送，并非完整 `GatedSender::send` 或真实 worker 集成；另有测试通过完整发送入口覆盖在途竞争。

成对接收器新增测试用 `drain_closed`：仅在 gate 关闭且在途许可为零后分批取出保留槽和队列；未收束时不取件，取满预算时要求再次检查。已关闭准入域下，即使 sender 仍存活，取到空队列也可表示该域内不会有后续入队，但不代表 sender 或故障域槽已释放。返回 `Drained` 的同一批可能包含消息，调用方须继续保有并处理；它只表示消息已移出队列，不代表业务执行、资产对账或整个 World 清理完成。固定竞态测试把已获许可的发送暂停在入队前，验证关闭时无法提前完成，放行后取得原载荷；该屏障通过测试代码拆分许可与底层发送，不是真实 worker 并发场景。真实组显式失败测试也使用此接口取回人工退款。独立复核通过，panic/trap、共享 deadline 和生产 World 仍未解决。

本批 Native 定向回归：准入 11 项、组退出与 flush 11 项、mailbox 8 项通过；未运行 WASM 或性能测试。

## 使用发送端生命周期证明收束的核对

为探索不在每条消息上增加原子计数，测试性 `drain_for_shutdown` 现通过静态分发同时支持 std 与 LibOS receiver：`Empty` 仍报告 sender 存活，只有取到 `Disconnected` 才说明该队列中已无消息且全部 sender 已释放；预算取满也需再次检查。它没有改动热路径发送，也尚未接入生产 World。

Native 真实 World 测试人为保留一个组 sender 克隆、使组因错序消息失败、sever 并 join 两个真实 worker，再通过测试观察槽取得组 std receiver。此时 drain 为空但报告 `SendersAlive`；测试随后通过该克隆人工发送一笔退款消息，释放克隆，再由 drain 取回消息并报告 `Disconnected`。这证明本例中所有 worker 已 join 仍不能推断所有 sender 已释放；要依赖 sender 生命周期避免逐消息计数，必须把协调者、各 worker、构造 guard 和任何外部克隆纳入所有权清单并证明都已释放。测试退款并非真实业务产生，观察槽也会延长接收器寿命；断连只证明此队列无剩余 sender/消息，不代表资产对账、WASM 宿主结束或完整 World 已安全清理。

本批先用真实 World 测试确认 std receiver 无法调用旧清理接口的编译失败，再泛化接口。Native World Calendar 18 项、mailbox 8 项、准入 13 项通过；默认及 `libos-upstream` WASM 编译检查通过，未运行真实浏览器或性能测试。独立复核确认测试边界和泛化行为。

进一步使用 Native 测试观察槽和 `NativeConstructionGuard` 注入，在 2 组、2 交易部拓扑分别启动 0、1、2、3 个 worker 后触发构造失败；另在已启动 3 个 worker 时注入 group-0 的真实 Continue 错序作为次生故障。各例验证构造首错、已启动 handle 的 join 数量、次生错误数量与身份，以及已启动 receiver 身份、空队列和 `Disconnected`。这说明在这些受控构造失败中，构造返回后观察到的已启动 std receiver 均已没有 sender；并不能外推到四个 worker 均启动后的失败、真实 OS spawn 失败、panic、携带资源的队列或 WASM。receiver 仍是 worker 返回前移入测试共享槽，不是 join 返回。独立复核通过；Native World Calendar 19 项及 mailbox 8 项回归通过，本批未运行 WASM 或性能测试。

## 接入与验收清单

1. 形成可区分组和交易部 receiver 的有类型退出值，保留原 `Result`、worker 身份和接收器所有权；处理 join panic、WASM hard trap 及构造中途失败，明确不可移交时的诊断。不能通过假 Final 冒充资源回收。
2. 核对正常 Shutdown 必须走原消息协议并产生 Final；fatal 清理则保留原错误与次生诊断。`PendingStartBeforeFinal` 必须先结束组生产者，再向交易部发送 Abort / Shutdown；不可把这种次序改成无条件等待所有 sender 断开。
3. 协调停止仍存活的 sender 与接收器，按保留槽、队列顺序分批移交；最终断连与暂空、预算耗尽分别报告。使用统一的外部可验证 deadline；panic/trap 不能假定 Rust 析构一定执行。
4. 先用失败测试固定正常退出、显式错误、发送并发退出、真实资源批次、部分构造失败和原错误保留，再实现并由未参与实现的 subagent 独立复核。真实浏览器验证通知、故障唤醒、join、清理及超时；Native 测试不能替代 WASM。
5. 下行正式启用前，以相同场景测量吞吐、CPU、等待与完整产品路径，并检查 FIFO、资金/股份、日界、存档以及 Native/WASM 业务语义。不得以此工作记录代替这些验收。

## 带共同 deadline 的发送收束（2026-11-11）

将准入状态机抽到 `libos::admission`；关闭后已获许可的发送仍可完成，最后一个许可释放时通知等待者。`wait_closed` 只接受一个等待者，并使用调用方提供的绝对 deadline；未关闭、已有等待者、超时分别显式返回，不能把暂空或泄漏许可当作收束。WASM hard trap 可能跳过许可析构，此时只能报告未完成。等待者在检查时钟后仍可能被调度，后续 drain、核对与发布仍须再次检查同一 deadline。

新增 Native 合成测试：持有许可、关闭准入、到期后拒绝宣称收束，释放许可并在新 deadline 下恢复；在成对通道中人工构造含 500 分现金退款与 100 股退款的批次，测试关闭后分批移交及再次 drain 为空。它不是真实成交或账户对账；此测试直接用内部 sender 入队、独立持有许可，真实在途发送竞争另由既有 held-sender 测试覆盖。review 指出已收束但期限已过仍成功的问题，补失败测试重现后改为先检查期限；复核确认修正及单等待槽边界。

Native LibOS 准入 4 项、成对准入 16 项、Worker mailbox 8 项、World Calendar 20 项、错路由 3 项通过，测试命令均有 10 秒进程树期限；真实 `web-wasm` 默认和实验配置 cargo check 通过。原型的 `close_and_wait` 及排空尚未接入正式 World；本批没有浏览器故障、真实交易守恒或完整产品性能证据。

使用 [独立 Native 三臂入口](admission-bench.rs) 重新编译，与此前一样每轮各发送 100 万条，plain、仅关闭标记及有在途计数分别轮换，两个进程各六轮、外层 10 秒期限，均观察退出 0。[运行 1](admission-notify-bench-1.log) 的 plain 20.32–47.02 ms、计数 40.45–47.02 ms；[运行 2](admission-notify-bench-2.log) 的 plain 19.91–46.33 ms、计数 41.53–63.75 ms。此微基准包含线程调度、接收方自旋、逐条检查和 join，非每次 send 延迟；结果波动且不代表生产 World、WASM 或功耗。新通知只在关闭后的最后一个许可释放时执行，正常每条发送仍有 CAS 与释放原子操作，实际性能须在生产接线后验证。采样后摘要：`admission.rs` 为 `5a753c1a10ab1cff273d7cc73449b590a4aab5124850180ab3e817040648f4a2`，`worker_admission.rs` 为 `af59e9652f4f824a806b18d1ed7d4a43515bc639edecb54888e88a665b1753a8`，入口为 `6fdad2a6b2008d089d81eb8978316be897fec56346885b400c10dc3a094a7831`，二进制为 `7f5e0b300527c7f428c35c537a64a7f02b883b06afb387f8322438ac25c30b1c`；采样后源码曾增加仅测试用断言，未冻结完整构建闭包。

## 生产发送准入接线审查（2026-11-11）

尝试将成对的发送准入包装直接接到现有下行入口时，独立复核指出仅在 receiver 的 `Drop` 中关闭准入并不安全：关闭前获得许可的发送仍能成功，receiver 随即销毁时该消息可能连同资金或股份载荷一起丢失。反之，如果延长 receiver 生命周期却不在 worker 返回前显式关闭准入，退出后发送仍可能成功。由于完整的 join 移交、等待收束及资产对账尚未接入，已撤回该生产接线尝试；不能将逻辑关闭视作资源回收。尝试时编译还暴露构造失败、现有测试替换通道及退出观察槽等调用方依赖原始类型，需要在生命周期方案中一并迁移。撤回后 `cargo test -p engine --lib --features verification-harness --no-run -j 8` 编译成功，未执行测试或性能验收。

## Rust unwind 退出移交候选（2026-11-11）

新增 LibOS 退出结果：在 Rust unwind 可捕获时保留原始 panic payload 和 receiver，并通过 receiver 自身提供的 Admission 关闭准入。成对通道测试覆盖 Native 线程 panic、保留的在途许可导致清理超时及不能排空、许可完成后取得消息；资金和股份样本是人工构造，并非真实业务对账。该入口尚未接入正式 World；通道关闭不等于已收束，调用方仍须等待和核对。WASM hard trap 或 panic abort 不保证执行此过程。

后续新增 `libos::downlink` 成对通道，发送端和 receiver 通过私有构造绑定同一 Admission；`worker_exit::run` 限定使用该 receiver，避免由调用方传入不相干的准入门。收束后 `Closed` 使用共同 deadline 分批移交，超时结果携带已取消息，未取消息留在队列；可控时钟测试覆盖中途到期。真实组循环的一项受控失败测试改用该通道，尾部退款仍是测试人工注入。Native 相关 23 项测试通过，默认 WASM cargo check 通过；没有运行浏览器或性能验收。独立复核通过上述范围，同时指出 Drop 未回收、故障广播到 worker 返回期间仍可发送、WASM trap 与资产对账均须在正式接线时处理。当前正式 World 仍未使用此通道。

针对故障广播后仍可发送的问题，成对发送端在取得许可后检查自身槽的故障标志；观察到故障时返回原始载荷。受控竞争测试确认故障前已通过检查的发送仍可成功，清理须等待该许可结束；广播完成后新发送被拒绝。故障广播先标记全局状态再逐槽标记，广播进行中尚未标记的槽仍可能接受发送，不能称为全局故障即时关闭。另有真实组循环测试在启动前注入故障及人工资金股份批次，验证 HostAborted 和消息移交，不代表运行中广播唤醒或业务对账。Native 相关 29 项测试通过，默认 WASM 编译检查通过；未测性能或运行浏览器。独立复核确认上述边界。

进一步让已登记的 sender 先检查其故障域标志，再检查槽标志；确定性测试在故障域已标记、槽尚未通知时确认载荷被拒绝并返回。WASM 宿主仍先写全局故障位再写故障域标志，两者之间的窗口尚未覆盖；已通过检查的在途发送仍须收束。独立复核确认槽复用和语义边界，新增原子读取的性能尚未测量。Native 首次并行测试因期限监督器执行 `ps` 超时而未获有效结果；确认无残留进程并降低并发后重跑，故障域 9 项、下行 3 项、真实组 1 项及 mailbox 21 项通过，WASM 默认 cargo check 通过。正式 World 仍未切换。

实例级故障检查已收进 LibOS，WASM 使用全局故障标志与 tombstone DEAD，Native 为 false；现有协调者上行改为调用该共同入口，行为不变。成对下行在发送许可后先检查该入口：模拟实例故障先于域标记的 Native 测试确认返还原载荷；检查通过后发生故障仍允许在途发送完成。下行 4 项、故障域 9 项、上行 3 项与真实组 1 项 Native 测试通过，WASM 默认 cargo check 通过；独立复核确认模拟标志不能替代浏览器实际广播或 DEAD 验证。新增读取的开销及完整 World 性能尚未测量，下行仍未接入正式 World。

实验通知通道的非阻塞 Worker 收件与故障检查现由 `libos::worker_wait::try_receive` 负责：取件前后检测故障，竞争时保留已出队消息及后续顺序，runtime 只映射带 Worker 身份的 Seal 错误。实验收件调用统一实例故障入口，不再在该调用点按平台选择检查；正式默认下行仍走旧 std 与 watchdog。Native 相关 25 项测试通过，WASM `libos-worker-experiment` 配置 cargo check 通过；独立复核未发现行为变化。本次未运行浏览器、真实资产对账或性能测试。

成对下行的 `run` 现向 Worker 借出成对 receiver 而不是裸 mailbox，阻塞与非阻塞取件经其 LibOS 方法进入共同 WorkerInbox；真实组候选测试因此直接使用该封装。独立复核发现 Native 非测试配置缺少 HostAborted 错误变体，现扩展为跨平台的故障域或宿主错误，同时使 shutdown 诊断在 Native 提取 Worker 身份；World 其余 Native 故障终态分支尚未统一。Native 正式 cargo check、WASM 默认及实验配置 cargo check 通过，相关 Native 13 项测试通过。首次两项测试自身通过但期限监督器 `ps` 超时、进程树终止未确认，核实无残留后重跑成功。该通道仍未接入正式 World，未运行浏览器、资产对账或性能验收。

World 的 HostAborted 终态、Seal/收束、日结撤销、行政清理及复合错误不再按 Native/WASM 编译不同分支。Native 正式 cargo check 与 WASM 实验配置 cargo check 通过；基于既有 Native 故障注入的 8 个单独短用例通过，覆盖 Seal、Drain、日结撤销和诊断保留。整组 World Calendar 与 Shutdown 回归各在 10 秒外部期限内超时，不能报告整组通过；已拆分运行上述代表用例。曾尝试以外部集成测试直接构造错误，但 RuntimeError 是内部类型而编译失败，该测试已删除，未改变公开 API。独立复核未发现本批 A 股日结语义变化；正式切换通知下行前，Native 永久停止仍须主动唤醒互持 sender 的空闲 worker，否则 join 可能卡住。未证明真实资产回收或完整性能。

新增 LibOS 停止唤醒：mailbox 与成对下行仅通知等待字，不入队控制消息；Native 停止与部分构造 guard 使用共同 wake 接口，guard 持有发送端直至移交或唤醒。双线程互持 sender 的 Native 通知等待测试在双方登记等待后验证停止均能退出，另一测试验证队列仍只有原业务载荷。曾尝试向 Native std 队列发送 Shutdown 唤醒，真实 World 保留 receiver 的回归发现多出控制消息；该尝试已撤回，相关原有断言未修改。当前 Native 旧 std 唤醒仍为 no-op，依赖旧超时等待；WASM 旧停止 helper 仍发送 Shutdown，故不能声称两端已统一为通知式停止。Native 相关 11 项短测试、Native 正式 cargo check 与 WASM 默认和实验配置 cargo check 通过；独立复核确认上述范围。未运行真实浏览器、正式下行性能或资产对账。

WASM 停止 helper 现在通过 LibOS 唤醒接口适配：实验 mailbox 与成对通道通知等待字，不插入 Shutdown；旧 std 通道仍使用 Shutdown。独立复核确认停止标志先于通知、全部通知后再 join；Native 10 项短测试、Native 正式及 WASM 默认和实验配置 cargo check 通过。真实浏览器重测未完成：release 和 dev 两种独立诊断包构建分别达到 290 秒外部期限，监督器因 `ps` 超时未确认完整终止；随后核对并终止各自残留 cargo/rustc，确认进程消失。不得使用旧包冒充本轮浏览器验证，也不得以开发构建推断性能。正式 World 默认下行仍未切换，资产清理仍未证明。

真实组发送失败后，`GroupState::final_state` 改为一次性将 `undelivered_ops` 诊断槽移入 Final，不再克隆该槽；新增断言先观察旧实现在原槽留存而失败，再验证槽清空、失败批次/尾部内容和上下文不变及第二次不再移交。独立复核确认未改变账户、撮合、回滚或状态哈希；该载荷仍只是诊断，不可重放或与账户资产重复计数。现有 GroupFinal 上行发送失败会丢弃 `SendError` 内 Final，故不能据此声称故障安全回收。WASM 实验配置 cargo check 通过；两项 Native 针对性测试首次因监督器 `ps` 超时未获得有效结果，确认无残留后重跑通过。

现已将 GroupFinal 发送失败的原始 Final 包入 `GroupFinalUndelivered`，连同 Worker 身份及可选首错随线程 join 返回；Arc 共享同一诊断载荷，错误按载荷身份比较，不能据此重放或重复计入资产。受控组测试在线程启动前以人工买单调用实际 flush 并使下行断连，再断开上行并启动真实组线程；检查 join 返回的 Final 保留账户、股份、未发送委托诊断及可选首错。真实 Native World 测试丢弃上行接收端后停止并 join 四个 worker，确认两组 join 错误携带对应 Final 与账户快照；不验证真实交易或最终对账。停止诊断测试确认文字保留首错和 worker 身份。独立复核通过上述范围并确认 `runtime_issue` 等现有后续路径仍可能将错误字符串化而丢失结构化 Final，故不能声称已实现端到端回收；上行 TaskDied 发送失败的 panic payload、DeskFinal、WASM trap 和接收器移交也未解决。Native 相关 12 项短测试、Native 正式配置及 WASM 实验配置 cargo check 通过，未运行浏览器或性能验收。

`ShutdownWorkerResult` 现同时保留 Rust 侧原始 RuntimeError，并提供 `retained_group_final()` 只读访问；原 result 字符串仍用于 wire，结构化字段 `serde(skip)`，错误文字明确文本不含载荷。真实 Native World 和 GameSession 的受控上行断连测试分别验证 Final 经 join 留在停止 completion 和 Session 失败结果中；Native World 停止 11 项与 Session 停止 6 项通过，Native 正式和 WASM 实验配置 cargo check 通过。独立复核确认该证据只覆盖 Rust completion 存活期间，不代表资产对账、可重放或浏览器可取回；wire 字段形状未变，但错误文本已变，且公开 `ShutdownWorkerResult` 增加私有字段会破坏外部结构体字面量构造的 Rust 源码兼容性。其他将错误提前字符串化的路径、构造 guard、DeskFinal、panic/trap 和在途 receiver 仍需处理。

交易部 Final 发送失败现同样随 join 保留原始 `DeskFinal`、Worker 身份与可选首错；Rust 停止报告增加只读 `retained_desk_final()`，wire 仍不传输结构化载荷。独立复核要求将交易部状态的未发送批次诊断一次性移入 Final：测试先在旧 clone 行为下失败，再验证 take 后诊断槽清空且重复 Final 不再移交。另一项真实交易部循环测试人工投递超价买单，使退款发往组失败、Final 发往协调者也失败，检查 join 保留退款诊断和首错；不代表真实产品完整交易或资产对账。Native 交易部 9 项、World 停止 12 项和 Session 停止 6 项通过，Native 正式及 WASM 实验配置 cargo check 通过；独立复核未发现本批新增阻断。WASM wire、trap、TaskDied、构造 guard 与未消费 receiver 尚未完成回收。

## 永久停止的发送准入（统一 LibOS 计划第一批）

成对下行的 `wake_for_stop` 原先仅通知等待者，在 worker 返回并关闭准入前仍能接受新消息，与计划中的停止收口不符。先新增断言并观察停止后发送错误地成功；现由 `downlink::Sender::stop` 先关闭共享 `Admission`，再通知接收者。先前已取得许可的发送仍可完成，清理方必须等待许可收束，不能仅凭 stop 推断队列已排空。

隔离 Native 测试程序直接编译当前 LibOS 源码，覆盖停止后拒绝新消息、原消息保留，以及被暂停的已准入发送在停止后完成、等待期限内不得提前排空；全部 53 项通过。该程序不是正式 World 或浏览器验证。正式默认下行仍未切换，实际业务资产对账和 WASM trap 仍未验证。

独立复核未发现本轮必须修复的问题。`worker_stop::wake_all` 先设置全局停止标志，再逐通道执行 stop；只有各通道的 stop 执行后，才能确认该通道的新准入已关闭，不能声称所有通道在设置标志的瞬间同时关闭。正式接线时须区分仍需发送业务 Abort/Shutdown 的路径，避免提前关闭通道。Native `cargo check -p engine --offline -j 8` 通过。

## 有类型 Worker 退出值（统一 LibOS 计划第一批）

新增 `World` 内部的 `OwnedWorkerExit::{Group, Desk}`，各自包含 `WorkerId` 和对应消息类型的 `worker_exit::Exit`；`group` / `desk` 构造入口调用成对 receiver 的 `run`，在普通返回、显式错误及 Native unwind 后带回 receiver 和原始结果。新测试先因退出类型不存在编译失败；实现后 Native 5 项定向测试通过，覆盖类型和身份、显式错误、原始 panic payload、正常真实 Group/Desk 循环及剩余消息。真实循环样本使用 Shutdown 和 Echo，未覆盖真实资产对账。

首次构建因共享 Cargo 锁等待占用期限并超时；监督器未能确认残留，检查发现 rustc 已被重父化，随后已自行退出；产物不含新测试，未作为结果。确认退出后重新以 8 Cargo jobs、290000ms 外部期限编译完成，定向测试使用 10000ms 外部期限、8 测试线程。独立复核及补测后复核未发现本轮必修问题。当前类型尚未接入生产 World 的 spawn/join 或构造 guard；WASM hard trap 不保证返回此值，资源收束与对账仍未完成。

## 上行断连时保留 TaskDied（统一 LibOS 计划第一批）

原 Group 三处及 Desk 一处 panic 分支忽略 `TaskDied` 发送错误，会丢弃 `SendError` 中的原始 panic 载荷。现经 `upstream::send_task_died` 在发送失败时把原对象移入 `RuntimeError::TaskDiedUndelivered`，并在随后的 Final 发送失败时作为首错保留；`RetainedPanicPayload` 在克隆错误间共享对象且只能取出一次。Rust `ShutdownWorkerResult::take_undelivered_panic` 可从该停止结果的结构化错误及 Group/Desk Final 首错提取载荷。成功发送仍由上行接收者取得原对象。

先观察新失败路径缺少 API 的编译失败，再实现。Native 上行 helper 两项、正常 TaskDied 停止一项及真实 World 上行断连后触发 Group panic 一项通过；另有既有 World panic 原载荷恢复与幸存者 rollback 测试通过。真实断连用例确认 GroupFinal 和原 String panic 均留在停止 completion，且载荷不能重复取出。Native `cargo check --tests` 和 `nightly-2026-09-05` WASM `libos-worker-experiment` cargo check 通过；均非真实 WASM 运行验证。独立复核及补充真实 World 测试后的复核未发现本轮必修问题。

局限：仅在 Rust 停止 completion 存活时可通过新入口取回；现有 `serde(skip)` 不把对象发送到 Web/FFI，fatal cleanup 尚不自动 `resume_unwind` 此嵌套 panic。真实 Desk panic、WASM trap、在途资产对账和生产接收器移交均未完成。`RuntimeError` 新增公开 variant 可能使外部穷尽匹配不再编译。

## 失败清理恢复未送达 panic（统一 LibOS 计划第一批）

World `stop_after_fatal_failure` 的 join 路径此前没有检查 `TaskDiedUndelivered`，真实上行断连后将原始 panic 降为普通协议错误。现先保留已有 `original_panic`；若尚无待恢复 panic，则从 join 结构化错误取出一个未送达载荷并按既有流程 `resume_unwind`，其他未取出的载荷仍留在相应 worker 结果。按 join 观察顺序选择不等于证明 panic 的实际发生时间。

真实 Native World 测试先断开上行、触发 Group panic；旧路径失败后修正，再验证恢复原 String 且 GroupFinal 保留。另用人工传入的 `original_panic` 与真实 Group panic 验证前者优先、后者仍可从停止结果取回；不代表两个 worker 均真实 panic。两项定向测试、既有正常 panic 运输及上行断连停止测试通过；每项以 10000ms 外部期限运行。WASM `libos-worker-experiment` cargo check 通过，独立复核及补测后复核未发现本轮必修问题。真实 WASM trap、Desk panic、完整资产对账和正式接收器移交仍未验证。

## join 错误的平台边界（统一 LibOS 计划第一批）

将 Native panic payload 与 WASM `io::Error` 的 join 错误描述归入 `libos::thread::join_error_detail`；`World::join_worker_results`、prepared World 停止及 Native 构造清理使用统一入口。上行 `TaskDied` 的 panic 描述也复用 LibOS 的 `panic_payload_detail`。函数只借用错误，原始 `Box` 不被 helper 消费；现有调用方后续是否保留该对象没有因此改变。

先写测试并观察缺少入口的编译失败，再实现。独立编译当前 `thread.rs` 源码的 Native 测试 6 项通过，包含真实线程 panic 后生成描述并对原对象 downcast；真实 prepared World 3 项及停止 completion 15 项通过。`nightly-2026-09-05` WASM `libos-worker-experiment` cargo check 通过，不代表真实浏览器运行验证。独立复核及补测复核未发现必修问题。正式 World 的成对下行、receiver 移交与构造 guard 仍未完成。

## 统一 World 构造 guard（统一 LibOS 计划第一批）

Native 与 WASM 的 `World::spawn` 现使用同一 `ConstructionGuard` 持有构造期 sender 与已启动 handle、显式处理 spawn/Calendar 绑定错误，并在成功时把唤醒端和 handle 一起移交给 World；`sever` 与 Drop 在同一份 World 代码中调用 LibOS 唤醒入口。原 WASM 专用 guard 仅保留为 `cfg(test)` 下的旧合同测试，不能作为新 guard 的 WASM 运行证据。成对下行尚未切换，仍需后续删除旧等待和控制消息适配。

Native 测试中，统一 guard 2 项、部分构造失败、正常关停接收器、组失败后未消费批次、外部 sender 持有各 1 项、停止 completion 15 项、prepared World 3 项均通过；原有测试断言未弱化。实施中首次统一停止错误地向 Native sever 队列追加 Shutdown，导致两项回归；随后分别以 `wake_for_stop` 和 `wake_for_host_stop` 表达 sever 和 Drop 的既有差异，重跑通过。构造 guard 在非 unwind 的遗漏清理且 worker 失败时显式报错；错误文案中性并将 `joined_workers` 解释为已消费 handle 的数量，不表示 Worker 成功退出、宿主 child 释放或资产回收。

独立复核发现 WASM mode 4 fixture 仍匹配旧 `SendFailed`，已改为严格核验 `ConstructionFailure` 的主错误、4 个 handle 和 group-0 原始次生错误；复核确认断言未弱化。默认与 `libos-worker-experiment` WASM cargo check 均通过。仓库中未发现可直接运行该 mode 的浏览器 runner，本次未运行真实浏览器 mode 4；不能以编译通过代替宿主验证。WASM 构造错误的公开 Rust 类型由 `SendFailed` 变为 `ConstructionFailure`；Web 宿主源码中未发现对这两个 Rust variant 的直接匹配，但端到端兼容尚未验证。正式 World 的 `OwnedWorkerExit` 接线、receiver 移交和在途资产对账仍未完成。
