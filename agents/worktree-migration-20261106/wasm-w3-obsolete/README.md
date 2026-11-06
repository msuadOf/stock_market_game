# WASM W3 存储实验归档

本目录保存历史工作树 `wasm-w3-storage-2026-10-28` 的未提交实验，源 HEAD 为 `6287f479b9c0691a6bd6addd2ef9e48c8f1a5b92`。`tracked.patch` 是相对该 HEAD 的完整 `git diff --binary HEAD`；`untracked/` 保留四个未跟踪测试文件的原始路径和内容。补丁与测试均为历史材料，不接入编译或测试。

该实验的初始存档恢复方案已由现行代码替代：基线 `1a1772299c15878144f798b4ee62d931b3d62b53` 的 `wasm-session-slot.ts`、`wasm-worker.ts` 和 `worker-host.ts` 已实现不同的初始恢复流程，且 `wasm-worker-ownership.test.ts` 覆盖同源 topology 与首次恢复。旧实验中阻止初始存档的 topology gate 不代表现行行为。归档仅保留历史证据，旧断言存在不等于测试通过，也不代表当前功能已通过验收。

SHA-256 清单记录原始补丁和四个测试文件，用于核对内容；补丁中原有空白作为历史文本保留。该归档不改变 A 股交易规则或运行时行为。
