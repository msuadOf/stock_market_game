# Desktop Build Matrix

Use the matrix planner to inspect Tauri installer and bundle routes before packaging:

```sh
bash scripts/desktop/build-matrix.sh --dry-run --host linux --target all
```

The runtime host is detected automatically. `--host` is accepted only with `--dry-run` and simulates
planning, never a build. Use `--target linux`, `--target macos`, `--target windows`, or `--target all`.

The planner runs supported commands only when `--dry-run` is omitted and exactly one target is selected.
It runs from `apps/desktop/src-tauri`, whose Cargo package is `stock-market-game`; it does not assume a
global `tauri` executable and requires `cargo tauri` explicitly.

- Native Linux creates `deb`, `rpm`, and `appimage` bundles.
- Native macOS creates `app` and `dmg` bundles. Signing and notarization remain macOS-native workflows.
- Native Windows creates `msi` and `nsis` bundles. MSI needs Windows WiX/VBScript support.
- Linux/macOS to Windows is an experimental NSIS-only route using `cargo-xwin`; it cannot create MSI packages. The build can remain unsigned, but signed distribution requires an external custom `bundle.windows.signCommand` because Tauri's default Windows signer is unavailable on these hosts.
- Linux to macOS, Windows to macOS, Windows to Linux, and macOS to Linux are not officially documented Tauri v2 packaging paths.

Tauri runs the configured frontend build command, but the generated `apps/web/wasm-pkg` prerequisite is
not tracked. In normal mode the planner verifies both `web_wasm.js` and `web_wasm_bg.wasm`, Node
dependencies, Corepack availability, `cargo`, and `cargo tauri`; Tauri remains responsible for its configured frontend command. It verifies the constrained route's `cargo-xwin`, NSIS,
MSVC target, the LLVM `lld-link` driver, and `llvm-rc` before packaging. Tool versions, native Linux system
libraries, Windows WiX/VBScript, Xcode, signing, and notarization are intentionally deferred to Tauri/Cargo
and their platform tooling. A Rust `--target` compile is not itself an installer bundle and does not satisfy
platform signing requirements.
