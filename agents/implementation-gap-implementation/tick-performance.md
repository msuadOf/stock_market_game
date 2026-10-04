# G16 / G17 / G57 局部实施记录

## 边界与依据

- 在独立工作树 `implementation-audit-final` 实施；未 stage、commit 或改审计总账。
- 已阅读根 `AGENTS.md`、`docs/principles.md`、架构与开放问题，以及 G16/G17/G57 原条目、`reaudit-foundations.md`、`exhaustive-review/luna10.md`、`exhaustive-review/luna13.md` 的现行证据。
- 以 accepted ADR-0008 的 Rust / Rayon 指标目标、ADR-0017 的 shadow / 单点提交契约及已接受的普通 tick 历史所有权目标为范围；不实施 proposed ADR-0018 的完整版本根、WAL、冷热历史、观察令牌或反向唤醒索引。
- 本批不新增或改变 A 股交易制度，不改变价格时间优先、T+1、委托数量、现金与股份单位；指标仍是展示计算，诊断仍是只读投影。未联网查询官方规则，不以本批性能证据宣称制度完整实现。

## 实现

- G16：`PlanBook` 私有 `PlanRecords` 使用按 `PlanId` 大端字节分支的 8 层 Arc radix。clone 仅共享根；单计划 mutation 对最多 8 个固定 fanout（至多 256）的分支及一个叶节点执行 COW，不复制未触及的终止计划。保留全部历史、PlanId 升序、active 索引、公开 API 与原存档 map 形状。序列化借用记录，以 `SerializeMap` 保持原 JSON 字节，不为导出或显式 hash 再复制全部计划。
- G16：`CommittableSessionState.closing/library` 改 Arc，由 `integrate_session_contracts` 接入 construction、真实 mutation、save projection 与 restore；`RootReadContext.library` 由 `implement_personal_strategy` 同步共享。真实自然日 mutation 仍可能复制对应历史 owner，不声称完整历史页级 COW。
- G17：不把单元素 batch 冒充并行。三宿主既有单项入口共同进入 `calculate_indicators`；当 `max(prices.len(), candles.len()) >= 2048` 时以 Rayon join 并行 MACD、price KDJ、candle KDJ，短输入保持顺序。没有新增宿主协议、额外复制输入或改变数值递推。
- G57：causal report / lifecycle / fact / time / depth / impact / recovery 的潜在大 u64 输出改为十进制字符串，Option 无值仍为 null，`information_delays` 仅其 u64 identity 改字符串。保留 u32 数量及其他既有类型，未扩大 Money / 正式存档数值范围。`decision_trace.tick` 的同类遗漏已交其 owner 补齐。
- 接线补充：`diagnostics.rs` 的 `PersonalAnalysis` reason 名称及两处 diagnostics fixture 的新 setup 字段；`session/hash.rs` 的新增 groups 显式 exhaustive projection 由公共 owner 请求补接，不隐含忽略权威 groups。

## 测试与性能事实

- 先写 `shadow_clone_shares_terminal_history_and_only_copies_touched_plan`（300 terminal + 1 live），检查真实对象指针共享、仅 live mutation 复制、历史保留、roundtrip 与 legacy BTreeMap JSON 字节一致。
- 先写 causal JSON 大整数契约测试，覆盖 `u64::MAX`、0、Option / null、CLI 等价 `{causal_runs: [...]}` wrapper、嵌套生命周期及 fact 深度。
- 最初 engine 测试编译被其他并行实施中的 setup 字段、trait import 与 module 接线阻塞；这是编译阻塞，不是预期行为断言失败。没有把它冒充行为红测试。新增测试先于实现，但 G16 没有获得完整的实施前 engine 行为红测试时序证据。
- G57 用 HEAD 原始 `causal/report.rs` 的独立 DTO harness（未使用的嵌套类型以别名替代）复现 seed 契约失败：`Number(18446744073709551615)` 不等于 `"18446744073709551615"`，0.00 秒。它是原始 DTO 的隔离复现，不冒充完整 baseline engine 运行。
- 最终 native 重新从当前工作树编译，显式核对 binary `--list` 含新增 case，不复用另一 agent 曾覆盖过的隔离 baseline binary。`PlanBook` 共享 / mutation / roundtrip / legacy 字节兼容 1 项通过（0.03 秒）；真实普通 tick Arc shadow 共享 / mutation 隔离 1 项通过（0.19 秒）；生产指标逐值一致 1 项通过（0.01 秒）。
- 最终 causal binary `causal_diagnostics-476880b73b651d71` 当前树编译通过；JSON 边界 2 项通过（0.19 秒），真实连续竞价委托 / 撤单对账 1 项通过（0.19 秒）。首次 JSON 测试把合成 sample 追加在已有 impact 后，却断言 index 0 为大值；修正 fixture 为显式合成 vector 后绿，不删除或弱化边界断言。
- G17 是保持结果的性能重构：新增真实生产入口与顺序组件逐值一致短测，在改实现前通过。用真实 `indicators.rs` 的独立 harness 解除整 crate 暂时编译阻塞，不替代最终 engine / 宿主门禁。
- 测量参数：8 Rayon worker、64 次循环、优化等级 2、16 codegen units；fixture 各有同长度 prices / candles。第一轮 64 samples：顺序 244 μs、并行 835 μs；2048：8670 / 4505 μs；8192：46089 / 20955 μs。第二轮为 244 / 757、8636 / 4436、46108 / 18199 μs。只支持当前机器这三种短 fixture，不能泛化为三宿主性能验收。
- 独立 indicators harness 的全部 11 项短测试通过，0.08 秒。每次 binary 命令均 `timeout 10s`、`RAYON_NUM_THREADS=8`、`--test-threads=8`；Rust 编译统一使用 `flock /tmp/stock-market-gap-cargo.lock`，Cargo 使用 `.tmp/gap-target -j16`。
- 默认 toolchain 仅安装 x86_64；进一步检查确认项目固定 `nightly-2026-09-05` 已安装 WASM target 与 rust-src。WASM check 使用该固定 toolchain、`web-wasm`、项目 atomics / build-std 配置、offline / -j16 / 同一编译锁与外层 300000ms deadline，已通过，实际编译 1m00s；仅有项目既有 atomics unstable feature 警告。未运行完整回归、长矩阵或多年吞吐验收。
- 最新独立追加的 non-feature lib 编译再次遇到 `accounting/reports/balance_sheet.rs:370` 的 `ConsolidationFacts` 新字段 fixture 遗漏，已通知其 owner 与 root。它不是本批 PlanBook / Arc / 指标 case 的失败；最终完整树仍须修复该编译阻塞并重编，不能以旧 binary 的局部 green 声称整个最新树已绿。

## 独立复核与未闭环

- `review_tick_performance` 已只读复核完整关联 diff，记录见 [tick-performance-review.md](tick-performance-review.md)。有效发现：PlanBook 原 Serialize 的全历史 clone 已改为借用；decision_trace tick 数值遗漏已交 owner 补齐。
- reviewer 已亲自核对 G16 新增 case 以及 hash 套件（14 项通过，0.68 秒），G16 局部 ownership 目标放行；G17 的真实入口 / 短 fixture 收益 / 精确数值 / nightly WASM check 放行；G57 causal 子项放行。`all diagnostic` 范围仍等待 decision_trace producer / Web consumer 的字符串契约闭环，由其 owner 实施并再次复核。总账核销只能由 root 在这些事实基础上决定，不从 Arc / Rayon 出现推断所有验收完成。
