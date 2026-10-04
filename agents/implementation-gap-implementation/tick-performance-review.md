# G16 / G17 / G57 独立复核

## 范围与依据

复核者未实施生产代码。工作树：`.worktree/implementation-audit-final`，以当前 HEAD 完整相关 diff 为对象；包含 `plans/mod.rs`、新增 `plans/records.rs`、`indicators.rs`、`diagnostics/causal*`、`diagnostics/decision_trace.rs`、对应测试，以及共享 `session.rs`、`hash_contract_tests.rs`、`decision_chain/roots.rs` 的 G16 Arc 改动。已读工程原则、架构、开放问题、ADR-0008 和主账 G16/G17/G57；不将 proposed ADR-0018 的整套长期架构当作 G16 必须实施范围。

本批未改变 A 股报价、申报、撮合、T+1、税费、披露时点或数量单位，因此不引入新的交易制度依据；现行领域规则沿用项目已登记依据。诊断 qty/depth 仍是股，decimal string 只是无损传输表示。

## G16

- `closing/library` 在 authority 与 tick shadow 间使用 Arc，实际写入口使用 `Arc::make_mut`；保存仍输出原对象，恢复重建 Arc。`RootReadContext` 借用共享 library，plans 的 clone 改为持久化 root clone。
- `PlanRecords` 为固定 8 层大端字节 radix tree；每层最多 256 子节点。改一个 plan 只复制该路径，终止历史仍共享；活跃索引的复制成本仍由活跃工作量决定，不随终止计划积累增长。按字节升序遍历保持原 PlanId 数值升序，不改正式委托优先级。
- 这一结构比单个 Arc 包裹 BTreeMap 更必要：后者一旦修改活跃计划就仍会深复制全部终止历史。本实现没有引入 WAL、页版本根、冷历史或新依赖。
- 初审发现序列化仍收集深克隆的完整 map，作者已改为借用 `PlanBookSaveRef` + `SerializeMap`，并加入旧 BTreeMap DTO JSON 字节一致断言。纠正初审表述：`candidate_commit.rs` 的 business hash 调用有 `cfg(test)`，不能误称生产普通 tick 每次运行；借用序列化仍消除显式 hash/save 路径的额外深复制。
- 静态审查未发现跨 shadow 可变所有权泄漏。现有及新增测试覆盖终止历史共享、被写计划隔离、ClosingEngine 隔离、序列化恢复。复核者亲自运行最终当前树 binary：PlanBook 新增 case 1 passed（0.03 秒）、Arc shadow 新增 case 1 passed（0.19 秒）、完整 hash_contract_tests 14 passed（0.68 秒），均为 8 test threads + 外部 10 秒 deadline。G16 原局部所有权目标通过，不证明长期 tick 吞吐或所有历史访问已变成常数成本。

## G17

- WASM、Server、Desktop 的真实生产入口均调用同一 `calculate_indicators`。2048 以下保留顺序计算，2048 及以上对三个独立算法执行 Rayon join；不是只在未消费的 batch API 上添加并行。
- 每个 EMA/KDJ 的内部浮点递推顺序不变，不改变参数、验证、输出序列顺序，不读取 RNG。新增测试对 2048 输入逐字段精确比较顺序组件结果。
- 作者测量为 8 worker、64 次循环：64 samples 顺序 244μs / 并行 757μs；2048 为 8636μs / 4436μs；8192 为 46108μs / 18199μs。支持保留小输入顺序路径，不证明所有硬件或浏览器固定提速。
- 复核者对作者已编译的真实 indicators 源文件测试 binary 运行 10 个短测，`--test-threads=8`、外部 10 秒 timeout，全部通过（0.01 秒）；最终当前树 engine binary 对生产入口精确数值 case 也实际运行 1 passed（0.01 秒）。首次 `--exact` 未包含模块前缀匹配到 0 tests，已纠正并重跑，不计为通过证据。
- WASM 已有 wasm-bindgen-rayon/initThreadPool/shared-memory 机制，新增 join 没有新增平台依赖；作者更正环境检查：默认 stable 未安装 wasm32，但项目固定 nightly-2026-09-05 已安装 wasm32 与 rust-src。作者实际运行 `cargo +nightly-2026-09-05 check -p web-wasm --target wasm32-unknown-unknown --target-dir .tmp/gap-target -j16 --offline` 成功（1 分钟，外部 timeout 300 秒、共享 flock）；仅既有 atomics unstable 警告。G17 生产接线、数值及 target 编译通过；未跑浏览器实际多线程性能，不宣称三宿主固定提速或完整长验收。

## G57

- causal report、fact、quote、OrderLifecycle、ImpactSample、RecoverySample 的 u64、Option<u64>、information_delays identity 均用 decimal string；None 保持 null，delay i64 与浮点比率保持原单位/类型。
- CLI `price_volume_baseline` 直接包装 DTO，无额外字符串转换或数值计算，因此 serde 边界修复会进入真实 CLI 输出。未发现 web 对 causal qty 做 number 运算的消费入口。
- 两个当前树新增 causal JSON case 经复核者亲自运行，8 test threads + 外部 10 秒 deadline，2 passed（0.20 秒），包含合法 `u64::MAX` seed、qty、sequence、depth、publication、Some/None 和真实 CLI 包装。最初作者误在已有 impacts 后追加测试 sample 导致期望下标错误，已改为明确合成 vectors，不弱化大值断言。
- 初审发现另一实际公开诊断 `NpcDecisionTraceRecord.tick` 尚为 JSON number，作者已改 producer decimal string；web `npc-decision-trace.ts` 原先要求 number，strict keys 未包含新 `plan_changes`，会拒绝正常 trace 响应。该发现已修复并独立复核：类型与 parser 使用 canonical u64 decimal string、BigInt 上界判断且不转 Number，`plan_changes` 贯穿字段白名单、校验与返回对象。新测试覆盖 0、超 JS 安全整数、u64.MAX、非规范文本、负数、数字输入、上界溢出及 plan_changes 类型错误；作者 5 个 Node 短测通过（case/command 10000ms、concurrency 8）。G57 跨层兼容阻断解除。

## 当前门禁结论

G16 原局部所有权目标静态语义/必要性及最终短测通过；G17 生产接线、原生数值短测和 WASM target 编译通过；G57 causal 静态序列化及大值短测、trace tick 跨层契约修复复核通过。没有运行完整回归、矩阵或长验收，没有 stage/commit。

验证绑定补记：共享 target 中一份 `engine-6ae5d5d49d140673` binary 只含旧版 13 个 hash tests，两项本批新增 G16 测试匹配为 0；13 个旧 hash tests虽通过，不能作为本批最终源码的 G16 通过证据。已通知整合者与协调者重编当前树并确认实际测试名称，避免 isolated build 覆盖同路径 binary 造成误报。

重编后复核：同路径 binary 更新时间为 2026-10-04 15:42:52，包含 1102 tests、14 hash_contract_tests，并实际运行上述两项新增测试。旧 binary 绑定问题现已消除。

## 分项提交边界复核

- `session/decision_chain/roots.rs` 只有首个 library 类型改为 Arc 的 hunk 属于 G16；其余 existing plan、watchlist、interim report 和 memory prune 改动归 strategy，不能随 G16 整文件纳入。
- `session/hash_contract_tests.rs` 当前完整 diff 均为 G16 的 Arc 影子测试与适配。
- `session/hash.rs` 当前两个 groups hunk 为 assembly 权威字段覆盖，不属于 G16；本报告未独立批准 assembly 领域变更。
- `session.rs` 与 `persistence.rs` 共享 company/retail/assembly 改动，G16 审查只覆盖 closing/library Arc 相关事实，不构成整文件放行。
