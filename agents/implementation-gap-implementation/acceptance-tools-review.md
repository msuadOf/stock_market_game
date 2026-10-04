# 验收工具独立复核

## 身份与范围

复核者：`/root/implement_acceptance_tools/review_acceptance`，未实施本组产品或测试改动，仅只读审查并编写本记录。

本次完整审查范围：

- `package.json`
- `scripts/run-with-deadline.mjs`、`scripts/run-with-deadline.test.mjs`
- `scripts/run-full-regression.mjs`、`scripts/run-full-regression.test.mjs`
- `scripts/performance/README.md`、`scripts/performance/market-ui-report.mjs`、`scripts/performance/market-ui-report.test.mjs`
- `scripts/simulation/escrow-performance-harness.mjs`、`scripts/simulation/escrow-performance-harness.test.mjs`
- `docs/testing.md`

另初步审查 G39/G72 的 `scripts/simulation/baseline-run.mjs`、`escrow-verification-contracts.mjs` 及其测试、`run-escrow-verification-matrix.mjs` 及其测试、`verify-simulation-artifacts.mjs` 及其测试，以及在途 Rust harness / `verification_evidence` 改动；不以本记录宣告该子组闭环。

## 有效发现与再次复核

1. `#onClose` 在非零退出后清理进程树，Windows 的已退出 PID 可能导致 `taskkill` 非零退出；原先 `#completeClose` 优先报告清理失败，覆盖执行退出码与编译器 stderr。实施者改为 `AggregateError` 同时保留执行和清理错误，新增 `terminateProcessTree` 注入点及 `taskkill exit 128` mock 断言。再次检查完整修改后确认发现已修复；不把 mock 当作 Windows 实机验证。
2. `docs/testing.md` 初版使用不存在的 `reproducibility` receipt 与 `raw.json` 名称。已修为真实的 `determinism.checkpoint.json`、`k7-determinism-receipt-v2`、`seed-<seed>.json` 和 `rerun.json`，再次复核名称与实现一致。

## 语义、必要性与边界

- G60 使用现有启动选择明确提交本地启动，不改变产品启动 policy。
- G61 针对 CDP 断连、sampler 拒绝和逐资源清理失败；聚合错误保留故障原因，正式性能命令增加进程外长验收 deadline，不引入依赖。
- G62 嵌套 supervisor 的 owned marker 避免主动创建逃离外部 deadline 的进程组；Linux 枚举后代并确认已观察 PID 不再 live；Windows 检查 `taskkill` 结果。未确认文案不冒充完整树终止成功。瞬时 reparent、所有平台和任意第三方进程树均不由短 fixture 证明。
- G63 完整/ignored case 清单取差集、逐 case `--exact` 独立进程及 10000ms watchdog，CPU 预算并发与失败时共享 abort 保留；ignored 长验收与 doctest 分类不变。没有用普通 case 的批次剩余 300000ms 放宽 case 上限。
- 以上工具改动不改变价格时间、撮合、费用单位、T+1 或资金股份规则，符合现行 A 股基线且范围必要；未发现其它有效必修问题。
- G39 删除自由 worker/rerun 整局字节相等符合 ADR-0017 的 2026-09-25 修订；立即保存/恢复等价仍保留，原始 rerun 证据独立绑定。G72 先检查 canonical containment 再读取内容，未冒充解决文件替换竞态。Rust 与跨层最终验证仍须另行闭环。

## 验证诚实性

复核者在受限 sandbox 执行三个 Node 文件的短验证，performance 文件通过；随后以 `--test-isolation=none` 执行 deadline/regression 共 52 case，43 通过、9 失败，其中包含 `spawnSync EPERM`、真实 child stdout 空等 sandbox 限制。没有将这些结果报告为完整通过，也没有将环境限制直接认定为实现缺陷。

实施者提供的非 sandbox 短验证结果：deadline/facade 22/22、performance/sampler 33/33，G63 定向 9/9；本复核仅确认其测试内容与门禁配置，不声称本人独立重跑了这些通过结果。未执行 Cargo 完整回归、真实浏览器性能矩阵或 macOS/Windows 实机验证。

## 分组结论

G60/G61/G62/G63 的完整 diff 和有效发现修复已完成独立复核，可以按该范围分组提交；Git 操作仍由获授权的实施者处理。

## G39/G72 终版补充复核

新增完整审查文件：`packages/engine/examples/escrow_verification_harness/committed.rs`、`packages/engine/examples/escrow_verification_harness/runtime.rs`、`packages/engine/src/verification_evidence.rs`、`packages/engine/src/verification_evidence/tests.rs`、`packages/engine/src/verification_evidence/phase_timing_tests.rs`，以及前述 G39/G72 JS 文件的最终完整 diff。

- Rust restored 分支使用当前 projector 的 clone，独立执行 `Accumulator.frame` 的 TickFrame、协议投影、收据、守恒及 finalizer 校验；原分支继续独立校验，不以删除 continuation 整局字节比较替代业务校验。
- runtime 发布 restored 分支的守恒证据；matrix 对新 capture 和复用共同检查两个独立命名的 restore slot、每 slot 恰一份 snapshot、来源与 tick 范围、资源守恒。新增非法 restored aggregate 短测明确拒绝，不放宽失败负控。
- rerun v2 保留两个原始输出的哈希绑定，并在 root 与 resume 路径重新核对来源配置；新增重签 hash 但来源非法的 rerun 拒绝测试。立即保存/恢复等价保持不变。
- G72 canonical containment 在读取 bytes 前检查，最小范围修复通过静态复核；不承诺文件替换竞态防护。

实施者提供最终 JS 四个独立 Node 进程并发结果：contracts 12/12、matrix 16/16、baseline 74/74、verifier 17/17，总计 119/119。最慢文件约 4.70 秒，配置 workspace TMPDIR、10000ms case timeout 与 GNU timeout 整批 10 秒进程树 deadline。复核者审查了最终测试及结果说明，但未声称本人独立重跑该通过结果。

此轮静态复核未发现新的有效必修问题，G72 可以闭环；当时 G39 代码与 JS 验证通过审查，但 Rust 编译/执行仍未完成。实施者报告共享 worktree 的在途 `company_groups` 模块解析及 `SessionSetup.groups` 迁移曾阻断编译，相关 owner 正在处理；没有把在途修复或 JS 通过提升为 Rust 验证通过。其后补齐的事实见下节，不改写本轮未完成的历史。

## G39 Rust 短验证最终复核

新增复核文件 `packages/engine/src/session/pipeline/event_key.rs`；再次完整审查 `verification_evidence.rs`、其 `tests.rs` 和两个 harness 文件的最终新增 diff。

真实 Rust 验证暴露原 validator 把全部事件身份域视为 0 基连续，与 ADR-0017 的 2026-09-26 事件身份接线说明不符：`Account/Sealed` 包含 Projection 下段及 P0 到期撤单顶部预留段。修复仅允许该域稀疏索引，复用 producer 的 `QUOTE_EXPIRY_EVENT_INDEX_BASE` 常量，不修改 producer、委托顺序或 P0 释放语义；JS 安全整数上限、非 `OrderCanceled` 不得占 P0 段、跨事件变体同域重 key 拒绝均保留。Stock、PriceTick、DayEnd、Session 的连续性不变。旧 gap 负控转为仍要求连续的 Stock/Trade 域，phase/entity/source 篡改负控没有删除，新增 Account 稀疏正例及碰撞、越界、P0 错误占用负控通过审查。

完整默认 fixture 的新增精准 test 曾超过普通十秒门禁，没有放宽期限；压缩三 tick 日仍带机构策略的尝试又暴露 near-close `parent_order`/checkpoint expiry invariant，不视为通过。该现象已通知 root，未在工具组盲修，也不以短 fixture 证明它已消失。

最终 `cfg(test)` 短 fixture 使用 history 1、三个 tick 的开盘/连续/收盘日、两个 retail 股份 donor、无机构/游资和零 retail arrival。它复用同一 capture 主体及两处 restore、协议 projector、frame、收据、守恒、finalizer 校验；正式 frozen setup 不变。独立复核发现仅缩小 NPC 可能令两个 restored 分支收据链均空；实施者已通过真实 `enqueue_player_intent` 预置不成交的 `600101` 买入挂单，并断言实际 restored journal 的收据总量大于零，覆盖收盘时真实 envelope 终结释放。再次复核确认不是篡改收据或只凭账户非空弱验证。

复核者独立执行两个复制二进制，每组分别受 `timeout -s KILL 10s` 进程外期限，均配置 `RAYON_NUM_THREADS=8`、`--test-threads=8`，没有把它们合为超过十秒的普通批次：

- `.tmp/process-tmp/g39-final/bin/escrow_verification_harness --exact runtime::tests::restored_continuations_validate_their_own_committed_receipt_chains --test-threads=8 --nocapture`：1/1 通过，case 8.84 秒，命令 9.44 秒，exit 0；包含两恢复点与真实非空恢复收据链。
- `.tmp/process-tmp/g39-final/bin/engine verification_evidence::tests:: --test-threads=8 --nocapture`：23/23 通过，case 总计 1.39 秒，命令 1.45 秒，exit 0；包含独立 continuation、稀疏身份正例和资金、T+1、收据链、身份、跨 update 状态负控。

最终新增完整 diff 与非空链发现修复再次复核通过，G39/G72 在本次实施及代表性短验证范围内可以核销。此结论不表示默认全 fixture、全 worker 矩阵、完整回归或此前发现的机构 near-close expiry 已通过；这些边界必须在实施汇报中继续保留。
