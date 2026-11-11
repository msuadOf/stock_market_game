# 主分支日终崩溃对照

- 日期：2026-11-09；基线：`4efb3c15`；内置浏览器独立 origin `127.0.0.1:4184`。
- 默认配置：5 只股票、20,000 retail / 5 institutional / 2 hot NPC；固定 seed 42；MAX 速度；新局。
- WASM SHA-256：`d48cca41d8f6674d271c40d3d4eaf7924d6370944eb4b18fa41c971b6577cdbe`。对照目录只修改前端 seed 和临时错误栈输出，没有修改 Rust 引擎。
- 第一次同配置（4183）首日日终崩溃，错误 `WASM_WORKER_PROTOCOL @ wasm-worker.step: unreachable`；第二次（4184）同配置同 WASM 字节，首日日终再次崩溃。
- 第二次临时输出的调用栈依次包含 `rust_oom` → `BTreeMap<StockId, HoldingEpoch>::clone` → `BeliefParticipantSnapshot::clone` → `GameSession::prepare_administrative_mirror` → 日终行政事务。由此可确定此复现为准备日终行政镜像时内存分配失败。
- 用户原截图只有 `FACADE_BOOTSTRAP_FAILURE ... unreachable`，没有调用栈；不能据此断言截图那次与本复现一定相同。
- 尚未逐组对照所有候选修改，也尚未证明这一个分配点是唯一瓶颈。

## 行政镜像单组对照

- 在同一基线和前端诊断改动之上，仅加入 `administrative_mirror.rs`、`account_paged_map.rs`、`runtime_mirror.rs` 及对应测试的镜像复制优化；默认配置和 seed 42 不变。WASM SHA-256 `c7bbbe5781f3a6a5fbda4b3d470f4b978b36a6400363fc4c99eefff663a14fbf`；独立 origin `127.0.0.1:4185`。
- 越过基线中 `prepare_administrative_mirror` 的分配点，但首日日终仍崩溃。临时堆栈为 `rust_oom` → `BTreeMap<StockId, StockHistoryRead>::clone` → `AccountPagedMap<PersonalHistoryReadLedger>::to_map` → `GameSession::save_committed_projection` → 日终行政事务。
- 此对照证明单独使用该组不足以完成首日日终，且失败点转移到生成存档的账户历史复制；不证明这两处是所有可能瓶颈。

## 严格行政镜像对照

- 移除上一组误带的 `AccountPagedMap::to_map` 优化，仅保留行政镜像及其 `get_arc` 辅助；同一主分支、默认配置、seed 42、MAX、新 origin `127.0.0.1:4186`。WASM SHA-256 `3255352d2a1e4ce4f442debe2ece0362aefaf828ed3336f5608e969bd8251f02`。
- 第一日日终成功，进入第二日。第二日日终崩溃，堆栈为 `rust_oom` → `BTreeMap<StockId, BeliefEntry>::clone` → `BeliefBook::clone` → 收集 `(AccountId, BeliefBook)` 的 `Vec::from_iter` → `BTreeMap::from_iter` → `GameSession::save_projection`。
- 这定位了本次失败时的具体分配链；不能仅凭此断言其他状态不是内存峰值贡献者。下一实验只改变该 `belief_books` 表的收集方式，保留完整字段及数据隔离。

## 仅调整 belief_books 收集方式

- 在严格行政镜像对照之上，仅将 `belief_books` 的 `collect()` 改为逐项插入，不加入 `AccountPagedMap::to_map` 改动。其余配置不变；独立 origin `127.0.0.1:4187`，WASM SHA-256 `984a0c279b17c118f560139920426dfa6dfd728379ec0c75553a0e0f00b8044c`。
- 仍在日终触发 `rust_oom`，栈落在 `PersonalHistoryReadLedger` 的股票历史数据复制及 `AccountPagedMap::to_map` 的 `Vec::from_iter`，后续为 `save_committed_projection`。没有在崩溃前记录到确切交易日，故不推断失败发生在第几日。
- 这说明仅调整 `belief_books` 收集不足；分配器布局和其余已持有数据影响失败点，不能仅根据最后一帧确定最大占用者。

## 历史读取 COW 对照

- 在严格行政镜像和逐项插入 belief_books 的基础上，让 PersonalHistoryReadLedger 的股票表在克隆时共享、写入时复制。其余配置不变，独立 origin `127.0.0.1:4188`，WASM SHA-256 `cd20d20df88ad4c175d55bea8b9b9eb6640ff7930b4b59c2e087e48f90a08466`。
- 内置浏览器明确观察到进入第 2 日、随后到达第 2 日 14:34；第二日日终崩溃。栈为 `rust_oom` → `BTreeMap<StockId, BeliefEntry>::clone` → `BeliefBook::clone` → `GameSession::save_committed_projection`。
- 该优化不足以避免第二日日终崩溃；最后一次分配的位置不等同于最大内存占用的来源。history_reads 的 COW 测试 3 项通过，独立静态复核未发现领域语义回归，但 Rust 公共字段类型发生源码兼容性变化，尚未决定是否保留。

## 信念条目 COW 对照

- 在 4188 的改动之上，仅让 BeliefBook.entries 在克隆时共享、写入时复制。独立 origin `127.0.0.1:4189`，默认配置、seed 42、MAX；WASM SHA-256 `151f8097f1baabb44b39c484b88aec50c31567fecf1161ae32630a039e17ee53`。
- 明确观察到第 2 日并成功进入第 3 日，暂停于 09:15:18。本次对照跨越了两个日终；不代表其他 seed 或重新加载存档已通过。
- 新增真实信念更新后的快照隔离测试通过；独立复核指出初始空 map 测试不充分，修正后再次复核通过。该实验仍包含前述行政镜像、belief_books 收集方式和 history_reads COW，尚不能证明每一项均有必要。

## 正常随机 seed 的正式构建

- 移除前端固定 seed 和诊断堆栈输出。正式构建首次达到共享 300000ms deadline 而失败；关闭已完成的诊断标签后重试，`--jobs 10` 构建成功。
- 使用独立 origin `127.0.0.1:4190`，默认配置、正常随机 seed、MAX，内置浏览器观察到跨过首日和第二日日终并进入第 3 日；暂停于第 3 日 09:47:23，速度改回 1x。
- 8 项行政镜像测试、history_reads 的 3 项测试、信念簿生产更新隔离测试、39 项 strategy 测试以及生成类型检查通过。完整 diff 由独立 subagent 复核，未发现必须修复的正确性问题。
- 以上不是重开第二日日终存档的成功验证；此前同类大存档的恢复曾被 JS 内存准入拒绝。

## 超过两个交易日的连续运行验证

- 日期：2026-11-09；沿用上述正常随机 seed 的正式构建，独立 origin `127.0.0.1:4191`，默认配置、MAX、新局；没有操作用户的 4190 试玩局。
- 内置浏览器依次观察到第 2 日 09:15、第 3 日 09:15、第 3 日 14:35，随后进入第 4 日 09:15，验证连续跨过三个日终。测试局暂停于第 4 日 09:49:41。
- 此结果只证明本次随机 seed 和默认配置下连续运行超过两个交易日；不证明所有随机 seed、用户操作或大存档重新加载均无问题。
