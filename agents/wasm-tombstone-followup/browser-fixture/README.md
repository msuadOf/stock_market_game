# release WASM 浏览器验证 fixture

本 fixture 使用 `verification-harness` 专用的真实 spawn、join 和 panic 入口，不进入正式导出。需要本地 Vite、Chromium、固定 nightly 工具链及受控 sysroot；浏览器须支持共享 memory 和 COOP/COEP。

先设置一个不存在的 `WASM_PANIC_SYSROOT` 绝对路径，再创建受控 sysroot：

```sh
node --input-type=module -e 'import { preparePinnedSysroot } from "./scripts/wasm-panic/prepare-sysroot.mjs"; await preparePinnedSysroot(process.argv[1], process.argv[2]);' "$(rustup which --toolchain nightly-2026-09-05 rustc)" "$WASM_PANIC_SYSROOT"
```

另须显式设置独立的 `CARGO_TARGET_DIR`、`WASM_PKG` 和 `CARGO_BUILD_JOBS`。从仓库根目录使用进程外 supervisor 以 300000ms 上限运行 `batch.sh`，并为终止和清理保留时间。批次比较受测源码的运行前后哈希，源码变化时必须重新构建与验证。

child 场景检查 death futex、join 死亡信号、记录和 hard trap 缺席；primary 场景检查 engine、ingress 记录和身份。owner 场景检查真实浏览器 Worker 的错误经 owner 进入故障处理与终止。fixture 不覆盖 `createWorkerHost` 到 UI 的完整旅程，也不代替全仓验收。
