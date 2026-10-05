# 第三批 ledger sharing 独立复核

复核范围：`ledger.rs`、`ledger_state_tests.rs`、`pipeline/mod.rs`、`ledger_rows.rs` 与 `cpu-probe.py` 的完整 diff（含新增文件）。复核基于该 worktree 当前内容；本记录不代表整轮性能任务或项目已完成。

## 结论

本批未发现会改变 A 股交易语义、成交费用、资源守恒或失败原子性的有效缺陷。ledger 行共享实现和证据缓存的失效路径总体成立；unchanged rebase fastpath 的前置条件经完整证据校验后，足以证明行没有本 tick 消费/释放事实，因此保留 envelope 的累计 `EnvelopeAudit`（包括 `charged`、`nominal`）是正确的。已有 conservation validation 覆盖 basis、spent、released 与 live 的守恒关系，fastpath 没有重置费用或资源事实。

## 复核要点

- `SharedRows<T>` 通过 `DerefMut` 的 `Arc::make_mut` 暴露所有 map 写入口。共享时写入会隔离 clone；仅一个 strong owner、但 cache 持有 `Weak` 时写入会使旧 `Weak` 脱离当前 allocation。旧 allocation 在 weak 释放前仍被占用，identity 比较不会因地址复用产生 ABA。读入口只解引用，不会使缓存失效。
- `EvidenceCache` 的四个 `Weak` identity 覆盖 `validate_ledger_evidence_uncached` 所读取的四个 map。缓存未覆盖的 `seen_local_keys` 与 `next_receipt_index` 不参与该完整证据校验；其变化不会使校验结果失真。共享 cache 由 `Mutex` 串行保护，锁中毒被转换为带上下文的显式 invariant error。序列化通过 `serde(skip)` 忽略 cache，`PartialEq` 和 `hash_projection` 也只覆盖业务数据。
- rebase fastpath 先运行完整证据校验，再要求无 terminal rows、所有 live envelope 均为 `TickStart`、无 pending price、`basis == live` 且 conservation 为 `EMPTY`。因此只有没有 tick-local 资源变化的行可跳过重建；仍清除 tick-local receipt identity，同时保留 `next_receipt_index` 和 envelope 累计审计。慢路径也保留 live 行的 audit，并仅将 conservation 重置到下一 tick 的空状态。
- 对外原子入口在 clone candidate 上操作，出错时不安装 candidate；`rebase_private_for_tick_commit` 自身可能在失败前消费 private candidate 的行，但其注释与调用约束要求失败后丢弃该 candidate。现有失败原子性测试覆盖 public rebase。
- 这些改动只优化内存共享和完整证据校验，没有修改交易制度、费用率、舍入、撮合或 A 股概念；无需新增交易规则依据或变更 `trading-rules.md`。
- `cpu-probe.py` 的 `/proc/<pid>/stat` 线程采样能按被测进程统计 CPU，预算占比也按 affinity CPU 数归一化。线程在两个样本之间退出时，`observed_thread_cpu_pct` 会漏掉其最后一段 CPU 时间，但进程级 `cpu_pct` 仍通过进程累计 CPU 时间统计，不影响核心 CPU 均值；将 per-thread 合计解释为诊断下界即可。

## 第二轮复核（2026-10-06）

- 对 `44ed1f07` 独立 HEAD 源快照重编后，原来的同名 continuous transaction case 仍因缺少 `pending_npc` queue 以 exit 101 失败；原始失败日志由主任务保留。当前 fixture 只在 preview 上调用 `queue_npc_for_next_tick`，满足 `ReadyIngress::capture_sources` 的入口前置条件，且保留所有业务断言。实际 tick 仍走现有 `plan_tick` 路径；这项只修测试构造，未改变生产逻辑。
- `next_tick_rebase_preserves_cumulative_fee_audit_and_receipt_cursor` 现在在一次正常 rebase 后再次运行 private fastpath，并断言 live row 指针共享、完整 `EnvelopeAudit` 相等、receipt cursor 保持 42。原 fixture 已确认首笔产生非零名义与实收差额；保留完整 audit 相等检查可验证费用累计信息（含后续追收所需欠费）未被 fastpath 重置。该补充覆盖了上一轮建议的关键 fee 边界。
- `unique_owner_mutation_invalidates_weak_identity_without_cloning_value` 直接覆盖 cache Weak 为唯一旁路 owner 时的 mutation；serialization 测试覆盖 SharedRows 输出不带包装层。主任务报告两项短测通过。
- 第二轮完整最新 diff 未发现阻断项。`git diff --check` 无输出；本轮只读审阅，没有自行运行测试。主任务正在重跑 continuous、ledger 等短测，最终测试状态以其结果为准。

## 尚可补充的非阻断边界测试

1. 连续执行 `validate_complete_evidence` 命中成功缓存后，clone 出 candidate 并分别通过 `get_mut`、`entry` 类写入口改动四种 map；逐一验证旧 ledger 仍命中原证据、改坏的 candidate 重新完整校验并失败。当前 corruption 测试覆盖了 map 修改，但没有分别锁定四种 public `DerefMut` 访问形态。
2. 将曾经通过缓存校验的 map 改为另一份仍合法但内容不同的证据，再校验成功；之后继续改变该 map 验证下一次不会误用上一次成功缓存。该用例明确缓存按身份而非按“曾成功过”判定。
3. 在成功缓存存在时测试锁中毒路径能显式返回错误（若测试可在不增加生产测试钩子的前提下访问锁），确认没有静默跳过验证。

其余测试结果和全轮性能门槛仍由主任务单独报告；本复核不表示总体任务或 1000x 目标已完成。
