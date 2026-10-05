# P1：日终 NPC 决策生成边界

## 真实问题与依据

原 representative producer 保持 5 stocks、26 NPC、seed `666959854`、每天 60 ticks、两个交易日。真实 tick 120 日终候选仍有 NPC Buy Highest 200 股，公共 `ProtocolSession::restore` 明确拒绝待受理输入；日志 `.tmp/current-schema-save-protocol-final-run.log` 与 `.tmp/protocol-archive-restore-probe-old-candidate.log`。不换 seed、降低 NPC 数量、手清候选字段或放宽 restore。

依据为 ADR-0025 的公共完整日终档无日内待受理请求契约，以及既有 `TradingDayEndTransition` 当日订单失效边界。此修复不新增交易制度：继续遵循 `docs/trading-rules.md` 已记录的沪深规则基线、当日有效委托与跨日个人计划差异，不冒称重新核验官方条款。

## 根因与最小修复

`prepare_candidate_commit` 对每个成功推进的市场 tick 都调用 `queue_npc_for_next_tick`。最后交易 tick 已完成真实撮合、订单失效、结算与 `day` 推进后，仍额外捕获 NPC snapshot、推进 attention / Strategy 并分配下一批 receipt，重新生成日内待处理事实。

仅在 `candidate.day > authority.day` 的真实完成日界创建合法空 `PendingNpcBatch`，`observed_tick` 保持当前市场 tick，供下一交易日 `take_ready_npc_batch` 消费。helper 严格拒绝旧批未消费及非完整交易日界；计算先转 `u64`，不对合法 tick 引入 `u32` 乘法上限。不先生成再删除，不回拨 receipt，不改变已发生订单、受理、交易和历史事实；保留 due attention、RNG 与跨日 plans。非日界 T-1 决策保持原路径，下一交易日空批之后正常生成并受理 NPC；休市自然日不伪造市场 tick 或观察。

## TDD 与验证状态

- root `host34` 新真实 3 Retail、正常 scheduler、arrival 1、seed 2、两日各 4 ticks 短测业务红：0.59 秒，失败为公共日级档含待处理输入，不是 fixture 无 NPC 受理。日志 `.tmp/checklist-wave4/host34-day-end-npc-archive-red.log`；红灯前未实施修复。
- 随后增强测试：每日日终空批和公共 restore，最后交易 tick 不推进 attention candidate / RNG，即时 restore / resave 深等值；原局与恢复局各自运行下一日 3 ticks，验证连续 tick、真实 receipt / cursor 不变量、真实 NPC 受理恢复。不要求并发未来顺序逐字节相等。
- 另有未消费批拒绝、非日界拒绝且完整事实不变，以及无需模拟大量日数的合法 `u64 tick` 边界测试。
- 非作者预审认可方案；第一次实施复核发现 `u32` 日数乘法上限，已转 `u64` 并补边界测试。最终独立复核和 root 统一绿色运行、原大 producer release 复验尚待完成，不提前报告通过。
- root `host35` 增强主 case 实跑仍红（1.14 秒）：每日日终公共恢复、空批及后续其他 guards 均通过，但下一交易日 3 ticks 内真实 NPC `OrderAccepted > 0` 断言失败。日志 `.tmp/checklist-wave4/host35-day-end-npc-archive-green.log` 保留其原命令命名与真实失败，不改名覆盖成绿色。三个 day-end guards 独立全绿（0.25 秒），日志 `.tmp/checklist-wave4/host35-day-end-npc-guards-green.log`。当前只新增完整 frame / pending NPC / attention / accounts 失败诊断，不删受理断言、不换 seed 或稀疏 scheduler；待实际诊断确定 fixture 或生产边界原因。
- 数值边界修复与非日界拒绝后非作者完整源码增量复核 PASS；该静态结论不覆盖上述仍红的运行验收。
- 后续 root 真实诊断（1.16 秒）保留 `.tmp/checklist-wave4/day-end-npc-resume-trace.log`：tick 9 / 10 / 11 的真实 `observed_accounts` 为 `[2]` / `[1]` / `[1,2]`，attention / RNG 持续推进，pending intents 均为空，只有 PriceTick、无拒单。这不是 queue 未恢复；Retail 的 arrival 参数还受行情活跃度、随机 side、可卖股份和个人 analysis 等影响，正常观察不承诺每 3 ticks 必下单。不能为了满足人为受理断言改 seed、补现金、forced trade 或修改 Strategy。
- 依 root 核定，校正本轮新增的“下一日必须 `OrderAccepted > 0` / receipt 必增”错误假设，改验真实 observed accounts 非空、attention candidate / RNG 持续推进及合法 tick / frame / pending / receipt / cursor 不变量。前两日真实 NPC 非零受理、每日日终候选公共恢复、即时完整 resave 深等和日界 attention 不消耗保持；等待非作者对该契约校正复核与实际绿色结果，不将诊断失败改写为从未发生。
- 校正经非作者复核 PASS；root `host36` 最终四个 case 真实全绿：主 case 1.25 秒，三个 guards 共 0.26 秒。两测试进程并行，每进程 Rayon 16 / test threads 4，整个批次外部 10000ms、每 child 9000ms 硬 deadline。日志 `.tmp/checklist-wave4/host36-day-end-npc-archive-green.log` / `host36-day-end-npc-guards-green.log`。原大 producer 同 seed / setup 的 release 重新生成及公共恢复回归尚待，不能把小 fixture 绿色外推为已经完成原大场景验收。
- 非作者 `review_shared_ingress_core` 已亲读上述两条 `host36` 原始日志并对最终源码与记录签核 PASS：公共恢复 1/1、guards 3/3，无待修复发现。该审查未独立运行 Cargo，明确保留原大 seed release 回归待验证边界。
