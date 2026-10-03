# hosts/performance 实施状态

日期：2026-10-03。状态：4/4 已实施并通过精准短测试与独立复审；P2-01/P2-02/P3-01 全部关闭，无未修复有效发现。

已读 AGENTS.md、docs/principles.md、docs/testing.md、docs/architecture.md、docs/open-questions.md、ADR-0017 与 actions.json；实施前完整读取所属源文件与既有测试。没有新增依赖，没有 Git 写操作。

| action | 状态 | 文件 | 对象与方法 | 真实 caller | 测试与未完成 |
| --- | --- | --- | --- | --- | --- |
| hosts-N07 | 已实施，独立复审通过 | scripts/simulation/escrow-performance-harness.mjs 及 .test.mjs | 私有 ProcessSampleRun 持有 child、stdout/stderr、spawnError、起止时间、RSS、thread samples、timer/release；start、sampleTree、waitNextSample、stopSampling、collectResult | runProcessSample facade 每次创建实例；runPerformanceHarness 默认 runSample 继续调用 facade | 保留正常采样、close 停表与非零退出；新增真实 spawn ENOENT 与注入零 RSS 的短 fixture。sampler rejection 的既有 child 清理问题未修复，本次不增加终止整树、shared deadline 或恢复保证。 |
| hosts-R2-N18 | 已实施，独立复审通过 | scripts/performance/market-ui-report.mjs 及 .test.mjs | MarketUiReportRun 私有持有配置、server/browser/client/profile/debugPort；startServerIfNeeded、launchBrowser、connectPage、close | main 分别取得 server/browser/client，再在 finally 单次调用 close；指标、截图、report 构造仍为既有显式代码 | 新增正常清理顺序、连接失败、四个清理步骤各自抛错后截断、部分初始化、远程 URL、不重复启动既有页面与已退出 browser 分支。未实际启动 Chrome/Vite 或运行浏览器 E2E；未新增幂等、重试、错误聚合或等待整树保证。 |
| hosts-R2-N21 | 已实施，独立复审通过 | scripts/simulation/escrow-performance-harness.mjs 及 .test.mjs | PerformanceComparisonRun 私有 config 深复制并冻结、两侧 samples、failed 状态；私有 #measureSide、#appendSample，公开 measure、buildReport、matchesReusableReport；原纯 validators/aggregate 保留 | runPerformanceHarness 创建 run；main/loadReusableReport 使用同类运行上下文比较复用报告 | 新增配置快照、不同 run 样本隔离、失败不能构造 PASS、全部现有 comparison key/environment/endpoint/sample count 拒绝复用。没有实际执行性能矩阵。JSON schema、报告键、吞吐/RSS/线程来源、warmup/交替顺序与源码前后检查保持。 |
| hosts-R2-N22 | 已实施，独立复审通过 | scripts/simulation/escrow-verification-contracts.mjs 及 .test.mjs | 模块内 Resource 不变值对象：fromJson、toJson、add、equals、assertEquals；EnvelopeConservation 持有 basis/live/P1/journal sums：applyReceipt、assertP1Boundary、assertCommitAndConservation | verifyConservationSnapshot 每行创建 envelope；receipt global identity/index、账户 aggregate 与 T+1 校验仍由顶层持有 | 新增超过 i64/u64 的非负 decimal、负数/非 canonical 拒绝、无 receipt/仅 P0 的 P1 截点、跨 envelope 顺序接受与 local chain 顺序拒绝；既有多 receipt 跨 journal 测试保持。Resource 没有新增 i64/u64 上界。 |

## 验证证据

- Node v25.8.2。
- PerformanceComparisonRun 生命周期与 MarketUiReportRun 正常清理测试先运行，分别因对象尚不存在的行为断言失败；随后实现通过。Resource 新增测试是现有行为 characterization，实施前通过，实施后也通过。
- 最终三个测试文件通过 Promise.allSettled 同时运行，外层真实进程并发数为 3；每条命令使用 scripts/run-with-deadline.mjs 10000，case 使用 --test-timeout=10000，显式 --test-concurrency=3。
- 本机 Node 的默认 process isolation 只报告整文件 test failed，改用每文件独立进程配合 --test-isolation=none，保持文件之间的进程隔离和外部 deadline；并非扩大时限。
- escrow-performance-harness.test.mjs：15/15 PASS，runner duration 478.890174ms。
- escrow-verification-contracts.test.mjs：12/12 PASS，runner duration 58.848162ms。
- market-ui-report.test.mjs：8/8 PASS，runner duration 48.100377ms。
- git diff --check 对所属六个文件通过。

代表命令（另外两个文件替换最后一个路径；三条命令并行）：

```sh
TMPDIR="$PWD/.tmp/process-tmp" node scripts/run-with-deadline.mjs 10000 -- node --test --test-isolation=none --test-reporter=tap --test-timeout=10000 --test-concurrency=3 scripts/simulation/escrow-performance-harness.test.mjs
```

## 语义与范围

本组仅重构工具链 owner，不改产品交易规则。依据 ADR-0017 的现行修订，继续区分金额分与数量股、Buy 的现金资源与 Sell 的股份资源、P0 释放/P1 截点/SealedBatch、连续全局 receipt index 与 source-local ordinal/T+1；没有新增跨 envelope 交易排序。资源验证器的无上界 BigInt 接受集保持，不把它等同 engine Money/Qty 的范围政策。吞吐只来自 committed ticks / measured wall time，RSS 与 Linux R thread 采样单列，Rayon registry capacity 仍不等同 worker activity。没有修改 ACT/365 或其他产品单位。

未执行完整回归、Cargo、性能矩阵和浏览器 E2E；本次测试只提供代表性短 fixture 的证据。


## 独立复核修复（2026-10-03）

review_performance 在 `../review-performance.md` 登记的 P2-01、P2-02、P3-01 均已修复；所属完整更新 diff 的独立复审已通过。

- P2-01：`#measureSide` 和 `#appendSample` 收为私有方法，只有 `measure` 成功执行每个 source 前后检查和 sample validator 后才追加 sample；外部任意对象不能追加。`#appendSample` 独立复制成功 sample，避免与注入 fixture 的引用共享。
- P2-02：`buildReport` 使用 `structuredClone` 返回完整独立报告，不暴露内部 samples、配置或输入 environment 的嵌套引用；JSON 字段与单位保持。
- P3-01：`runProcessSample` 恢复 `async function` facade；缺失/null options 的参数解构错误继续表现为 rejected Promise，没有添加默认参数掩盖错误。
- 三项新增短测试先红后绿，分别固定禁止外部追加、消费者修改 report 不污染 run、缺失/null options 的异步拒绝。
- 修复后仅运行相关精准用例：7/7 PASS，runner duration 77.514484ms；未重跑其他已通过测试文件或真实 benchmark。

```sh
TMPDIR="$PWD/.tmp/process-tmp" node scripts/run-with-deadline.mjs 10000 -- node --test --test-isolation=none --test-reporter=tap --test-timeout=10000 --test-concurrency=3 --test-name-pattern='PerformanceComparisonRun|异步 facade|records actual paired|parses a config file once' scripts/simulation/escrow-performance-harness.test.mjs
```


## 最终独立复审

review_performance 已完成所属六个源/测试文件完整更新 diff 的独立复审，P2-01/P2-02/P3-01 均关闭，无未修复有效发现。报告见 `../review-performance.md`。Reviewer 独立运行相同精准过滤，7/7 PASS，runner duration 78.672502ms；六文件 diff --check 通过。初版共35 case通过与修后精准7 case通过是不同批次，没有宣称终版整套测试重跑。结构化逐动作台账见 `status.json`。

## Root 最终代表性验证（2026-10-03）

本组最终状态以 status.json 的 final_validation 为准。root build07 与 all-targets check08 均通过；root 选定精确 Rust case 的逐条结果已按 source 映射，WS 两个 bind sandbox EPERM 保留初始失败记录并由沙箱外原断言 2/2 通过核销。Writer 5/5 与桌面 CLI 5/5 通过。未选中的旧测试、完整 suite、API doctest（actors）及真实 matrix/E2E/性能未据此声称执行。历史“worker 未运行/待 root”段落保留为过程证据，当前没有未关闭实施或独立复核发现。
