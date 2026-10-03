# scripts 普通测试结果

使用 Node v25.8.2；32 个文件，最多 4 个文件 worker 并行；每文件进程外 10000ms deadline，每 case 10000ms timeout，文件内 test-concurrency=1、test-isolation=none。可用 CPU：128。

通过 32/32 文件、369/369 case；失败、skipped、cancelled 均为 0。JSON 记录的批次 wall-clock：7820ms。未修改产品或原用例。

## 沙箱首轮与诊断复跑

首轮沙箱内通过 27/32 文件，5 个文件失败，日志 `scripts-*.log` 与 `scripts-results.json` 保留。首轮捕获的 child stdout/stderr 为空，不能据此确定每个失败的原因；随后按相同 10 秒 case/进程树门禁，显式使用 TAP reporter 并重跑这 5 个文件，诊断日志单独保存为 `scripts-sandbox-diagnostic-*.log`。沙箱外在相同 Node v25.8.2 上全部 32 文件通过，说明以下环境限制不应登记为产品断言缺陷：

| 文件 | 沙箱内实际诊断 | 诊断日志 |
|---|---|---|
| scripts/desktop/build-matrix.test.mjs | `spawnSync /bin/bash EPERM`，原本期待 planner 结果的断言因此失败 | [日志](scripts-sandbox-diagnostic-desktop-build-matrix.log) |
| scripts/run-full-regression.test.mjs | `spawnSync Node EPERM`；真实 child 诊断输出未送达，导致进度、编译错误与残缺 JSON 验证失败 | [日志](scripts-sandbox-diagnostic-run-full-regression.log) |
| scripts/run-with-deadline.test.mjs | 真实 child 输出捕获为空，stdout/stderr 断言失败，stream callback 没有收到数据而等到 deadline | [日志](scripts-sandbox-diagnostic-run-with-deadline.log) |
| scripts/wasm-build-dependencies.test.mjs | 三个 case 均报 `spawnSync cargo EPERM` | [日志](scripts-sandbox-diagnostic-wasm-build-dependencies.log) |
| scripts/smoke-deployment.test.mjs | `listen EPERM: operation not permitted 127.0.0.1`；另有 child 输出为空影响 feature 拒绝诊断 | [日志](scripts-sandbox-diagnostic-smoke-deployment.log) |

沙箱外执行已通过工具 approval review。该执行不启动其他完整回归，不改产品或原测试，不发布 Release；只验证这些脚本测试及其短 fixture。

| 文件 | 结果 | 耗时 ms | 日志 |
|---|---|---:|---|
| scripts/build-targets.test.mjs | 通过（exit 0） | 2448 | [scripts-unsandboxed-build-targets.log](scripts-unsandboxed-build-targets.log) |
| scripts/cache-workflow.test.mjs | 通过（exit 0） | 256 | [scripts-unsandboxed-cache-workflow.log](scripts-unsandboxed-cache-workflow.log) |
| scripts/check-doc-symbols.test.mjs | 通过（exit 0） | 259 | [scripts-unsandboxed-check-doc-symbols.log](scripts-unsandboxed-check-doc-symbols.log) |
| scripts/check-web-release-wasm.test.mjs | 通过（exit 0） | 268 | [scripts-unsandboxed-check-web-release-wasm.log](scripts-unsandboxed-check-web-release-wasm.log) |
| scripts/ci-workflow.test.mjs | 通过（exit 0） | 256 | [scripts-unsandboxed-ci-workflow.log](scripts-unsandboxed-ci-workflow.log) |
| scripts/desktop/build-matrix.test.mjs | 通过（exit 0） | 4531 | [scripts-unsandboxed-desktop-build-matrix.log](scripts-unsandboxed-desktop-build-matrix.log) |
| scripts/distribution-macos-cache.test.mjs | 通过（exit 0） | 245 | [scripts-unsandboxed-distribution-macos-cache.log](scripts-unsandboxed-distribution-macos-cache.log) |
| scripts/distribution-workflow.test.mjs | 通过（exit 0） | 250 | [scripts-unsandboxed-distribution-workflow.log](scripts-unsandboxed-distribution-workflow.log) |
| scripts/frontend-build.test.mjs | 通过（exit 0） | 281 | [scripts-unsandboxed-frontend-build.log](scripts-unsandboxed-frontend-build.log) |
| scripts/full-regression-web-worker.test.mjs | 通过（exit 0） | 253 | [scripts-unsandboxed-full-regression-web-worker.log](scripts-unsandboxed-full-regression-web-worker.log) |
| scripts/package-distributions.test.mjs | 通过（exit 0） | 442 | [scripts-unsandboxed-package-distributions.log](scripts-unsandboxed-package-distributions.log) |
| scripts/package-static-web.test.mjs | 通过（exit 0） | 281 | [scripts-unsandboxed-package-static-web.log](scripts-unsandboxed-package-static-web.log) |
| scripts/pages-isolation.test.mjs | 通过（exit 0） | 373 | [scripts-unsandboxed-pages-isolation.log](scripts-unsandboxed-pages-isolation.log) |
| scripts/performance/market-ui-report.test.mjs | 通过（exit 0） | 263 | [scripts-unsandboxed-performance-market-ui-report.log](scripts-unsandboxed-performance-market-ui-report.log) |
| scripts/prune-actions-cache.test.mjs | 通过（exit 0） | 302 | [scripts-unsandboxed-prune-actions-cache.log](scripts-unsandboxed-prune-actions-cache.log) |
| scripts/publish-release.test.mjs | 通过（exit 0） | 334 | [scripts-unsandboxed-publish-release.log](scripts-unsandboxed-publish-release.log) |
| scripts/release-policy.test.mjs | 通过（exit 0） | 253 | [scripts-unsandboxed-release-policy.log](scripts-unsandboxed-release-policy.log) |
| scripts/run-full-regression.test.mjs | 通过（exit 0） | 1938 | [scripts-unsandboxed-run-full-regression.log](scripts-unsandboxed-run-full-regression.log) |
| scripts/run-long-validation.test.mjs | 通过（exit 0） | 351 | [scripts-unsandboxed-run-long-validation.log](scripts-unsandboxed-run-long-validation.log) |
| scripts/run-web-shard-isolation.test.mjs | 通过（exit 0） | 259 | [scripts-unsandboxed-run-web-shard-isolation.log](scripts-unsandboxed-run-web-shard-isolation.log) |
| scripts/run-web-tests.test.mjs | 通过（exit 0） | 266 | [scripts-unsandboxed-run-web-tests.log](scripts-unsandboxed-run-web-tests.log) |
| scripts/run-with-deadline.test.mjs | 通过（exit 0） | 1147 | [scripts-unsandboxed-run-with-deadline.log](scripts-unsandboxed-run-with-deadline.log) |
| scripts/simulation/audit-diagnostic-divergence.test.mjs | 通过（exit 0） | 631 | [scripts-unsandboxed-simulation-audit-diagnostic-divergence.log](scripts-unsandboxed-simulation-audit-diagnostic-divergence.log) |
| scripts/simulation/baseline-run.test.mjs | 通过（exit 0） | 4309 | [scripts-unsandboxed-simulation-baseline-run.log](scripts-unsandboxed-simulation-baseline-run.log) |
| scripts/simulation/escrow-performance-harness.test.mjs | 通过（exit 0） | 692 | [scripts-unsandboxed-simulation-escrow-performance-harness.log](scripts-unsandboxed-simulation-escrow-performance-harness.log) |
| scripts/simulation/escrow-source-manifest.test.mjs | 通过（exit 0） | 305 | [scripts-unsandboxed-simulation-escrow-source-manifest.log](scripts-unsandboxed-simulation-escrow-source-manifest.log) |
| scripts/simulation/escrow-verification-contracts.test.mjs | 通过（exit 0） | 285 | [scripts-unsandboxed-simulation-escrow-verification-contracts.log](scripts-unsandboxed-simulation-escrow-verification-contracts.log) |
| scripts/simulation/prepare-escrow-performance.test.mjs | 通过（exit 0） | 788 | [scripts-unsandboxed-simulation-prepare-escrow-performance.log](scripts-unsandboxed-simulation-prepare-escrow-performance.log) |
| scripts/simulation/run-escrow-verification-matrix.test.mjs | 通过（exit 0） | 1178 | [scripts-unsandboxed-simulation-run-escrow-verification-matrix.log](scripts-unsandboxed-simulation-run-escrow-verification-matrix.log) |
| scripts/simulation/verify-simulation-artifacts.test.mjs | 通过（exit 0） | 3011 | [scripts-unsandboxed-simulation-verify-simulation-artifacts.log](scripts-unsandboxed-simulation-verify-simulation-artifacts.log) |
| scripts/smoke-deployment.test.mjs | 通过（exit 0） | 2446 | [scripts-unsandboxed-smoke-deployment.log](scripts-unsandboxed-smoke-deployment.log) |
| scripts/wasm-build-dependencies.test.mjs | 通过（exit 0） | 1005 | [scripts-unsandboxed-wasm-build-dependencies.log](scripts-unsandboxed-wasm-build-dependencies.log) |
