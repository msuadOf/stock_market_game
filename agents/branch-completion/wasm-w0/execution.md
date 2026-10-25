# W0 救回实施台账

- 基线：`f5820291`；旧增量：`1554b89a..0ac6bcc6`。独占分支 `feat/wasm-w0-recovery`。
- 46 文件清单见 `old-increment-files.txt`；排除全部旧 agents 正文与 asm comparator。现行 main 中 #117/#121 仅用 patch 叠加 clock/thread_facade 调用点。
- 设计依据：仓库根 `.tmp/wasm-thread-abstraction-design-input.md` 与 `.tmp/wasm-gate-r19-review-stdout.txt`；本次仅恢复 W0 基座，wasm runtime 保持 false，生产 Worker 接线留 W1。
- 红证：先移入旧测试，scripts 在缺少 `verifySharedMemoryArtifact`、combo 入口、nightly 参数及 CI 门时失败；5 个 Web 测试因目标模块不存在失败。见 `red-scripts.log` / `red-web.log`。Rust 门面探针因 clock.rs 缺失编译失败，见 `red-rust.log`。这些为移植必要性红证，不冒充旧历史提交的 TDD。
- 验证计划：Node 受外置10000ms deadline及case10000ms约束，显式8文件并发；Rust 先用 rustc 独立测试纯门面/slot模块，多线程执行、外置10000ms；实际engine编译与wasm产物构建按不可替代长构建分类，独占根 `.tmp/wasm-w0-recovery/target`，jobs=16，共享300000ms外置supervisor。禁止完整all回归。
- 内存裁定待落实现：设计 §6.3 的4GiB为工具链解阻塞参数；最终预算按 §7.5/§8.5/§17.1 为2GiB。先新增4GiB拒绝红测，再收紧配置和binary/glue门。initial64MiB生产装配留W1。
- 本批不改交易制度；native门面保持std re-export，业务规则、账户单位、撮合顺序与存档契约不变。最终须由未实施者独立审查完整diff。
