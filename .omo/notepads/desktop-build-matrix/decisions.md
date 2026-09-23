# Decisions

## 2026-09-13

- Added one dependency-free Node planner behind small POSIX shell and Windows batch launchers so host detection, matrix classification, output, and normal-mode preflight are identical across host systems.
- `--host` is dry-run-only: it simulates a host for deterministic matrix inspection and cannot alter a real package build host.
- Native commands run inside `apps/desktop/src-tauri` rather than pretending a global Tauri executable or a Tauri `--package` flag exists. The planner requires `cargo tauri` and labels the selected package as `stock-market-game`.
- Final-review remediation stores fixed executable/argument vectors in each supported plan and derives display text from them; no normal build parses a display command string. NSIS uses `/VERSION`, while LLVM tool availability probes intentionally use no arguments because valid `lld` and `llvm-rc` variants do not uniformly support `--version`.
- 2026-09-13 remediation: constrained Linux/macOS to Windows preflight uses POSIX NSIS `makensis -VERSION`; the simulated `--host` remains dry-run-only, so normal mode always uses the actual Node runtime platform.
- 2026-09-13 clarification: the preceding `/VERSION` wording records the superseded implementation; the enforced contract and regression fixtures are `-VERSION` for POSIX hosts.
- 2026-09-13 Windows shim decision: normal preflight probes Corepack itself (`corepack --version`); on Windows it invokes the fixed `ComSpec /d /s /c "corepack --version"` vector. It no longer discovers or interpolates arbitrary `.cmd` paths, and Tauri owns the configured frontend command.
