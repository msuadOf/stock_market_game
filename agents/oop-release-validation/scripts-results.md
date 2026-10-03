# scripts 普通测试结果

使用 Node v25.8.2；32 个文件，最多 4 个文件 worker 并行；每文件进程外 10000ms deadline，每 case 10000ms timeout，文件内 test-concurrency=1、test-isolation=none。可用 CPU：128。

通过 27/32，失败 5/32。批次 wall-clock：8357ms。未修改产品或原用例。

| 文件 | 结果 | 耗时 ms | 日志 |
|---|---|---:|---|
| scripts/build-targets.test.mjs | 通过（exit 0） | 2930 | [scripts-build-targets.log](scripts-build-targets.log) |
| scripts/cache-workflow.test.mjs | 通过（exit 0） | 248 | [scripts-cache-workflow.log](scripts-cache-workflow.log) |
| scripts/check-doc-symbols.test.mjs | 通过（exit 0） | 252 | [scripts-check-doc-symbols.log](scripts-check-doc-symbols.log) |
| scripts/check-web-release-wasm.test.mjs | 通过（exit 0） | 249 | [scripts-check-web-release-wasm.log](scripts-check-web-release-wasm.log) |
| scripts/ci-workflow.test.mjs | 通过（exit 0） | 245 | [scripts-ci-workflow.log](scripts-ci-workflow.log) |
| scripts/desktop/build-matrix.test.mjs | 失败（exit 1） | 3519 | [scripts-desktop-build-matrix.log](scripts-desktop-build-matrix.log) |
| scripts/distribution-macos-cache.test.mjs | 通过（exit 0） | 259 | [scripts-distribution-macos-cache.log](scripts-distribution-macos-cache.log) |
| scripts/distribution-workflow.test.mjs | 通过（exit 0） | 243 | [scripts-distribution-workflow.log](scripts-distribution-workflow.log) |
| scripts/frontend-build.test.mjs | 通过（exit 0） | 295 | [scripts-frontend-build.log](scripts-frontend-build.log) |
| scripts/full-regression-web-worker.test.mjs | 通过（exit 0） | 251 | [scripts-full-regression-web-worker.log](scripts-full-regression-web-worker.log) |
| scripts/package-distributions.test.mjs | 通过（exit 0） | 2085 | [scripts-package-distributions.log](scripts-package-distributions.log) |
| scripts/package-static-web.test.mjs | 通过（exit 0） | 265 | [scripts-package-static-web.log](scripts-package-static-web.log) |
| scripts/pages-isolation.test.mjs | 通过（exit 0） | 379 | [scripts-pages-isolation.log](scripts-pages-isolation.log) |
| scripts/performance/market-ui-report.test.mjs | 通过（exit 0） | 253 | [scripts-performance-market-ui-report.log](scripts-performance-market-ui-report.log) |
| scripts/prune-actions-cache.test.mjs | 通过（exit 0） | 497 | [scripts-prune-actions-cache.log](scripts-prune-actions-cache.log) |
| scripts/publish-release.test.mjs | 通过（exit 0） | 538 | [scripts-publish-release.log](scripts-publish-release.log) |
| scripts/release-policy.test.mjs | 通过（exit 0） | 258 | [scripts-release-policy.log](scripts-release-policy.log) |
| scripts/run-full-regression.test.mjs | 失败（exit 1） | 1656 | [scripts-run-full-regression.log](scripts-run-full-regression.log) |
| scripts/run-long-validation.test.mjs | 通过（exit 0） | 331 | [scripts-run-long-validation.log](scripts-run-long-validation.log) |
| scripts/run-web-shard-isolation.test.mjs | 通过（exit 0） | 252 | [scripts-run-web-shard-isolation.log](scripts-run-web-shard-isolation.log) |
| scripts/run-web-tests.test.mjs | 通过（exit 0） | 285 | [scripts-run-web-tests.log](scripts-run-web-tests.log) |
| scripts/run-with-deadline.test.mjs | 失败（exit 1） | 1867 | [scripts-run-with-deadline.log](scripts-run-with-deadline.log) |
| scripts/simulation/audit-diagnostic-divergence.test.mjs | 通过（exit 0） | 692 | [scripts-simulation-audit-diagnostic-divergence.log](scripts-simulation-audit-diagnostic-divergence.log) |
| scripts/simulation/baseline-run.test.mjs | 通过（exit 0） | 4293 | [scripts-simulation-baseline-run.log](scripts-simulation-baseline-run.log) |
| scripts/simulation/escrow-performance-harness.test.mjs | 通过（exit 0） | 734 | [scripts-simulation-escrow-performance-harness.log](scripts-simulation-escrow-performance-harness.log) |
| scripts/simulation/escrow-source-manifest.test.mjs | 通过（exit 0） | 284 | [scripts-simulation-escrow-source-manifest.log](scripts-simulation-escrow-source-manifest.log) |
| scripts/simulation/escrow-verification-contracts.test.mjs | 通过（exit 0） | 267 | [scripts-simulation-escrow-verification-contracts.log](scripts-simulation-escrow-verification-contracts.log) |
| scripts/simulation/prepare-escrow-performance.test.mjs | 通过（exit 0） | 1126 | [scripts-simulation-prepare-escrow-performance.log](scripts-simulation-prepare-escrow-performance.log) |
| scripts/simulation/run-escrow-verification-matrix.test.mjs | 通过（exit 0） | 1201 | [scripts-simulation-run-escrow-verification-matrix.log](scripts-simulation-run-escrow-verification-matrix.log) |
| scripts/simulation/verify-simulation-artifacts.test.mjs | 通过（exit 0） | 2349 | [scripts-simulation-verify-simulation-artifacts.log](scripts-simulation-verify-simulation-artifacts.log) |
| scripts/smoke-deployment.test.mjs | 失败（exit 1） | 2179 | [scripts-smoke-deployment.log](scripts-smoke-deployment.log) |
| scripts/wasm-build-dependencies.test.mjs | 失败（exit 1） | 1097 | [scripts-wasm-build-dependencies.log](scripts-wasm-build-dependencies.log) |
