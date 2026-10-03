# scripts 完整脚本回归（长验收）

使用 Node v25.8.2；32 个文件，最多 4 个文件 worker 并行；每文件进程外 10000ms deadline，每 case 10000ms timeout，文件内 test-concurrency=1、test-isolation=none。可用 CPU：128。

通过 32/32，失败 0/32。批次 wall-clock：16904ms。本文件记录此批次执行结果，不代表测试执行器修改了产品或原用例。

| 文件 | 结果 | 耗时 ms | 日志 |
|---|---|---:|---|
| scripts/build-targets.test.mjs | 通过（exit 0） | 2396 | [scripts-isolated-final-build-targets.log](scripts-isolated-final-build-targets.log) |
| scripts/cache-workflow.test.mjs | 通过（exit 0） | 252 | [scripts-isolated-final-cache-workflow.log](scripts-isolated-final-cache-workflow.log) |
| scripts/check-doc-symbols.test.mjs | 通过（exit 0） | 270 | [scripts-isolated-final-check-doc-symbols.log](scripts-isolated-final-check-doc-symbols.log) |
| scripts/check-web-release-wasm.test.mjs | 通过（exit 0） | 271 | [scripts-isolated-final-check-web-release-wasm.log](scripts-isolated-final-check-web-release-wasm.log) |
| scripts/ci-workflow.test.mjs | 通过（exit 0） | 266 | [scripts-isolated-final-ci-workflow.log](scripts-isolated-final-ci-workflow.log) |
| scripts/desktop/build-matrix.test.mjs | 通过（exit 0） | 4649 | [scripts-isolated-final-desktop-build-matrix.log](scripts-isolated-final-desktop-build-matrix.log) |
| scripts/distribution-macos-cache.test.mjs | 通过（exit 0） | 248 | [scripts-isolated-final-distribution-macos-cache.log](scripts-isolated-final-distribution-macos-cache.log) |
| scripts/distribution-workflow.test.mjs | 通过（exit 0） | 258 | [scripts-isolated-final-distribution-workflow.log](scripts-isolated-final-distribution-workflow.log) |
| scripts/frontend-build.test.mjs | 通过（exit 0） | 294 | [scripts-isolated-final-frontend-build.log](scripts-isolated-final-frontend-build.log) |
| scripts/full-regression-web-worker.test.mjs | 通过（exit 0） | 275 | [scripts-isolated-final-full-regression-web-worker.log](scripts-isolated-final-full-regression-web-worker.log) |
| scripts/package-distributions.test.mjs | 通过（exit 0） | 440 | [scripts-isolated-final-package-distributions.log](scripts-isolated-final-package-distributions.log) |
| scripts/package-static-web.test.mjs | 通过（exit 0） | 288 | [scripts-isolated-final-package-static-web.log](scripts-isolated-final-package-static-web.log) |
| scripts/pages-isolation.test.mjs | 通过（exit 0） | 363 | [scripts-isolated-final-pages-isolation.log](scripts-isolated-final-pages-isolation.log) |
| scripts/performance/market-ui-report.test.mjs | 通过（exit 0） | 264 | [scripts-isolated-final-performance-market-ui-report.log](scripts-isolated-final-performance-market-ui-report.log) |
| scripts/prune-actions-cache.test.mjs | 通过（exit 0） | 302 | [scripts-isolated-final-prune-actions-cache.log](scripts-isolated-final-prune-actions-cache.log) |
| scripts/publish-release.test.mjs | 通过（exit 0） | 332 | [scripts-isolated-final-publish-release.log](scripts-isolated-final-publish-release.log) |
| scripts/release-policy.test.mjs | 通过（exit 0） | 272 | [scripts-isolated-final-release-policy.log](scripts-isolated-final-release-policy.log) |
| scripts/run-full-regression.test.mjs | 通过（exit 0） | 1912 | [scripts-isolated-final-run-full-regression.log](scripts-isolated-final-run-full-regression.log) |
| scripts/run-long-validation.test.mjs | 通过（exit 0） | 345 | [scripts-isolated-final-run-long-validation.log](scripts-isolated-final-run-long-validation.log) |
| scripts/run-web-shard-isolation.test.mjs | 通过（exit 0） | 255 | [scripts-isolated-final-run-web-shard-isolation.log](scripts-isolated-final-run-web-shard-isolation.log) |
| scripts/run-web-tests.test.mjs | 通过（exit 0） | 269 | [scripts-isolated-final-run-web-tests.log](scripts-isolated-final-run-web-tests.log) |
| scripts/run-with-deadline.test.mjs | 通过（exit 0） | 1163 | [scripts-isolated-final-run-with-deadline.log](scripts-isolated-final-run-with-deadline.log) |
| scripts/simulation/audit-diagnostic-divergence.test.mjs | 通过（exit 0） | 627 | [scripts-isolated-final-simulation-audit-diagnostic-divergence.log](scripts-isolated-final-simulation-audit-diagnostic-divergence.log) |
| scripts/simulation/baseline-run.test.mjs | 通过（exit 0） | 4800 | [scripts-isolated-final-simulation-baseline-run.log](scripts-isolated-final-simulation-baseline-run.log) |
| scripts/simulation/escrow-performance-harness.test.mjs | 通过（exit 0） | 719 | [scripts-isolated-final-simulation-escrow-performance-harness.log](scripts-isolated-final-simulation-escrow-performance-harness.log) |
| scripts/simulation/escrow-source-manifest.test.mjs | 通过（exit 0） | 269 | [scripts-isolated-final-simulation-escrow-source-manifest.log](scripts-isolated-final-simulation-escrow-source-manifest.log) |
| scripts/simulation/escrow-verification-contracts.test.mjs | 通过（exit 0） | 279 | [scripts-isolated-final-simulation-escrow-verification-contracts.log](scripts-isolated-final-simulation-escrow-verification-contracts.log) |
| scripts/simulation/prepare-escrow-performance.test.mjs | 通过（exit 0） | 825 | [scripts-isolated-final-simulation-prepare-escrow-performance.log](scripts-isolated-final-simulation-prepare-escrow-performance.log) |
| scripts/simulation/run-escrow-verification-matrix.test.mjs | 通过（exit 0） | 1200 | [scripts-isolated-final-simulation-run-escrow-verification-matrix.log](scripts-isolated-final-simulation-run-escrow-verification-matrix.log) |
| scripts/simulation/verify-simulation-artifacts.test.mjs | 通过（exit 0） | 2351 | [scripts-isolated-final-simulation-verify-simulation-artifacts.log](scripts-isolated-final-simulation-verify-simulation-artifacts.log) |
| scripts/smoke-deployment.test.mjs | 通过（exit 0） | 2465 | [scripts-isolated-final-smoke-deployment.log](scripts-isolated-final-smoke-deployment.log) |
| scripts/wasm-build-dependencies.test.mjs | 通过（exit 0） | 8662 | [scripts-isolated-final-wasm-build-dependencies.log](scripts-isolated-final-wasm-build-dependencies.log) |
