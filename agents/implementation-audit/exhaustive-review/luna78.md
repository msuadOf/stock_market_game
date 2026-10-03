# scripts 结果记录独立全文复核

复核对象：merge `a7c7ce357bdc9f88c03633744b2d5815db49e9b2`（合入产品提交 `08e4fc7`）；限定 `agents/oop-release-validation/scripts-results.md`、`scripts-supervised-results.md`、`scripts-unsandboxed-results.md` 及其支撑脚本、32 个 `scripts/**/*.test.mjs` 的测试声明与聚合调用链。没有运行测试，没有修改产品或 Git 状态。本结论只审阅文档记载和当前源码；不把历史 `PASS` 当作本次执行或独立复现。

## 逐章复核

| 文档章节 | 原记录及复核 | 判断 |
|---|---|---|
| `scripts-results.md` 首段与 32 文件表 | Node `v25.8.2`、4 个 file worker、每文件外部 10 秒、每 case 10 秒、`test-concurrency=1`、`test-isolation=none`、128 CPU；记录 27/32，失败 `desktop/build-matrix`、`run-full-regression`、`run-with-deadline`、`smoke-deployment`、`wasm-build-dependencies`。表中的五项均明确 exit 1，其余 27 项 exit 0。 | 这是沙箱首轮历史记录，不可当最终验收。表明当轮全文件批次失败。 |
| `scripts-supervised-results.md` 首段与 32 文件表 | 相同 Node、并发、逐文件及逐 case 门禁；记录 32/32 exit 0，批次 `13675ms`。最长所列单文件为 `wasm-build-dependencies` `7961ms`。 | 支持后续 supervised 批次文件级通过；批次超过 10 秒，因此应按长验收看待。它没有列 case 总数或每 case TAP 结果。 |
| `scripts-unsandboxed-results.md` 首段及“沙箱首轮与诊断复跑” | 首轮 27/32 与前表相符；诊断称五项分别遇到 `spawnSync /bin/bash EPERM`、`spawnSync Node EPERM`、真实 child 输出空、`spawnSync cargo EPERM`、loopback `listen EPERM`。诊断行明确不能从空 child 输出推定具体原因；后续报告沙箱外 32/32 通过。 | 有后续环境对照，五个首轮失败不应直接归为产品断言缺陷；这是记录所支持的最窄解释。具体原因依赖文档转述，诊断日志当前不在工作树中，无法独立核对。 |
| `scripts-unsandboxed-results.md` 沙箱外 32 文件表 | 全部 32 项 exit 0；最大列出耗时 `4531ms`（desktop build matrix）。 | 记录了沙箱外逐文件通过。不能据此扩大为 Rust/Web/构建/发布全量验收，也不等同于复核测试断言质量。 |

## 每文件结果交叉矩阵

以下按三份指定文档逐行对照。`首轮`为 `scripts-results.md`，`监督`为 `scripts-supervised-results.md`，`沙箱外`为 `scripts-unsandboxed-results.md`。这是文档历史状态矩阵，不是复跑结果。

| 测试文件 | 首轮 | 监督 | 沙箱外 | 交叉复核 |
|---|---:|---:|---:|---|
| `scripts/build-targets.test.mjs` | 通过 | 通过 | 通过 | 三表一致通过 |
| `scripts/cache-workflow.test.mjs` | 通过 | 通过 | 通过 | 三表一致通过 |
| `scripts/check-doc-symbols.test.mjs` | 通过 | 通过 | 通过 | 三表一致通过 |
| `scripts/check-web-release-wasm.test.mjs` | 通过 | 通过 | 通过 | 三表一致通过 |
| `scripts/ci-workflow.test.mjs` | 通过 | 通过 | 通过 | 三表一致通过 |
| `scripts/desktop/build-matrix.test.mjs` | 失败 | 通过 | 通过 | 沙箱首轮 EPERM 诊断后，两份后续记录通过 |
| `scripts/distribution-macos-cache.test.mjs` | 通过 | 通过 | 通过 | 三表一致通过 |
| `scripts/distribution-workflow.test.mjs` | 通过 | 通过 | 通过 | 三表一致通过 |
| `scripts/frontend-build.test.mjs` | 通过 | 通过 | 通过 | 三表一致通过 |
| `scripts/full-regression-web-worker.test.mjs` | 通过 | 通过 | 通过 | 三表一致通过 |
| `scripts/package-distributions.test.mjs` | 通过 | 通过 | 通过 | 三表一致通过 |
| `scripts/package-static-web.test.mjs` | 通过 | 通过 | 通过 | 三表一致通过 |
| `scripts/pages-isolation.test.mjs` | 通过 | 通过 | 通过 | 三表一致通过 |
| `scripts/performance/market-ui-report.test.mjs` | 通过 | 通过 | 通过 | 三表一致通过 |
| `scripts/prune-actions-cache.test.mjs` | 通过 | 通过 | 通过 | 三表一致通过 |
| `scripts/publish-release.test.mjs` | 通过 | 通过 | 通过 | 三表一致通过 |
| `scripts/release-policy.test.mjs` | 通过 | 通过 | 通过 | 三表一致通过 |
| `scripts/run-full-regression.test.mjs` | 失败 | 通过 | 通过 | 沙箱首轮 child spawn EPERM 诊断后，两份后续记录通过 |
| `scripts/run-long-validation.test.mjs` | 通过 | 通过 | 通过 | 三表一致通过 |
| `scripts/run-web-shard-isolation.test.mjs` | 通过 | 通过 | 通过 | 三表一致通过 |
| `scripts/run-web-tests.test.mjs` | 通过 | 通过 | 通过 | 三表一致通过 |
| `scripts/run-with-deadline.test.mjs` | 失败 | 通过 | 通过 | 沙箱首轮捕获 child stream 失败诊断后，两份后续记录通过 |
| `scripts/simulation/audit-diagnostic-divergence.test.mjs` | 通过 | 通过 | 通过 | 三表一致通过 |
| `scripts/simulation/baseline-run.test.mjs` | 通过 | 通过 | 通过 | 三表一致通过 |
| `scripts/simulation/escrow-performance-harness.test.mjs` | 通过 | 通过 | 通过 | 三表一致通过 |
| `scripts/simulation/escrow-source-manifest.test.mjs` | 通过 | 通过 | 通过 | 三表一致通过 |
| `scripts/simulation/escrow-verification-contracts.test.mjs` | 通过 | 通过 | 通过 | 三表一致通过 |
| `scripts/simulation/prepare-escrow-performance.test.mjs` | 通过 | 通过 | 通过 | 三表一致通过 |
| `scripts/simulation/run-escrow-verification-matrix.test.mjs` | 通过 | 通过 | 通过 | 三表一致通过 |
| `scripts/simulation/verify-simulation-artifacts.test.mjs` | 通过 | 通过 | 通过 | 三表一致通过 |
| `scripts/smoke-deployment.test.mjs` | 失败 | 通过 | 通过 | 沙箱首轮 loopback/child 诊断后，两份后续记录通过 |
| `scripts/wasm-build-dependencies.test.mjs` | 失败 | 通过 | 通过 | 沙箱首轮 cargo spawn EPERM 诊断后，两份后续记录通过 |

## Case、并发与监督链

- 三份指定结果的粒度是文件 exit code 和 wall time；没有逐 case TAP 输出、case 名称、case 结果数或 skipped/cancelled 数。因而可以交叉核对 32 个文件状态，不能从这些 Markdown 独立证明“每个 case 都 pass”。当前源码静态测试声明数不能替代某一次运行的 TAP 结果。
- `scripts-results.md` 的五个失败是首轮观察；`scripts-unsandboxed-results.md` 的诊断说明和沙箱外重跑构成后续信息。复核仅接受“后续记录报告通过”，不将首轮失败从历史中抹掉，也不声称独立复现了 EPERM。
- `agents/oop-release-validation/run-scripts-tests.mjs` 递归发现 `scripts` 下 `.test.mjs` 并排序；以 4 个 worker 分配文件，每文件 child 调 `scripts/run-with-deadline.mjs 10000 -- node --test --test-timeout=10000 --test-isolation=none --test-concurrency=1 --test-reporter=tap`，每文件写日志与 JSON，再生成 Markdown 汇总。其报告计时覆盖发现、逐文件等待与结果写入。
- 聚合入口调用 `runBoundedCommand` 包裹同脚本的 worker 模式，`timeoutMs: 300000`、`cleanupReserveMs: 1000`。`scripts/run-with-deadline.mjs` 在另一个 Node 监督进程中计时，执行截止为 299000ms，预留 1000ms 终止进程树和收尾；被监督 worker 的 event loop 不负责自己的聚合截止。每个文件另有 10000ms 外层进程 deadline，Node runner 另有 10000ms case timeout。故 `13675ms` 批次不违反“长验收 300000ms”，也不应表述为普通测试整命令小于 10 秒。
- 并发证据是 4 个文件 worker、单文件 case 串行及可用 CPU 128。它说明批次存在跨文件并发，不证明运行期间实际占满 4 核或 CPU 利用率；“显式多核”在这个短脚本批次中由并行文件 worker体现。监督记录没有进程树利用率采样，不应补写实际 CPU 利用率结论。
- 在当前工作树中三份 Markdown 所链接的逐文件 `.log` 与 `scripts-*-results.json` 均不存在；文件名指向可读的预期证据，但日志正文无法现场核销。`scripts-supervised-results.md` 自身只记 32/32 files，不提供 aggregate TAP case totals。`summary.md` 称 370 case，而 `scripts-unsandboxed-results.md` 称 369/369 case；指定三文件中没有数据可消解这个一例差异。它是本次发现的证据口径候选，不能据此断言某个 case 遗漏或失败。

## 旧结论复核与新候选

旧记录“沙箱五项失败、后续沙箱外全部通过”的结论与三份表格互相吻合，保留为**有条件通过**：范围限于这 32 个 scripts 测试文件及各记录所述环境。`scripts-supervised` 报告支持其独立 supervised 批次通过；`scripts-unsandboxed` 报告支持沙箱外通过。历史通过不能替代当前实现验证，也不能推导产品其他测试通过。

新候选 E1（低至中）：case 总数口径 `369` / `370` 冲突；supervised 结果只有文件级 exit code，日志与 JSON 又不在工作树，无法按逐 case 计数解决。建议正式验收记录统一用同一份保留的 TAP/JSON 清单生成文件数、case 数及 skipped/cancelled 数；在完成核对前，摘要不要把 `370` 或 `369/369` 当作经本次独立复核确认的事实。这是报告可审计性问题，不是已证实产品或测试代码错误。

未发现可由这三份结果文档证明的 A 股交易语义变化；涉及的是脚本验证工具链。没有将结果表中的 PASS 扩展解释为业务正确性或发布成功。
