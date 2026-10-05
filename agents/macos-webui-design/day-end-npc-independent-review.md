# 日终 NPC 请求准备独立初审

2026-10-05。读取当前完整 engine diff、ADR-0025、NPC准备/消费、candidate commit、底层save验证和公共restore路径。未参与实现、未修改产品代码。主agent正在编译，未另启Rust构建或测试，遵守最多2case×Rayon4及预构建binary协调要求。

## 阻断发现

**P1：合法新日终档的 pending_npc=None 被底层恢复验证拒绝。** persistence.rs validate_save_slot 在 pending_npc 分支仅接受Some且observed_tick等于snapshot.tick，其余一律InvalidSave。GameSession::from_save先调用该验证，ProtocolSession::restore又先调用from_save；本批new及最后tick改为None后，新日终档尚不能成功恢复。应仅在合法tick日界允许None，日内仍强制存在已观察批次并保留时间、账户、依赖验证；补日内None拒绝反向测试，不能将所有缺批次都放行。已报告主agent。

## 三项门禁

1. 大 A 语义：本批不改变撮合、撤单、T+1、费用或公开披露时间制度。按ADR-0025，完整自然日结后公开档不能包含待受理日内请求；延迟到下一实际tick准备NPC符合该契约。尤其休市开局不应提前观察尚未完成的经营/披露状态。公共restore对非空pending的严格拒绝仍保留，不以清空事实掩盖错误。
2. 必要范围：new去预生成、最后tick不预生成、日界shadow惰性准备是同一根因的最小链条。日内继续使用前commit缓存，未引入第二套NPC行为生成。company_event_contract.rs和company_scenarios/main.rs两处纯rustfmt差异与本修复无关，已建议撤掉。queue_npc_for_next_tick函数注释仍只写CommitTick前调用，应补日界shadow调用契约。
3. 边界/观察与回滚：日界准备发生在已捕获隔离shadow内且早于ExpiryShadow，读取的是已提交日结后状态，失败丢弃shadow，不应消耗权威RNG/attention。新增测试包含真实非空NPC请求、休市与交易日日结、live/restore逐tick完整状态一致、首tick协议失败回滚，方向正确；但底层验证缺口必须先修复。institutional/publication依赖与旧fixture假设需在预构建定向回归中关注，不以UI验收代替engine验证。

结论：初审暂不通过，等待底层None日界验证修复及编译/定向测试结果后再次复核。未运行或编造Rust测试结果，不宣称真实默认NPC日终问题已经解决。

## 第二次静态复核（修复后，测试 fixture 仍在诊断）

2026-10-05 再次核对完整 7 个 engine 文件 diff。初审 P1 已关闭：`validate_save_slot` 只在 `context.day_tick == 0` 接受缺失 `pending_npc`，日内仍拒绝缺失批次；新增 `missing_npc_batch_is_valid_only_at_a_day_boundary` 同时保留 restore 和直接 step 的反向拒绝断言。主 agent 报告该测试已通过，本 reviewer 未独立运行。公共档禁止非空待受理输入的规则没有放宽。两处无关 rustfmt 变更已撤销，准备函数注释已覆盖日界调用。

针对额外关注点，`plan_tick` 中先生成日界批次再清空 diagnostics 没有发现新的权威状态漂移：`last_retail_decisions` 是投影产生的诊断输出，并非准备或策略输入；原有日内机制也是前一 candidate commit 准备批次、下一 `plan_tick` 清除诊断。日界调用继续在可丢弃 `TickShadow` 内、早于 `ExpiryShadow`；策略状态、attention、RNG 和请求批次的变化随 shadow 一起提交或丢弃。日内准备条件和消费验证未改，日界 `Some` 不会被重复准备，日内异常 `None` 不会被静默补齐。

本批仍保持修复日级档与下一交易日输入准备的最小范围，没有新增 A 股规则、费用或撮合优先级模型。依据仍为 ADR-0025 及既有统一 NPC 准备流水线，不需要以新的交易所制度假设支持该内部生命周期调整。

当前验收限制：主 agent 明确报告两个 continuation fixture 的实际 `OrderAccepted` / `IntentRejected` 断言仍红，正在修正确定性 fixture；attention 通过只证明发生观察，不证明策略一定产生请求。该断言不可删除或削弱。需待真实 NPC 请求、两种日结恢复、失败回滚与完整状态一致测试全通过，再作最终通过结论。当前临时诊断输出也应在完成诊断后清理。此次没有新增静态阻断发现，但尚未关闭整批验证门禁；未启动构建或测试，未使用 UI 结果代替 engine 证据。

## 第三次复核（continuation 通过后）

核对最终 8 文件 diff（含 ADR-0025 新增日界契约）及 `day-end-npc-related-tests.log`：主 agent 的预构建 binary 长验收记录为 115/115 通过、18.13 秒，4 个 case 进程并发，每 case harness=1、Rayon=1，case 与命令外部 10000ms，共享外部 300000ms。此为所阅日志证据，并非 reviewer 独立重跑；WASM 构建在途，没有启动 CPU 验证。

两个 continuation 测试现保留严格的 AccountId(1) 请求事件判定、live/restore 每 tick 完整 SaveSlot 相等、失败回滚与冻结日终候选断言；玩家买单不能冒充 NPC 事件。seed 与既有 fixture 调整没有弱化这些断言，临时 eprintln 已清除。ADR 描述与生产代码一致。

**P2 验证缺口：日界 due NPC 的新增准备分支尚未被这两个 continuation fixture 直接覆盖。** helper 把 attention 调度在 `tick + 1`；因此首个日界 `queue_npc_for_next_tick` 走 no-due 空批次捷径，实际决策与其失败回滚覆盖的是随后原有 precommit 准备。应保留这两个绿色测试，另外新增日界 `due=tick` 的小 fixture，确认 `plan_tick` 中确实发生 attention/策略捕获投影变化，再验证后续失败不修改权威状态及恢复后继续一致。此测试允许 NoSignal，不必伪造首 tick 成交；重点是不能用旧准备路径证明新路径。已告知主 agent，等待补测后复核。

当前没有新生产代码缺陷，但上述有效测试缺口尚待关闭。真实默认浏览器 save/refresh 验收也仍待完成，不能宣称该用户故障已完整验收或原完整目标完成。

## 最终复核：engine 批次门禁通过

新增 `due_day_boundary_npc_preparation_rolls_back_and_restores_exactly` 关闭 P2：真实 due=0 的准备探针明确得到 observed_accounts=[AccountId(1)]，attention 与原状态不同且下次观察晚于 tick 1；首个成功 tick 的 attention 必须精确等于日界探针。这避免把原有下一 commit 准备当成新增分支证据。malformed_frame 后完整权威 SaveSlot 未变、随后 live/restore 完整状态相同、公开日终候选冻结均有保留断言。1000 ticks/day 只用于隔离一次观察，实际只推进一个 tick，不形成长 fixture。

Reviewer 已独立运行预构建 binary 的四条关键 case（日界 due 准备、休市 continuation、交易日 continuation、缺失批次日界/日内验证），全部通过，总计 1.07 秒；并发 2、每进程 harness=1 / Rayon=4，每个进程与整个命令均设置外部 10 秒 deadline，未触发构建。证据为 `day-end-npc-independent-tests.log`。另核对主 agent 定向日志 116/116、23.38 秒，以及 production build 末尾 release WASM verified；`git diff --check` 通过。检查新增定向验证脚本/清单，单 case 使用现有进程外 deadline 工具，未静默忽略失败。

最终三项结论：A 股制度与单位没有改变，日界观察遵循 ADR-0025；生产修改局限初始/最后 tick 准备、日界 shadow 和匹配的恢复校验，没有新依赖或第二套 NPC 路径；此前 P1 实现问题及 P2 新路径测试缺口均关闭，没有剩余有效 finding。本 engine 批次可提交。

明确限制：真实默认 20007 NPC 局的 pending 输入错误据主 agent 实看已消失，但随后实际压缩存档触及 LocalStorage quota，界面仍明确报错。此为尚未修复的 Web 存储容量问题，不能把当前 engine 通过表述为默认局保存/刷新完整成功，也不能据此认定原完整终端目标完成。历史工作记录中的旧故障描述属于此前批次事实；本批应追加上述最新边界，不回写为从未失败。
