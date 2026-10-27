# 公司六项只读查询的分层交接

可信 base 为 `8e5b32de305257b7cb9aeb284d19551cdf3ce2c3`，最终产品来源提交为 `01d0ac7eb28ef11a0eb92a763e446bfc1cc6def4`，十四项产品逐文件 SHA 见 [v3 freeze](green-candidate-v3/product-source.json)。任务仅改 Rust 共同 protocol 与三宿主注册；未改六项既有 `GameSession` 领域方法、交易制度、税算法、生产结算或 TS。最终证据提交不新增产品 delta。

| 输入层 | 产品路径 | 实际验证与输出 |
|---|---|---|
| engine protocol | `session/protocol.rs`、`protocol/company_reads.rs`、其 `tests.rs` | 健康／实际 owner／CompanyCatalog 身份、六项原 DTO、全局 outstanding 按 owner 过滤、非空税账／receipt／entitlement、save 字节不变；11 exact 与 `wire-engine.json` |
| Server actor／HTTP | `apps/server/src/actor.rs`、`actor/fatal_tests.rs`、`routes.rs`、`routes/auth_tests.rs`、`lib.rs` | 当前非零 membership 与 generation、与 engine 本人隔离差异正控组合验证、真实 HTTP 鉴权及注册；3 exact 与 `wire-server.json` |
| Tauri actor／IPC | `apps/desktop/src-tauri/src/actor.rs`、`actor/protocol_tests.rs`、`lib.rs`、`lib_tests.rs` | 本地 owner 0、generation、typed 领域失败、真实 mock invoke 注册；3 exact 与 `wire-tauri.json`／`wire-tauri-failures.json` |
| WASM registry | `apps/web-wasm/src/lib.rs`、`protocol_tests.rs` | 真实 native session registry、owner、save 不变；1 exact；不含真实 JS export 或浏览器 |

四包编译共享 1001 项真实输入、8 项 runner，来源始末 hash 完全一致；这不是 1001 项产品改动。一次 Cargo 命令统一解析 features，engine unit 合法带 `verification-harness`，不外推为真实 wasm32 图。实际四 unit 的 path／完整 SHA／bytes／features／profile／fresh／独立 seal 见 [artifact binding](four-host-green-02/artifact-binding.json)，每例 argv／child exit／binary SHA 见 [batch receipt](four-host-green-02/batch-receipt.json)，实际 wait／资源／清理与四份 wire hash 见 [actual wait](four-host-green-02/actual-wait-cleanup.json)。

六项 kind 为 `CompanyPeriodExplanation`、`AccountDividendTaxStatus`、`DividendTaxOutstandingViews`、`OwnerFlatWithholdingReceipts`、`CompanyCapabilities`、`OwnerRightsOfferings`。Server／Tauri 成功为 `{generation:string,owner:AccountId,value:{kind,data}}`；engine／WASM 没有 generation 字段。AccountId 为规范十进制字符串，CompanyId 依真实目录，不能当作 StockId；证券沿既有 StockCode，Money／AccountingAmount 不换单位。请求无 client owner／account，严格拒绝未知字段；Server actor 按当前认证 subject 的 membership 导出 owner，六项新增接口均要求 member，旧 public report 权限保持原契约。

HTTP 为 `POST /api/company-read-query`，body 包含 `session_id`、`generation`、`query`，沿既有 credential 认证。业务拒绝为 `{code:"COMPANY_READ_REJECTED",failure:CompanyReadFailure}`。IPC 名为 `company_read_query`，参数为 `sessionId`、`generation`、`query`；Session 错误保留具体 message，Query 错误保留 typed 公开失败。WASM 导出同名，当前实际验收仅 native registry。精确字段应以实际 fixture 与源码为准，不从说明反向手写 fixture。

四份实际 JSON 已通过 followup 交 `/root/sol_bc_ram_execution_28` 继续消费验证。Cargo 时段在 session `57868` 实际退出及清理后立即交 root 默认后继 R4red03；不重跑、不保留占用。main 接收由 root 等待 E2 公共 base 出口后安排，复用本分支实际证据，不另造全宿主回归矩阵。首红九个业务红／一个严格输入绿及 green01 的真实 E0308 失败原件均保留，不能由后续成功覆盖。
