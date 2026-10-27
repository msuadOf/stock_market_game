# 公司查询真实 wire fixture

四份 JSON 均为实际 Rust native 测试输出的逐字副本；测试按固定 SHA-256 校验，不在测试中改写原件。完整来源、stdout 对照、构建与运行绑定见 [真实 wire 消费证据](../../../../../../agents/branch-completion/company-web-f5-read-2026-10-28/real-wire-v1/README.md)。

`wire-engine.json` 合并独立 domain fixture 的结果，不能解释成同局生产结算；`wire-server.json` 和 `wire-tauri.json` 各为同局六种只读响应。`wire-tauri-failures.json` 为真实未结算期间拒绝。受控 TypeScript transport 消费不等于 WASM JavaScript export 或 browser 端到端验证。
