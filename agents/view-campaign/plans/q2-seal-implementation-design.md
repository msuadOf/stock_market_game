# Q2 Seal 收尾确认相位——实施设计（报批稿）

> 第 19 任起草 2026-10-18。依据：view-design-commit-ingress.md §1.2（S1-S6）
> + 本战役实证（acct 分片上行的时序竞态红证，见 §0）。**架构级改动，
> 批准后实施。**

## 0. 本战役新增的实证依据（为什么现在必须做 Q2）

acct 分片上行刀（已撤回）红证：组报告时点 **desk 终轮批次可能在途**
（组决策快→报告早于 desk flush 到达）——组端 undo∪staging 集不可靠
含终轮成交 owner。**修复=报告点后移到"全部批次已入组收件箱"**——
这正是 Seal 的 S4 锁步不变量。**无 Q2，EchoAccountsSparse 独立往返
（62μs/tick）无法退役**；同拍投影（D3）与双写（D2/D8）同理。

## 1. 实施范围（最小刀界）

按设计稿 S1-S6 落地 **Seal 相位**（终轮收尾确认）：

### 消息面（protocol.rs）
- 新增 `GroupIn::Seal { tick: u64, round_final: u32 }`（终轮 converge
  后由协调者广播——复用 group_txs 逐组 send 形态，零新通道）。
- 新增 `UpMsg::SealAck { group, tick, final_hash: u64 }`。
- **GroupReport 不动**（决策事实早发——S5：Continue 判定不等结算）。

### 组端（group.rs）
- `handle_seal`：drain 收件箱（S4：协调者已收齐全部 DeskReport ⇒
  批次 FIFO 已入队——组 drain 必然收全）→ `apply_staging(tick,
  round_final)`（进 undo 日志——**落账点从 Start 前移到 Seal**，S2
  落账点唯一化）→ 计算期末哈希 → SealAck。
- Start 的 apply_staging 保留为**防御核对**（Seal 后恒空批次——设计
  稿 §5 预判）。

### 协调者（world.rs run_ticks）
- 终轮 validator 过 evidence 后：广播 Seal → 收齐 G 个 SealAck
  （新 Phase 档：watchdog 扩 `Phase::Seal`）→ **install_tick（发布）**。
- **SealAck 后、发布前的失败**：Abort（组 undo 回放——S3：undo 跨
  发布无害，仍在下一 Start 清）。

### 驱动器（drive.rs）——拆迁连锁（Seal 落地后立即执行）
1. **acct 分片上行**（本次红证的正解）：组端 Seal 收尾时 undo∪staging
   （此时 staging 已 drain 落账=undo 全含）构建账户分片随 **SealAck**
   上行 → PublishedTick.groups[].accounts → EchoAccountsSparse 通道
   退役（62μs）。
2. **D3 同拍投影退役**：unsettled_trades 恒空（Seal 后无尾巴）——
   apply_same_tick_fill_projection/计划补写/记忆裁剪的"叠加增量"分支
   删除（组回写值已含本拍成交）。
3. **D2 退役**：account_dirty_prev 两拍并集删除（Seal 落账=同拍，无
   跨拍滞后）。

### wasm/串行
- 无 OS 线程——Seal 在串行路径=同线程直调 handle_seal（协议面不变，
  设计稿 §2.3 已建模）。

## 2. 行为合同（红测试先行，批准后第一批写）

1. `seal_settles_before_publish`（红证组 1a）：step() Ok 后立即从发布
   版本读账户——现金/持仓/T+1/成本基==期末组权威（无投影叠加）。
2. `continue_early_round_exactly_once`（1b，前置 #1 判据）：多轮成交
   恰一次落账。
3. `no_early_reuse_of_tick_resources`（1c）：Seal 期末落账不让本拍
   续行消费本拍释放资源。
4. `seal_ack_failure_aborts_cleanly`（3a）：SealAck 收集失败→Abort→
   tick 起终 business_state_hash 一致。
5. `unsettled_tail_is_empty_after_seal`：TickRecord.unsettled_trades
   恒空（D3 退役的直证）。
6. 突变红旋钮：注释 handle_seal 的 apply_stailing→1 号红；投影分支
   复活消费全量 trades→2 号红（双计）。

## 3. 性能预期（带数据回报）

- 消息增量：每 tick G 条 Seal + G 条 SealAck（B 口径 G=1：+2 消息/拍
  ≈ 2×7μs 唤醒——14μs vs acct 往返 62μs 净赚）。
- acct 通道退役：−62μs；D3 投影+补写+裁剪：−~30μs；D2 并集：−~5μs。
- B 1000 预期 750→**~870**（净机窗）；A 口径 Seal 轮的 wall 增量
  （预期小于决策轮的收敛等待——A 探针配对实测）。
- 风险：Seal 串行相位可能吃掉分段 flush 重叠（设计稿 1.4 实验 1）——
  A 探针配对；若 A 降>5%，评估 eager-apply 臂（等待窗内到达即落账）。

## 4. 分批提交计划（批准后）

1. `test(engine)`：行为合同红测试 6 条（Seal 未实现——编译红或断言红）。
2. `feat(engine)`：Seal 相位+SealAck+组端落账前移（合同全绿；A/B 测量）。
3. `perf(engine)`：acct 分片随 SealAck 上行+通道退役（红证=本次竞态
   复现守卫）。
4. `perf(engine)`：D3/D2 退役（unsettled 恒空断言+投影分支删）。
5. 每批：双模式全量+突变红+B/A 双口径净机 3 轮。

## 5. 开放点（实施中定）

- SealAck 与 GroupReport 的 watchdog Phase 预算切分。
- 多组形态下 Seal 广播的批形态（逐组 send vs 复用 AcceptedBatch 桶）。
- 日结静点（DayEnd）与 Seal 的确认复用（设计稿 §5——本批不展开，
  Q5 批做）。
