# CFG / 可见性 / 参数候选一

- 固定基线 `50d493701aa0fbb7d62563855a84dc07c89c29b0`，独占树 `.worktree/clippy-cfg-2026-10-30`，分支 `codex/clippy-cfg-20261030`。
- 精确源码 manifest 为 candidate-1-source-manifest.json，完整可合并源码 patch 为 candidate-1-source.patch，含唯一新增 worker_context.rs。工作材料不混入源码 patch。
- 合并前分别完整审查 fixture / 精确 cfg / company / args-tests / args-leaf / settlement / diagnostic / stream 净 diff；有效发现修后复核，详见 combined-independent-reviews.md 与各独审记录。主树实施的参数/可见性及合并后的完整树仍由上级未实施者审查。
- 覆盖原 private 接口链（避开性能在途 runtime_mirror/progress 两 getter）、3 个 test fixture 去重、18 处原 too_many_arguments 对应签名、9 处父派 must_use/bool 诊断及能证明的测试专属旧入口。没有宣称原 266 个 dead_code 已全修。
- source 改动不增加依赖，不改存档/JSON，不改变分/股单位、T+1、整手、撮合或受理顺序。内部参数命名分组保原引用/owned 边界与验证顺序；stock_task 明确保 sender/Arc 借用、计时绑定释放后才发送、完成通知后才 drop(spawn_slot)。
- 独审曾拦截 settlement 多 clone/双 source、Option 外层 ! 漏删、stream reference 改 owned；错误初版未进入本候选。修正版同源 reborrow、三值真值表和引用合同已重新核对。
- 精确 cfg 先查完整 engine/src、engine/tests、engine/examples、apps Rust consumers；不把 integration tests 当 lib 的 cfg(test)。目前新增 cfg(test) 均是 crate 内窄入口或测试模块，未扩对外能力。Simple 自动提案依 root 的当前正式边界门控计算原型，config/ledger/ex_dates 保生产。
- 静态：82 个变更 Rust 文件由 8 workers 指定 rustfmt（edition 2021、skip_children=true）在 10s 期限内完成，git diff --check 通过。没有任何 Cargo、编译或测试启动。

## 建议唯一集成窗口 argv

由上级与唯一 integration 指定 CARGO_TARGET_DIR（本候选未读写任何 target）；j8 / INC0 / DEBUG0。先合并上级其他设计与机械净片，再一批实际验证，不为每个叶重复排票：

`<仓库根目录>/.tmp/node24-env/node scripts/run-long-validation.mjs 300000 -- cargo check --workspace --all-targets --all-features --locked -j 8`

编译成功后使用相同门禁实际 `cargo clippy --workspace --all-targets --all-features --locked -j 8 -- -D warnings`，保完整 stdout/stderr/CPU 与实际退出及进程树清场。支持 native/wasm 仍须以项目既有 production feature/target 矩阵实际验证，不把 native check 当作 wasm 通过。

状态：READY 供独审与集成验证；不是 Clippy 绿灯或任务完成声明。
