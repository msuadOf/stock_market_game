# D4 设计稿：runtime 相位计时（10 相 schema 映射）——只设计不实现

> D1 尾部产出（按指示）。现状：phase_timing ×7 红（NoCommittedTick——
> Collector.tick_before/after 与相位记录由旧管线提交路径填充）。

## 机制事实
- 捕获面：thread-local COLLECTOR（capture_one_committed_tick 包 step()）；
  旧管线经 with_phase_timing_binding 把 collector 传进池任务，10 相
  （ExpiryShadow…CommitTick）各记 span。
- 测试契约：records().len()==10、tick_before/after 恰 +1、失败步零证据。

## 映射提案（runtime 段 → 10 相，语义诚实优先）
| 旧相 | runtime 段 | 备注 |
|---|---|---|
| ExpiryShadow | driver.step 内 world.run_tick 的 expire 窗口 | 部内线程——需绑定传播 |
| SealAllocationSnapshot | run_tick 广播前快照安装 | 协调者线程 ✓ |
| DecisionAndCoordinatorWork | 组决策+收束 | 组线程——需传播 |
| AccountValidation | 组端 purse/持仓校验（run_round 扣除段） | 组线程 |
| StockProcessing | 部撮合 | 部线程 |
| ReceiptAggregation | desk receipts/回执汇总 | 协调者收束点 ✓ |
| SettlementShadow | driver.step 账户回写 | 驱动器线程 ✓ |
| DerivationAudit | candidate 校验（validator） | 协调者 ✓ |
| PreCommitValidation | 锁步不变量检查 | 驱动器 ✓ |
| CommitTick | set_runtime_tick+seq 写回 | 驱动器 ✓ |

## 绑定传播（关键工程件）
协调者广播 Start/Ops 时附 `PhaseTimingBinding`（Arc<Mutex<Collector>> 克隆）；
组/部 worker 在对应段 enter(phase)/close。完成信号在绑定释放后发出
（沿用旧协议保证——spawn 两段式）。Driver 线程直接 enter 驱动器侧四相。

## 步骤
1. 驱动器侧四相先做（无传播）——tick_before/after+4 相记录 →
   phase_timing 部分测试转绿（records.len()==10 仍红）。
2. 绑定传播进组/部（协议扩展）→ 10 相齐。
3. 失败步零证据：现有 post_shadow 等价缝 discard ✓ 已天然满足。

## 风险
- 组/部线程常驻：绑定必须按 tick 传递且不跨 tick 泄漏（Abort 路径清绑）。
- 10 相名是旧管线语义标签——runtime 复用同 schema（SCHEMA 字符串
  不变）需在 trading-rules/验证证据文档登记映射依据。
