# CI 启动与编译缓存边界

2026-10-02 修复 Windows Web 普通测试批次与 macOS Desktop 编译阶段的超时。
不放宽十秒普通测试门禁、五分钟阶段门禁，也不跳过测试或改交易断言。

- 完整回归直接在已有进程外十秒监督下启动 Web internal worker，独立 Web CLI
  保留自己的监督。只移除重复 Node 启动，不移除监督。
- 多核 Web 批次给 `app/workspace-grid.test.ts` 的 TypeScript 编译器与 SSR 验证
  独立分片，其他文件均分到剩余分片；仍受 CPU 预算和相同十秒总期限限制。
- macOS Desktop 库预编译阶段显式设置 `MACOSX_DEPLOYMENT_TARGET=10.13`，与当前
  固定 Tauri CLI 和未覆盖最低版本的配置一致，避免 CLI 阶段因环境变化重编依赖。
  不修改最终应用最低版本或发行者签名策略。该值不是对 ARM64 支持 macOS 10.13
  的承诺；制品仍是实际 runner 架构，兼容性由平台与编译工具链约束。

macOS 环境依据固定 Tauri `tauri-cli-v2.12.1` 官方
[build 实现](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.12.1/crates/tauri-cli/src/build.rs)
和 [macOS 配置默认值](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.12.1/crates/tauri-utils/src/config.rs)。
如修改 `bundle.macOS.minimumSystemVersion`，须同步预编译环境；对应契约短测会拒绝
与当前固定默认值不一致的配置。升级 CLI 时也须重新核对默认值。

本机沙箱可能使子进程管道输出捕获为空；同一批次在沙箱外、CI 固定 Node 24.18.0
下复核，不通过改断言、伪造输出或修改游戏代码掩盖环境限制。短测和静态审查只验证
局部契约，是否解决平台耗时必须以对应提交的真实 GitHub Actions 结果为准。
