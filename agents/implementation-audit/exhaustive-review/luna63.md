# EOF 状态记录独立全文复核

复核对象：deadline/status.md、fixtures1/status.md、fixtures2/status.md（分别 123、78、39 行）。产品基线 08e4fc7，merge 同。本次只读源码、历史复核及对应 status.json；未运行测试/长测，未修改产品或 Git 状态。按正式时限规则：普通单命令及单 case ≤10000ms；长验收每个 child 与从批次启动到终止、清理完成的共享 deadline ≤300000ms，最后 1000ms 留给终止/清理；适用 Node case 另须 10000ms case timeout。

## 章节矩阵与旧结论复核

| 状态文件章节（行） | 实际调用链 / 核查 | 对旧结论的复核与残余 |
|---|---|---|
| deadline 概述、hosts-N08（1–28） | runBoundedCommand → runWithDeadline / run-long-validation.mjs / run-web-tests.mjs / run-full-regression.mjs。run-with-deadline.mjs:8-17,27-42,68-90：POSIX child 使用 detached:true；终止向其 pid 的负 pid 发 SIGKILL；Windows taskkill unref。 | 对象生命周期与单 child 限时描述基本属实；review-deadline 也说未保证整个树已关闭。但每次 spawn detached 都创建独立进程组，外层对自身 child group 的 kill 不会传递到另一个 detached 后代 group。 |
| deadline hosts-R2-N19（30–50） | buildFullRegressionArtifacts、readAndValidateInventory、executeFullRegression；由内部 build/execute phase 调用。 | Inventory 所有权/校验结论与 R1 修复复核一致；与进程树监督无关，不能推出全回归满足共享期限。 |
| deadline 验证证据、复现命令、限制（52–100） | 修正后只重跑 4 项短测（115–119）；54/54 属修正前。历史 review 明确 55 项新全集未跑。 | 限定证据陈述诚实；EOF 说旧 worker 段落是历史记录，并不能证明相关路径满足整树清理。原生 detached 子进程复现只覆盖正常短退出，没有测试孤儿后代。 |
| deadline 深层 JSON 修正、root 最终验证（102–123） | R1 为 inventory JSON 接受范围；final 指向代表性编译/cases。 | R1 已有独立复核及 4/4 定向证据。产品 case 不验证 runner 的整树终止或全回归 300000ms 共享 deadline；55-case 最新完整短测也未执行。 |
| fixtures1 总述、hosts-N03/N04/N05（1–35） | N03 Scenario 消费者在 acquisition_gold、failures、view_gold；N04 WeekendScenario 用于 weekend_publish 两个用例；N05 SeasonedSaveFixture 用于 save_contract 主/失败模块。 | per-action “待验证”属实施时状态；EOF final_validation 后续更新：N03 两个选定 case、N05 cached_baseline_clones_do_not_share_tampering 通过；未见 N04 两个周末 case 的 selected 运行证据。不可把编译/静态检查扩成周末行为测试已执行。 |
| fixtures1 hosts-R2-N01/N02/N04（37–65） | N01 exposure/discovery/failures 消费者；N02 AuctionFixture 在 auction.rs；N04 BehaviorScenario 被 39 个行为测试使用。 | status.json 中这些动作各两个选定短 case 均通过。其余原测试虽保留，未全跑；代表性筛选不等于完整 suite。没有新增交易制度声明。 |
| fixtures1 静态核查、最终代表性验证（67–78） | build07/all-targets check08 是编译检查；case 结果在 status.json，逐案 10 秒。 | 历史“未执行 Cargo”与之后验证不矛盾。终段正确限定未选中场景/完整 suite/E2E/性能；未发现 fixture caller 漏迁的静态证据。 |
| fixtures2 总述及逐动作表（1–16） | N05–N12 callers/filter 均列于表格；N12 TestOrderSaveFixture 的主要 consumers 在 session.rs。 | “Rust待验证”是旧时点状态；JSON 的 final_validation 后续记录每项代表性 case 通过。不能说表内全部旧 caller 都被运行。 |
| fixtures2 验证/修订轨迹（18–35） | 静态 diff/review、DTO 与生产 getter 边界；serde 状态修改仍限测试 fixture。 | “没有运行 Cargo”“待 root”是历史记录；N12 新 fixture case 没有单独 TDD red/green 执行证据，后来 green 不反证 red 曾运行。最终编译与 selected case 已通过，范围仍以 case 清单为限。 |
| fixtures2 最终代表性验证（37–39） | 同 fixtures1，链接 build/check 与 selected cases。 | 编译、代表性筛选及 WS sandbox EPERM 核销与 JSON 证据范围一致；完整 suite/API doctest/真实 matrix、E2E、性能仍明确未执行。 |

## 新候选：deadline 树终止与共享预算

1. **独立 detached group 的嵌套监督空档。** 源码原文（scripts/run-with-deadline.mjs:68-90）：

   detached: process.platform !== "win32"
   setTimeout(() => {
     this.#timedOut = true;
     terminateTree(this.#child);
   }, timeoutMs - cleanupReserveMs)

   scripts/run-full-regression.mjs:606-646 由外层 runFullRegressionPhase() 用 300000ms child supervisor 启动内部 Node phase；内部 buildFullRegressionArtifacts() 再以 runBoundedCommand 启动 Cargo，各自 detached。phase 与 Cargo 属于不同进程组。若外层 phase deadline 先到而杀掉 phase，phase 的 JS timer 不会继续运行来杀 Cargo；外层 kill 也不包含 Cargo 所在组。内层 Cargo 的 remaining budget 与外层独立计时起点不同，且外层在最后 1000ms 已开始终止 phase，因此内层 kill 计时可能失去执行者。可留下 Cargo/compiler 后代占用 CPU、锁与临时文件，无法满足批次至进程树终止和清理的共享 deadline。现有测试覆盖单 child group，不足以证明嵌套 detached group 会级联终止。

2. **默认完整回归没有单一 300000ms 批次 deadline。** run-full-regression.mjs:606-623 的 runFullRegression() 顺序启动 build 与 execute；runFullRegressionPhase() 每次都配置 timeoutMs: LONG_VALIDATION_MAX_MS（628-646）。因此两阶段分别有 300000ms，上界可逼近 600000ms，违反从批次启动到终止/清理完成共享 300000ms 的规则。内部 phaseRemainingMs() 仅对当前 phase 计时，未共享 build/execute 的起点；外层函数只在结束后报告累计时长，没有共享 deadline。

3. **Web worker 有同类嵌套 detached siblings。** runWebTests() 以 10000ms supervisor 启动 --internal-worker（run-web-tests.mjs:143-153）；runWebTestBatch() 并发启动 shard children（105-129），每个 runBoundedCommand child 都 detached。worker 存活时 AbortController 可在 shard 失败后终止 sibling；外层硬 deadline 杀 worker 时不能依赖该 JS 协调，shards 的独立进程组可能残留。case timeout 与单 shard timeout 已配置，但跨 worker/shards 的整树终止/清理未获证明。

以上是源码可证的候选缺口，不是实测孤儿进程；本任务禁止长测，未启动故意超时的 Cargo/worker 探针。旧 review 说明未承诺确认整个 tree 已关闭，却未把此限制作为验收 blocker；应重新限定其生命周期结论。修复方向供主审：由单一外部 supervisor 管完整批次预算及后代进程组/Job Object；避免嵌套 detached 组，或传递剩余总预算并由真正包含后代的 supervisor 清理。只加内层 JS timeout 不能解决父进程先被外层杀死的问题。

## 汇总结论

- 三份记录中的动作、owner、调用者、代表性验证与范围限制总体能与源码/status.json 对上；不能把 selected case 扩大说成整套测试完成。
- Deadline 旧结论只支持单个 runBoundedCommand 管辖的 detached group 生命周期，不能支持多层 wrapper 的递归整树终止。
- 新候选：默认完整回归缺跨 phase 300000ms 共享 deadline；外层杀 phase 时可能留下 detached Cargo/Web shard 后代。当前源码不足以声称全回归满足硬时限规则，应修复监督结构或限时契约后复核。
- A 股：本审计是本地脚本和测试 fixture；未发现这三份记录描述的改动新增交易语义，也未重新查验交易所现行规则。既有 T+1、沪深差异和披露断言的代表性 case 不能扩成领域全覆盖声明。
