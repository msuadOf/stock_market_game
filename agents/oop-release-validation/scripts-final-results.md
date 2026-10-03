# scripts 普通测试结果

使用 Node v25.8.2；32 个文件，最多 4 个文件 worker 并行；每文件进程外 10000ms deadline，每 case 10000ms timeout，文件内 test-concurrency=1、test-isolation=none。可用 CPU：128。

通过 32/32，失败 0/32。批次 wall-clock：14234ms。未修改产品或原用例。

| 文件 | 结果 | 耗时 ms | 日志 |
|---|---|---:|---|
| scripts/build-targets.test.mjs | 通过（exit 0） | 2429 | [scripts-final-build-targets.log](scripts-final-build-targets.log) |
| scripts/cache-workflow.test.mjs | 通过（exit 0） | 274 | [scripts-final-cache-workflow.log](scripts-final-cache-workflow.log) |
| scripts/check-doc-symbols.test.mjs | 通过（exit 0） | 262 | [scripts-final-check-doc-symbols.log](scripts-final-check-doc-symbols.log) |
| scripts/check-web-release-wasm.test.mjs | 通过（exit 0） | 273 | [scripts-final-check-web-release-wasm.log](scripts-final-check-web-release-wasm.log) |
| scripts/ci-workflow.test.mjs | 通过（exit 0） | 268 | [scripts-final-ci-workflow.log](scripts-final-ci-workflow.log) |
| scripts/desktop/build-matrix.test.mjs | 通过（exit 0） | 4615 | [scripts-final-desktop-build-matrix.log](scripts-final-desktop-build-matrix.log) |
| scripts/distribution-macos-cache.test.mjs | 通过（exit 0） | 247 | [scripts-final-distribution-macos-cache.log](scripts-final-distribution-macos-cache.log) |
| scripts/distribution-workflow.test.mjs | 通过（exit 0） | 261 | [scripts-final-distribution-workflow.log](scripts-final-distribution-workflow.log) |
| scripts/frontend-build.test.mjs | 通过（exit 0） | 296 | [scripts-final-frontend-build.log](scripts-final-frontend-build.log) |
| scripts/full-regression-web-worker.test.mjs | 通过（exit 0） | 265 | [scripts-final-full-regression-web-worker.log](scripts-final-full-regression-web-worker.log) |
| scripts/package-distributions.test.mjs | 通过（exit 0） | 490 | [scripts-final-package-distributions.log](scripts-final-package-distributions.log) |
| scripts/package-static-web.test.mjs | 通过（exit 0） | 286 | [scripts-final-package-static-web.log](scripts-final-package-static-web.log) |
| scripts/pages-isolation.test.mjs | 通过（exit 0） | 377 | [scripts-final-pages-isolation.log](scripts-final-pages-isolation.log) |
| scripts/performance/market-ui-report.test.mjs | 通过（exit 0） | 272 | [scripts-final-performance-market-ui-report.log](scripts-final-performance-market-ui-report.log) |
| scripts/prune-actions-cache.test.mjs | 通过（exit 0） | 288 | [scripts-final-prune-actions-cache.log](scripts-final-prune-actions-cache.log) |
| scripts/publish-release.test.mjs | 通过（exit 0） | 361 | [scripts-final-publish-release.log](scripts-final-publish-release.log) |
| scripts/release-policy.test.mjs | 通过（exit 0） | 254 | [scripts-final-release-policy.log](scripts-final-release-policy.log) |
| scripts/run-full-regression.test.mjs | 通过（exit 0） | 1944 | [scripts-final-run-full-regression.log](scripts-final-run-full-regression.log) |
| scripts/run-long-validation.test.mjs | 通过（exit 0） | 339 | [scripts-final-run-long-validation.log](scripts-final-run-long-validation.log) |
| scripts/run-web-shard-isolation.test.mjs | 通过（exit 0） | 245 | [scripts-final-run-web-shard-isolation.log](scripts-final-run-web-shard-isolation.log) |
| scripts/run-web-tests.test.mjs | 通过（exit 0） | 290 | [scripts-final-run-web-tests.log](scripts-final-run-web-tests.log) |
| scripts/run-with-deadline.test.mjs | 通过（exit 0） | 1128 | [scripts-final-run-with-deadline.log](scripts-final-run-with-deadline.log) |
| scripts/simulation/audit-diagnostic-divergence.test.mjs | 通过（exit 0） | 633 | [scripts-final-simulation-audit-diagnostic-divergence.log](scripts-final-simulation-audit-diagnostic-divergence.log) |
| scripts/simulation/baseline-run.test.mjs | 通过（exit 0） | 3670 | [scripts-final-simulation-baseline-run.log](scripts-final-simulation-baseline-run.log) |
| scripts/simulation/escrow-performance-harness.test.mjs | 通过（exit 0） | 693 | [scripts-final-simulation-escrow-performance-harness.log](scripts-final-simulation-escrow-performance-harness.log) |
| scripts/simulation/escrow-source-manifest.test.mjs | 通过（exit 0） | 275 | [scripts-final-simulation-escrow-source-manifest.log](scripts-final-simulation-escrow-source-manifest.log) |
| scripts/simulation/escrow-verification-contracts.test.mjs | 通过（exit 0） | 283 | [scripts-final-simulation-escrow-verification-contracts.log](scripts-final-simulation-escrow-verification-contracts.log) |
| scripts/simulation/prepare-escrow-performance.test.mjs | 通过（exit 0） | 927 | [scripts-final-simulation-prepare-escrow-performance.log](scripts-final-simulation-prepare-escrow-performance.log) |
| scripts/simulation/run-escrow-verification-matrix.test.mjs | 通过（exit 0） | 1467 | [scripts-final-simulation-run-escrow-verification-matrix.log](scripts-final-simulation-run-escrow-verification-matrix.log) |
| scripts/simulation/verify-simulation-artifacts.test.mjs | 通过（exit 0） | 2495 | [scripts-final-simulation-verify-simulation-artifacts.log](scripts-final-simulation-verify-simulation-artifacts.log) |
| scripts/smoke-deployment.test.mjs | 通过（exit 0） | 2439 | [scripts-final-smoke-deployment.log](scripts-final-smoke-deployment.log) |
| scripts/wasm-build-dependencies.test.mjs | 通过（exit 0） | 8182 | [scripts-final-wasm-build-dependencies.log](scripts-final-wasm-build-dependencies.log) |
