# Web AccountId 当前字符串契约接线

## 范围与授权

- Root 确认全域 `AccountId` 使用规范 u64 十进制字符串，无旧数字兼容路径。
- 本批仅修改 Web Save schema、所属短测与 NPC diagnostics/query/UI；不修改 generated、`host/protocol` 或 trade tape。
- 交易数量、金额、订单 ID、seq、plan ID 与 FIFO／T+1 规则不变。公司会计科目的 `account` 是科目代码，不是投资者 `AccountId`，不改。
- `pending_npc.observed_accounts` 原有非零边界保留；Player 账户 0 不能冒充 NPC。

## 实施与验证

- `accountId` 严格拒绝数字 JSON、前导零、负数与 u64 溢出；账户 map key 同步支持 MAX u64。
- Save snapshot、Order owner、Receipt owner、个人 Information owner、Belief npc、TradingPlan account、Runtime envelope account 使用同一规则；plan key 保持原 safe integer 边界。
- NPC diagnostics 使用 string；Worker query 转为 `BigInt(account)`，不经 `Number`；Remote、Tauri 与 Inspector 保持字符串。
- 真实 Node 红测：新三个 case 实际运行 3 项，失败 3 项，证明旧 owner/mapkey/diagnostics 数字路径不满足 MAX 契约。
- 红后同三项绿色，追加 Runtime envelope／NPC queue／order number 边界短测。
- 最终定向命令：外部 `run-with-deadline.mjs 10000`，Node case timeout 10000，file concurrency 8；实际 101 项，101 passed，0 failed，0.57 秒。
- 定向 `oxlint --deny-warnings` 通过。
- 扩展 Save corpus 曾实跑 159 项，126 passed、33 failed；多数来自既存公司 schema 与陈旧 current JSON/save-file fixtures，本批没有弱化断言。相关 Runtime confirmations 缺失与 NPC 数字负控已修复。
- `fixtures/current-schema-save.json` 单行约 8.8 MB，本批未手改生成数据，需 Root 使用正规 generator 重产当前格式。TypeScript 在 generated AccountId 仍为 number 时不能宣称通过。

## 独立复核状态

已通知上级安排未实施本批的 subagent 审查完整 diff；本记录不宣称独立复核通过或整批交付完成。

## MarketMembership 与 WASM Restore 续批

- Root 追加授权 Web `market_memberships` 必填 strict schema 与账户集合验证；完整读取 Rust `session/memberships.rs` 后对齐现行契约。
- `OpaqueSubjectId` 非空且不含 C0/C1 控制字符；不 trim、不禁止空格、不添加长度上限，`__proto__`／`constructor` 是合法主体键。
- members/member/AdmissionFunding 层逐层 exact，所有 caps／unknown／缺字段拒绝；external_cash 使用非负 Money string，不追证当前现金是否仍等于入场资金。
- member account 唯一、不能占 NPC 1..N、账户 0 必须有成员关系；Snapshot 账户集合恰为 members 加 NPC。使用 BigInt 集合范围与基数核对，不为恶意巨大 NPC count 构造长循环。
- 最小 currentSaveFixture 显式增加 market_memberships、NPC 1 账户与其 history_reads；不为旧档注入 local-owner fallback。real JSON/generated 仍由 Root 正规 producer 输出。
- WASM Restore AccountId map key 保留规范字符串与 MAX u64；PlanId 不是 AccountId，继续使用经过 safeIntegerKey 验证的 Number Map key。
- normalizeSerdeMaps 用 Object.fromEntries 保留合法原型同名主体键；不改变普通 bigint seq 的原 safe number 校验。
- 真实红：membership 初始 3/3 失败，新增原型同名主体边界 1 项失败；WASM MAX 1 项失败；WASM 原型同名主体 normalization 1 项失败。各自均先运行 Node red，再实施。
- 最终综合定向：case timeout 10000、外部进程树 deadline 10000、file concurrency 8；116 passed、0 failed，0.61 秒。定向 lint 通过。
- 同轮 TypeScript 输出中 Save／serde 无错误；其他 owner 的旧 generated removed:number[] 与 PublicMetadata 测试错误交由上级，不把该轮称为整仓 tsc 通过。
- 源码冻结，等待 Root 整批独立审查。
