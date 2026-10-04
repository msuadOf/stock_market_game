# 验收工具缺口实施记录

## 范围与依据

本批处理 G39/G60/G61/G62/G63/G72。已读根 AGENTS、principles、testing、architecture、open-questions、ADR-0017/0018 的现行并发受理边界及 ADR-0027 的启动选择约定，核对总账及各项审计证据。不修改生产交易顺序、价格、A 股单位或 T+1，不引入依赖，不修改发布 CI，也不启动完整回归、浏览器性能矩阵或发布。

## 实施

- G39：自由 worker/扰动/rerun 不要求整局字节相等，立即保存/恢复等价保留；每次 capture 保留收据重放、守恒、业务覆盖及失败负控。K7 receipt v2 新增保存和绑定 rerun 原始证据；复用与 root verifier 独立验证两次输出。Rust harness 的 restored 分支用当前 projector 的独立副本校验实际 frame、收据守恒和 finalizer，并发布 `restored_conservation`；matrix 校验两个不同恢复点、来源 tick 范围和恢复分支守恒。生产 verification helper 同步允许两个独立 continuation receipt 不同，不仅删除 JS 门禁。跨层实施及静态复核完成，但 Rust 编译/行为验证未完成。
- G60：性能工具通过既有启动选择，明确选择本地并提交启动操作，已进入游戏时不重复启动。
- G61：正式性能命令加入进程外 300000ms supervisor；UI/CDP 异常显式拒绝、资源逐项清理并汇总失败；sampler 拒绝立即终止 owned tree 并等待 child close。
- G62：嵌套 owned supervisor 继承同一 POSIX 进程组；终止前枚举 descendant，Linux 用 `/proc` 核对已观察后代不再 live；Windows 等待 taskkill 退出并检查结果。hard cutoff 明确区分请求与未确认终止，不再仅凭 direct child close 宣称完整树已终止。清理失败与执行 exit/stderr 用 AggregateError 同时保留。进程树快照不能保证捕捉任意瞬间 reparent 的第三方进程，macOS/Windows 未实跑。
- G63：普通 prebuilt Rust case 从完整/ignored 清单差集获得，逐 case 独立 `--exact` 进程外 10000ms watchdog，按 CPU 预算真实多进程并行，共享原 300000ms 执行预算。ignored 必要长 case 和 doctest 阶段分类不变。
- G72：matrix artifact 的 canonical containment 检查移至读取 bytes 前，新结果和 PASS 复用共同消费。没有扩展为文件替换竞态防护。

## TDD 与短验证

- G62 新增 nested detached、未确认文案、清理失败保留执行错误的断言均先红后绿；真实阻塞 event loop + detached 孙进程短 fixture 验证终止。最终 deadline/facade 两文件分别用两个独立 Node 进程并行运行，22/22 通过，约 1.86 秒；case timeout 10000ms，GNU timeout 整批命令 10 秒。
- G60/G61 完整两文件 33/33 通过，约 0.62 秒，case 和命令 10000ms、并发参数 2；包含真实 sampler 错误后 child 已退出。workspace TMPDIR 必须配置，最初未配置产生的 fixture 路径错误不当作生产问题。
- G63 新增 case 先红后绿，定向 9/9 通过并由独立 reviewer 重跑；排除四个本环境 subprocess-sensitive 既有 case 后 31/31 通过。首次完整文件尝试被 10 秒 supervisor 终止，不宣称全文件通过；没有运行真正 Cargo 全回归。
- G39/G72 最终 JS 四文件独立进程并发 4：contracts 12/12、matrix 16/16、baseline 74/74、verifier 17/17，共 119/119 通过；最长文件 baseline 4.69 秒，case10000ms + GNU `timeout -s KILL 10s` 整批门禁。先前一轮并发批次被 10 秒截止，拆 baseline 后绿，再重跑最终整批 exit0，不把先前超时算作通过。
- Rust 定向编译尝试一为 `cargo test -p engine --features verification-harness --example escrow_verification_harness restored_continuations_validate_their_own_committed_receipt_chains --no-run -j8`，遇共享 worktree 在途 `session/disclosures.rs:51/159` 的 `super::company_groups` E0433，以及 `session.rs:2119` 的 DayEndDisclosureCtx 缺 groups。尝试二为 `cargo test -p engine --features verification-harness --lib observation_rejects_restore_byte_mismatch_but_allows_independent_continuations --no-run -j8`，遇 SessionSetup/SessionState 新字段缺失与解构不完整（含 `session.rs:3065/3371`）。两次均使用 workspace `.tmp/build-cache/full-regression` 与外部 10 秒短期限；编译错误不是目标行为红灯，未声称 Rust 测试通过。整合稳定后需要定向编译和短执行这两个目标。
- 默认 Node 文件隔离在本环境产生整文件 worker 失败，采用 `--test-isolation=none` 并在需要时以独立 Node 进程并行文件；真实 spawn/进程观察用 require_escalated 运行，初次沙箱 EPERM/stdout 空失败未伪报通过。
- `git diff --check` 通过。无 stage、commit、完整矩阵、真实浏览器旅程或对外发布。

## 独立复核

非实施 reviewer `/root/implement_acceptance_tools/review_acceptance` 审完整工具 diff，确认现行 A 股与并发受理语义、必要性和范围，发现 Windows 清理失败覆盖原 exit/stderr：已修为 AggregateError 并补红绿 mock，再次复核通过。正式 testing 文档中初版 receipt 名称错误也已按 reviewer 修为真实 schema/路径。G63 子组另有非作者 reviewer 独立重跑 9/9 并通过。G39/G72 扩展 Rust/JS 完整 diff 另经 `/root/implement_acceptance_tools/simulation_contract/review_contract` 和主组 reviewer 独立静态复核，未发现必修问题；主组复核记录见 `acceptance-tools-review.md`。G60/G61/G62/G63/G72 已在实施范围闭环；G39 仍明确保留 Rust 定向编译/短行为验证，不宣称完成验收。
