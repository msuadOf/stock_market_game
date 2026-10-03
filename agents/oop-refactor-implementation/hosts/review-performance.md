# hosts/performance 独立复核

日期：2026-10-03。复核者：`/root/implement_hosts/review_performance`，未参与实现。最终结论：通过。初审的 2 项 P2 ownership 问题及 1 项 P3 facade 错误契约变化均已由实施者修复，并经独立复审关闭；没有未修复的有效发现。

## 范围与依据

- 基线：`b89afb3346743a4b4fccf26c9ac9ff108595f696`。
- 完整审查六个文件的相对基线 diff 及当前全文：`scripts/simulation/escrow-performance-harness.mjs`、`.test.mjs`；`scripts/simulation/escrow-verification-contracts.mjs`、`.test.mjs`；`scripts/performance/market-ui-report.mjs`、`.test.mjs`。
- 已读 `AGENTS.md`、`docs/principles.md`、`docs/architecture.md`、`docs/open-questions.md`、`docs/trading-rules.md`、ADR-0017 与本组 `performance/actions.json`、`performance/status.md`。
- 本批为工具 owner 提取，没有修改交易制度参数或 engine 交易算法。领域判断依据既有交易规则基线及 ADR-0017 现行 P0/P1、Buy/Sell、收据身份修订；没有将既有官方规则核对记录冒充本日重新访问核验。

## 初审有效发现

### P2-01：公开 appendSample 绕过测量与源码验证，仍可生成可复用 PASS

位置：`scripts/simulation/escrow-performance-harness.mjs:507`，关联 `:521`、`:553`、`:555`。

`PerformanceComparisonRun.appendSample` 对任意传入对象直接 push。`buildReport` 只检查每侧长度、吞吐与 RSS 聚合数值，不保证样本来自本 run 的 `measureSide`，也不保证已完成 warmup、交替采样或源码前后检查。新导出的 owner 因而允许从未调用 `runSample`/`sourceManifest` 的 run 报告 `status: PASS`，同时声明 `source_manifest_verified_before_and_after_every_invocation: true`。

精确短 fixture：合法 config 的 `sample_count=1`、`warmup_runs=0`，向两侧分别调用 `appendSample(side, { ticks_per_second: 1, peak_process_tree_rss_bytes: 1 })`。实测 `buildReport({ fixture: true }, timestamp)` 返回 PASS；`matchesReusableReport` 返回 true；返回的 after sample 只有上述两个字段。该输出不满足已有 `validateMeasuredSample` 契约，却被 owner 宣称为可复用性能证据。

修复方向：将追加能力限制在本实例正常 `measure` 流程内部，或验证本实例、side 与测量来源；不要新增公共旁路。补未测量 run 不能构造 PASS 的代表性边界。

### P2-02：buildReport 返回私有样本的可变别名

位置：`scripts/simulation/escrow-performance-harness.mjs:555`、`:556`。

返回报告的 `before.samples`/`after.samples` 就是私有 `#samples` 数组；样本对象也共享引用。消费者改报告会反向改变 owner 保存的证据，第二次报告可能产生未经测量的新比值，与新增的单次 run ownership 承诺不符。

精确短 fixture：接续 P2-01 得到的报告，设置 `report.after.samples[0].ticks_per_second = 100`，再次 `buildReport` 得到 after/before 比值 100。无需重新测量即可改变 run 的证据。同样可以通过 pop/push 修改私有数组长度。

修复方向：报告应为独立 snapshot，样本接收边界也避免保留外部可变别名。补消费者修改报告后，本 run 再次生成的报告不受影响。

### P3-01：runProcessSample facade 不再总是返回 Promise

位置：`scripts/simulation/escrow-performance-harness.mjs:418`，关联 `:341`。

基线为 `export async function runProcessSample(endpoint, { rssSampleIntervalMs })`，缺失或 null options 时得到 rejected Promise。提取后 facade 改为普通 function，constructor 参数解构在 `.start()` 前同步抛错。实测缺失 options 是同步 TypeError；调用者使用 `runProcessSample(...).catch(...)` 或 `assert.rejects(runProcessSample(...))` 的原有处理形态失效。

修复方向：保留 async facade，把构造阶段异常仍转换为 Promise rejection；不要用默认 options 静默改变原有拒绝行为。补缺失/null options 的失败契约。

## 三项门禁判断

1. **大 A 语义与依据：初审通过。** `Resource.fromJson` 仍调用无上界 canonical 非负 decimal→BigInt codec，没有混入 signedDecimal/unsignedDecimal 的 i64/u64 范围；单位仍为 cash 分、shares 股。Buy shares 与 Sell cash 零腿、positive Fill、P0-only/无 receipt 的 P1 截点、PreSeal→SealedBatch 单向 journal 转换、局部 live chain、账户逐资源聚合及 T+1 校验与基线保持一致。全局 receipt index 和 source-local ordinal 检查继续在顶层；未新增跨 envelope 交易顺序。
2. **需求必要性与最小范围：修复 ownership 后通过。** 四个 action 均落在所授权工具文件内，未增依赖、交易 policy、指标 schema 或通用框架。ProcessSampleRun 持有 child/采样器/timer，MarketUiReportRun 持有 Vite/browser/profile/CDP，EnvelopeConservation 持有单 envelope 转移状态，均为真实 owner；PerformanceComparisonRun 的配置与两侧运行身份归属合理，但初审发现的公开样本旁路及可变别名需收口，才能完整满足 owner 不变量。
3. **边界、跨层漂移与复杂度：初审有阻塞项。** P2-01/P2-02 的边界未被原新增 lifecycle 测试覆盖，P3-01 未被原新增 spawn fixture 覆盖。其它新增测试保留原有断言，覆盖配置 snapshot、不同 run 隔离、失败不构造 PASS、既有 reuse 比较字段、BigInt 大数、P1 边界、UI 正常/部分初始化/连接失败/四步清理截断等代表性边界。未见 product 跨层语义漂移。

## 明确保留的旧边界

- sampler rejection 仍先等待 child close 再 await sampler，没有保证及时终止存活 child；这是 actions 明确排除的另批行为问题，不把本批对象提取宣称为修复。
- UI 清理仍是 `client.close → stopProcess(browser) → stopProcess(server) → rm(profile)`；任一步抛错会截断后续，未新增幂等、错误聚合、重排或整树等待。Linux `stopProcess` 仍只发 SIGTERM，未扩大等待承诺。
- 复用报告校验与基线比较字段保持相同；本批没有把旧 reuse 校验扩展为逐样本完整证据重验。

## 验证边界

复核期间未运行真实性能、浏览器、Cargo、E2E 或完整回归。上述复现仅为不启动 child、不访问网络的内存短 fixture，用 `scripts/run-with-deadline.mjs 10000` 限制整条命令，实际约 0.4 秒。35 个 Node 短测试 PASS 为实施者 `performance/status.md` 提供的证据，初审没有独立重跑或将其当作真实性能验收。

## 修复后独立复审

实施者通知六个所属文件再次冻结后，重新检查相对同一基线的完整更新 diff。其它四个文件的领域与 UI 清理改动与初审一致；修复仅收口 performance run 的证据所有权和恢复 facade 契约。

| 发现 | 修复位置 | 复审结论 |
| --- | --- | --- |
| P2-01 | `escrow-performance-harness.mjs:492`、`:507`、`:517` | `#measureSide`、`#appendSample` 私有化；唯一追加路径来自 `measure` 内源码前后核对和 sample validator 成功的结果，sample 另作 `structuredClone`。无公共外部追加旁路，未测量 run 构造报告明确失败。关闭。 |
| P2-02 | `escrow-performance-harness.mjs:533` | `buildReport` 对完整报告执行 `structuredClone`，样本、配置与输入 environment 均不外泄可变别名。嵌套 sample histogram、样本数组、workload、command、environment 的消费者变动不改变后续报告。关闭。 |
| P3-01 | `escrow-performance-harness.mjs:418` | facade 恢复 `export async function`，缺失及 null options 保持 rejected Promise，未加默认值掩盖异常。关闭。 |

独立执行以下代表性短测试，case timeout 与外部进程树 deadline 均为 10000ms，显式 `--test-concurrency=3`：

```sh
TMPDIR="$PWD/.tmp/process-tmp" node scripts/run-with-deadline.mjs 10000 -- node --test --test-isolation=none --test-reporter=tap --test-timeout=10000 --test-concurrency=3 --test-name-pattern='PerformanceComparisonRun|异步 facade|records actual paired|parses a config file once' scripts/simulation/escrow-performance-harness.test.mjs
```

结果：7/7 PASS，0 failure，runner duration 78.672502ms。涵盖三项发现回归、配置 snapshot 与 run 隔离、失败报告、配对顺序、报告复用；未运行真实 benchmark。独立 `git diff --check` 对六个文件通过。

三项门禁最终均通过：A 股及 escrow 证据语义未漂移，四项授权 action 具有真实 owner 且范围最小，新增 ownership 及错误边界已有有效回归覆盖。保留旧 sampler failure、UI 清理截断与旧 reuse 验证边界，不将这次工具对象提取声称为那些行为的修复或完整产品验收。
