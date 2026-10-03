# scripts 完整脚本回归（长验收）

使用 Node v25.8.2；32 个文件，最多 4 个文件 worker 并行；每文件进程外 10000ms deadline，每 case 10000ms timeout，文件内 test-concurrency=1、test-isolation=none。可用 CPU：128。

通过 32/32，失败 0/32。批次 wall-clock：13675ms。本文件记录此批次执行结果，不代表测试执行器修改了产品或原用例。

| 文件 | 结果 | 耗时 ms | 日志 |
|---|---|---:|---|
| scripts/build-targets.test.mjs | 通过（exit 0） | 2379 | [scripts-supervised-build-targets.log](scripts-supervised-build-targets.log) |
| scripts/cache-workflow.test.mjs | 通过（exit 0） | 266 | [scripts-supervised-cache-workflow.log](scripts-supervised-cache-workflow.log) |
| scripts/check-doc-symbols.test.mjs | 通过（exit 0） | 260 | [scripts-supervised-check-doc-symbols.log](scripts-supervised-check-doc-symbols.log) |
| scripts/check-web-release-wasm.test.mjs | 通过（exit 0） | 262 | [scripts-supervised-check-web-release-wasm.log](scripts-supervised-check-web-release-wasm.log) |
| scripts/ci-workflow.test.mjs | 通过（exit 0） | 261 | [scripts-supervised-ci-workflow.log](scripts-supervised-ci-workflow.log) |
| scripts/desktop/build-matrix.test.mjs | 通过（exit 0） | 4584 | [scripts-supervised-desktop-build-matrix.log](scripts-supervised-desktop-build-matrix.log) |
| scripts/distribution-macos-cache.test.mjs | 通过（exit 0） | 260 | [scripts-supervised-distribution-macos-cache.log](scripts-supervised-distribution-macos-cache.log) |
| scripts/distribution-workflow.test.mjs | 通过（exit 0） | 246 | [scripts-supervised-distribution-workflow.log](scripts-supervised-distribution-workflow.log) |
| scripts/frontend-build.test.mjs | 通过（exit 0） | 292 | [scripts-supervised-frontend-build.log](scripts-supervised-frontend-build.log) |
| scripts/full-regression-web-worker.test.mjs | 通过（exit 0） | 253 | [scripts-supervised-full-regression-web-worker.log](scripts-supervised-full-regression-web-worker.log) |
| scripts/package-distributions.test.mjs | 通过（exit 0） | 471 | [scripts-supervised-package-distributions.log](scripts-supervised-package-distributions.log) |
| scripts/package-static-web.test.mjs | 通过（exit 0） | 280 | [scripts-supervised-package-static-web.log](scripts-supervised-package-static-web.log) |
| scripts/pages-isolation.test.mjs | 通过（exit 0） | 382 | [scripts-supervised-pages-isolation.log](scripts-supervised-pages-isolation.log) |
| scripts/performance/market-ui-report.test.mjs | 通过（exit 0） | 264 | [scripts-supervised-performance-market-ui-report.log](scripts-supervised-performance-market-ui-report.log) |
| scripts/prune-actions-cache.test.mjs | 通过（exit 0） | 284 | [scripts-supervised-prune-actions-cache.log](scripts-supervised-prune-actions-cache.log) |
| scripts/publish-release.test.mjs | 通过（exit 0） | 333 | [scripts-supervised-publish-release.log](scripts-supervised-publish-release.log) |
| scripts/release-policy.test.mjs | 通过（exit 0） | 252 | [scripts-supervised-release-policy.log](scripts-supervised-release-policy.log) |
| scripts/run-full-regression.test.mjs | 通过（exit 0） | 1934 | [scripts-supervised-run-full-regression.log](scripts-supervised-run-full-regression.log) |
| scripts/run-long-validation.test.mjs | 通过（exit 0） | 345 | [scripts-supervised-run-long-validation.log](scripts-supervised-run-long-validation.log) |
| scripts/run-web-shard-isolation.test.mjs | 通过（exit 0） | 255 | [scripts-supervised-run-web-shard-isolation.log](scripts-supervised-run-web-shard-isolation.log) |
| scripts/run-web-tests.test.mjs | 通过（exit 0） | 273 | [scripts-supervised-run-web-tests.log](scripts-supervised-run-web-tests.log) |
| scripts/run-with-deadline.test.mjs | 通过（exit 0） | 1138 | [scripts-supervised-run-with-deadline.log](scripts-supervised-run-with-deadline.log) |
| scripts/simulation/audit-diagnostic-divergence.test.mjs | 通过（exit 0） | 671 | [scripts-supervised-simulation-audit-diagnostic-divergence.log](scripts-supervised-simulation-audit-diagnostic-divergence.log) |
| scripts/simulation/baseline-run.test.mjs | 通过（exit 0） | 3594 | [scripts-supervised-simulation-baseline-run.log](scripts-supervised-simulation-baseline-run.log) |
| scripts/simulation/escrow-performance-harness.test.mjs | 通过（exit 0） | 700 | [scripts-supervised-simulation-escrow-performance-harness.log](scripts-supervised-simulation-escrow-performance-harness.log) |
| scripts/simulation/escrow-source-manifest.test.mjs | 通过（exit 0） | 285 | [scripts-supervised-simulation-escrow-source-manifest.log](scripts-supervised-simulation-escrow-source-manifest.log) |
| scripts/simulation/escrow-verification-contracts.test.mjs | 通过（exit 0） | 274 | [scripts-supervised-simulation-escrow-verification-contracts.log](scripts-supervised-simulation-escrow-verification-contracts.log) |
| scripts/simulation/prepare-escrow-performance.test.mjs | 通过（exit 0） | 975 | [scripts-supervised-simulation-prepare-escrow-performance.log](scripts-supervised-simulation-prepare-escrow-performance.log) |
| scripts/simulation/run-escrow-verification-matrix.test.mjs | 通过（exit 0） | 1191 | [scripts-supervised-simulation-run-escrow-verification-matrix.log](scripts-supervised-simulation-run-escrow-verification-matrix.log) |
| scripts/simulation/verify-simulation-artifacts.test.mjs | 通过（exit 0） | 2416 | [scripts-supervised-simulation-verify-simulation-artifacts.log](scripts-supervised-simulation-verify-simulation-artifacts.log) |
| scripts/smoke-deployment.test.mjs | 通过（exit 0） | 2426 | [scripts-supervised-smoke-deployment.log](scripts-supervised-smoke-deployment.log) |
| scripts/wasm-build-dependencies.test.mjs | 通过（exit 0） | 7961 | [scripts-supervised-wasm-build-dependencies.log](scripts-supervised-wasm-build-dependencies.log) |
