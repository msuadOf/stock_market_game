# WASM capacity 与税务测试历史归档

本目录保存 2026-11-06 从旧 worktree 提取的未提交材料，基于 main `5ddf9469` 归档。材料仅供追溯，未经本轮测试；其历史实现和测试假设已被 main 后续契约取代，不能作为现行行为或 A 股规则的依据，也不直接接入编译或测试。现行容量范围和税务模式分别以 ADR-0019、ADR-0040 及 `docs/trading-rules.md` 为准。归档不改变交易语义。

## 来源与 SHA-256

- `.worktree/wasm-memory-capacity-2026-10-29`，HEAD `0b368b1a3909a5e821b16d3fc81b5f04d0d36337`：
  - `apps/web/src/host/facade-wasm-module.test.ts` 和 `facade-wasm-module.ts` 的原有改动已核对与 main 文件字节一致并核销，当前 worktree 的 tracked diff 为空，因此不归档补丁。
  - `wasm/wasm-memory-capacity.ts`：原路径 `apps/web/src/host/wasm-memory-capacity.ts`，未跟踪文件。SHA-256 `7c33e7e49bebb551cecddcf090d48557ddb9dea69e75ecaef68b83e12fd5d28a`。
  - `wasm/wasm-memory-capacity.test.ts`：原路径 `apps/web/src/host/wasm-memory-capacity.test.ts`，未跟踪文件。SHA-256 `67f0a9412478f78bace59752114c4a22fd26cffa17c46078563c7bcde59ddfc2`。
- `.worktree/tax-stage-raw-leaf-2026-10-29`，HEAD `e78cb9e81da7070afccd4c43bb59e796066e8185`：
  - `tax/stage_information_tests.rs`：原路径 `packages/engine/src/runtime/world/admin_transaction/stage_information_tests.rs`，未跟踪文件。SHA-256 `f14929846f8f270c13fa8362dd606506a1445912f5e04a9549189aa7a148169b`。
  - `tax/payment_runtime_tests.rs`：原路径 `packages/engine/src/session/ordinary_tax_day_end/payment_runtime_tests.rs`，未跟踪文件。SHA-256 `1e8ee73762adde2003c31011f48aec3b1a7858676701920ce57a03fbd45fb872`。
