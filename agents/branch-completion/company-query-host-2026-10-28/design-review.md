# 六项公司只读查询的边界裁决（2026-10-28）

本记录来自未实施者 `/root/sol_company_queries_28/company_query_boundary_review` 的只读源码审查。该 agent 由原生工具显式创建为 `gpt-6.1-sol/high`。此处仅登记设计结论，不构成完整 diff 或实际运行验收通过。

## 最小范围

- `GameSession` 的既有六项 query 为唯一领域实现；`ProtocolSession` 加只读 dispatch，三宿主只做注册、当前身份、generation 与传输。
- request 不允许选择 account／owner；company 使用本局实际 `CompanyCatalogEntry.id`，不把 `StockId` 或代码数字值当作 `CompanyId`。
- 六项结果保留既有类型和 serde，包装为 `{kind,data}`。成功外壳包含真实 `owner`；Server／Tauri 另包含 `generation`。`AccountId` 已使用规范 u64 十进制字符串，不能依据 memberships 的旧 number 注释改成数值。
- Server 在 actor 内先检查当前 generation，再由 `market_membership(subject)` 解析当前账户。无成员拒绝，不为查询加入或回退到 0。Tauri／WASM 延用单机 player 0。

## 错误公开面

- 内部保留 `CompanyReadError` 的 typed source。`CompanyReadFailure::Company` 需包含原分类与具体 `reason`。当前期间解释错误为公司、周期末、未结算、前史范围和解释缺失，可保留该公开域错误文本。
- 普通 `CorporateState` 需包含具体 `reason`。当前税务上下文、Flat 模式不符、公开配售额度等错误不能被统一的空分类吞掉。`RegistryPositionMismatch` 包含其他账户持仓，必须安全投影。
- `dividend_tax_outstanding_views` 先计算全局税账再过滤成功结果。原错误会包含失败税账的账户和证券，不能统一公开 `to_string()`。该分支单独保留内部 source，公开说明全局汇总失败、无法生成本人结果、明细已保留诊断及反馈位置；不解析／删除错误字符串、不忽略失败后交付部分成功。

## 三问结论

1. A 股语义：不改税率、舍入、分／股／bp 单位、配股窗口或支持能力；复用已登记领域事实与 ADR-0040／0041。本批不新增交易规则，未重新联网核验官方规则，不声称完成生产结算。
2. 必要性：真实 owner 外壳对空列表与不含 account 的权益／税籍结果也必要；安全具体 reason 是显式错误要求。该范围不需要新增数字 fault 或税算法。
3. 边界：必须验证非零成员、旧 generation、伪造 owner、无成员、未知公司、空列表、失败原因、全局错误不泄漏其他账户、六项查询存档字节不变。晚到响应由 TS owner 按真实 scope 处理，响应 owner 不得重写认证关系。

## 证据限制

窄审依据为 `company_read_views.rs`、`company_capability_views.rs`、`corporate_actions.rs`、`company/system.rs`、`orderbook.rs` 及当前 stub 的实际控制流。审查者未运行 Cargo，未构造损坏税账运行例；该限制保留。完整实现后需再次交未实施者审完整 diff 与红绿原件。

后续同一未实施者补核：typed source 应保留到最后可信 Rust 边界；Server 可由既有 tracing 记录后投影，Tauri 在 invoke 边界投影，WASM 在 export 边界投影。不新增 last-error 存储、console raw 或日志依赖。WASM 返回后不保留原始 source，因此公开 reason 不能承诺“已保存诊断记录”；应说明错误明细可能涉及其他账户、公开回复不含这些明细，并请用户连同查询类型与会话信息反馈。本节修正上文示例中“保留诊断”的持久化歧义；内部传播保有 typed source 不等于返回后仍可检索。
