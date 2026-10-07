# E9 临时容器 churn：判据通过与惰性 checkpoint 语义证伪（无生产改动）

## 判据结果：通过（脊柱 churn ~19.5% wall）

- 依据 gdb 1811 组采样（4 worker 真实运行，日内为主体）：actor 串行脊柱清醒 654 采样中 clone/drop/释放机械占 ~352（**54% 脊柱清醒时间、19.5% 总 wall 采样**），远超 4% 门槛。worker 并行车道 churn 极少（与 P0-dirtykey"车道省 CPU 不涨 wall"互证）。
- **callgrind 归因修正**：fair-400t 窗口里 SaveSlot 析构 142.7M IR（33%）经查来自日边界 `day_end_save`（探针不发 SessionCommand::Save；日终才构造 SaveSlot）——属用户明确"无所谓"的边界成本，**不得计入日内 churn**。日内脊柱 churn 的可靠数字以 gdb 采样为准。
- 脊柱 churn 构成（actor 叶帧）：clone_subtree 138（BTreeMap 克隆）、drop 71 + dying_next 29 + drop_slow 14（析构）、sdallocx 16 + tcache_flush 13（释放）、memcpy 21；调用方：clone_for_tick_shadow 53、runtime_snapshot 28、cancel 20、其他 clone 16。

## 双克隆现场与惰性方案证伪（本轮核心产出）

`ProtocolSession::step_frame`（session/protocol/civil/session.rs:263-283）每 tick：
1. `checkpoint()` 深克隆整个 GameSession（callgrind 归属 211.2M/窗口）；
2. `game.step_frame()` 内部 `plan_tick` 再克隆工作影子（111.7M）；
3. 成功路径 `drop(checkpoint)` 整份当场丢弃（脊柱析构）。

**证伪的惰性方案**：原设想"权威在不可失败交换前不被改动，失败只需清 poison"。读码发现 checkpoint 是**承重的**：`game.step_frame()` 成功提交后协议层还有 `prepare_frame`→`frame.validate()`，该处失败时回滚必须把**已提交的 tick 整体退回 tick 前状态**（现网测试 `checkpoint_rejects_poison_and_failed_candidate_can_retry_from_healthy_state` 即断言 business_state_hash 回到 before）。权威"不被改动"只覆盖 game 内部失败，不覆盖"提交后协议外层失败"。惰性方案语义不成立，未实现。

## 剩余可行路径（均汇入任务 #18 借用解耦）

1. **checkpoint 克隆与 tick 只读阶段重叠**：克隆读 `&GameSession`、tick 的 prepare 阶段也只读、首个权威变更在 candidate_commit——重叠安全，但被 `execute_authoritative_tick(&mut)` 整程独占借用挡住（与 X1 投机预计算同一堵墙）。**先做 #18，一次解锁两笔**（X1 外推 +16%、克隆隐藏上限 ~10% 量级）。
2. **逐字段 Arc/COW 化 clone_for_shadow**（LiveReservations/BorrowedEnvelope 是已兑现的同款先例 +9~11%/+10%）：下一轮先做"纯日内段 callgrind"（callgrind_control 分段避开日边界）拿干净的逐字段质量排序，再选 top 字段。
3. 延后析构专用线程（E7-X2 复活方案）独立小实验，仍排队。

## 结论

E9 本轮按"判据先行"纪律收官：方向确认（19.5% ≥ 4%），最大子方案（惰性 checkpoint）以半天成本语义证伪、零生产改动；无保留代码故无基准/复核需求。下一轮优先级：#18 借用解耦 > 日内段逐字段质量画像 > 拍卖增量复用（#17）。

产物：`.tmp/e9/`（本报告分析脚本与中间数据引用 `.tmp/expl-hotpath2/`）。
