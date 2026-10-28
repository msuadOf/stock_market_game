# 真实 completed Correction SaveSlot

`rust-completed-save.json` 是 E2 真实共同窗口的完整原始 SaveSlot，356888 bytes，SHA-256 为 `92127f8a150787c3d9ec64f44211117b1f1435027ed643bfa779d908dd35b6bd`，逐字节复制，未改字段、数字 token、键序或换行。

来源是 E2 独占树的 `agents/perf-intraday-independent/implementation/e2/correction-production-green-01/actual-completed-save.json`。真实 producer 为同一新 unit 的 `session::company_corrections_tests::correction_day_end_completes_at_real_eighteen_and_restores_exact_identity`，product-010 完整通过后才提取唯一 marker；unit SHA 为 `f897ce27f4a1e457a26a0b5012007dc6cdc093edace2dd4610445e98bace6af7`，source 为 E2 冻结 961 项。原窗口 session95868 实际 WAIT1，约214.1695s，虽然两 build 与最终23个 Rust case 通过，但 Web strict consumer 在 closing.restatements source 字段失败，**整个原窗口永久失败，未执行 Rust Web 回读**。

本原件的非空完成操作为 `session-correction-1`；公司0的 `finance.closing.restatements` 含 BusinessEventId 裸数 `900000001`。真实 Rust `RestatementRows=Vec<(ScopeId,Vec<(BusinessEventId,AccountingPeriod)>)>`、透明 u64 BusinessEventId 不带字符串 codec；Web 使用安全整数 number 共同域，超 MAX_SAFE 显式拒绝，不能宣称 Rust 完整 u64 与 JS 同域。会计期间仍为 YYYY-MM，未改变任何元/分金额。

本次只修 Web 原有纯 codec 对这个 source 的错误字符串假设，保原完整存档全值相等，不为兼容错误字段把 number 转成 string。TDD 与后续独审/受控回读记录见 [任务证据](../../../../../../agents/branch-completion/source-b-web-strict-save/correction-save-web/real-wire-v2/README.md)。本 fixture 的正常 Web 消费不独自证明同 binary Rust reader 已执行；后者须新窗口独立记录。
