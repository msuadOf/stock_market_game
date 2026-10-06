# E7-X2 旧状态延后析构实验（已撤回）

## 假设与依据

tick-overlap 探索诊断（.tmp/perf-expl/tick-overlap/）测得 `commit_tick_shadow`/`commit_from` 的"不可失败交换"367µs/tick（×3000=1.1s），主体是旧权威状态（20k 账户/簿页/字符串）在逐字段赋值时就地析构；旧值析构无副作用、与新状态无别名，可交 `rayon::spawn(drop)` 移出关键路径，外推 ~+9%。

## 实现（已按 HEAD 原字节撤回）

- `commit_from` 改为 `std::mem::replace(self, shadow)` 整体换出 + `rayon::spawn(move || drop(old))`（cfg(test) 计数器证明析构确实发生在池线程）。语义等价：原实现所有字段均取 shadow 值，整体 replace 等价；无副作用 Drop（全库仅 executor_perturbation 的 ResetScope 有 Drop impl，不在提交状态内）；页 Arc 仅减引用计数，Weak 身份失效语义不变。
- 删除两个失去调用方的死方法 `replace_and_drop_parallel`（account_paged_map.rs / account_book.rs；原实现是阻塞式 `into_par_iter().for_each(drop)`，仅覆盖 4 个字段）。
- TDD：红灯（计数恒 0 超时断言）→ 绿灯；定向 committable_state 3 / hash_contract 14 / candidate_commit 5 / shadow 43 全绿。

## 基准（锚定交错配对，0-15 核占用 8.0% <10% 门槛，threads=4 生产口径）

anchor 漂移 1.94% ≤5% 有效。6 对配对：+2.83/+5.38/−0.37/+0.99/+2.52/+4.91%，mean +2.71%、sd 2.21%，**pair3 为负、区间重叠**——同号一致与区间分离不满足，按预登记规则**判定不通过，撤回**。

数据文件：e7x2-*.json/err（agents/engine-max-performance/）；产物 .tmp/e7-x2/（candidate 源码、双臂 binary、构建 jsonl）。

## 机制结论

外推 +9% 只兑现 +2.71% 均值且不稳定：**"移出 tick 线程" ≠ "移出关键路径"**——worker=4 的小池里，spawn 的析构任务与下一 tick 的并行突发共用同一 FIFO/窃取队列，析构挤占了并行车道（与 P0-dirtykey 证伪的"省的活在并行车道不挡路"互为镜像：本实验证明搬进并行车道的活也会挡路）。若后续重试，应改用**专用析构线程**（std::thread + channel，不进 rayon 池），尤其在与 X1 跨 tick 投机预计算组合时（投机 prep 同样占用 commit 尾部窗口的池容量）。

## 遗留

- probe 主 target 已重建绑定回 HEAD（SHA 34084792…，verify-build 通过）。
- 顺带登记：HEAD 既有 `packages/engine/tests/company_scenarios` 编译失败（WithinKindDistribution 未定义，3 处），与本实验无关，未修复。
