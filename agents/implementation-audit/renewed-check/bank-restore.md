# Q19 银行存档恢复复核

## 范围与结论

- 目标为审计 worktree `HEAD c0ab429`；本记录只复核 Q19 的降级判断，不重扫其历史来源、不修改产品代码或 G/Q 总账。
- 指定的 `docs/company-accounting.md`、`agents/implementation-audit/exhaustive-review/luna52.md`、`agents/implementation-audit/hidden-review/batch-155.md` 均从头连续读至 EOF。随后按现行源码追踪 `SaveSlot` → `decode_save_slot` → `validate_save_slot` → `GameSession::restore`。
- **Q19 不能以“完整会话恢复尚未开放银行”作为无当前恢复可达性的依据。** 新局装配仍固定为 `Industrial`（`session/company_assembly.rs:331-346`），但 `SaveSlot` 必填保存 `CompanyOperations`，其 serde 状态含四种 `IndustryBooks` 变体；`restore` 校验后将已保存的经营集合整体安装。对照现行验证范围，确证可由完整 SaveSlot 解码并恢复的 `BankBooks` 可带无效 `EclPolicy`，且 `validate_company_domain` 不调用银行/ECL validator。现有模块内测试也直接证明空 `stage1_default` 可 serde 恢复、`validate()` 报错且初始贷款仍可发放（`behavior_tests.rs:228-249`）。
- 因而应把 Q19 的当前状态精确更新为：**经完整存档恢复入口可达的行业账套政策验证缺口**，但不泛化为默认新局已提供银行功能，也不声称合法编辑后的账面资产必须与某段历史交易相符。建议在正式 SaveSlot 路径中对恢复的 `BankBooks.ecl_policy` 调 `validate()`，并映射为带上下文的 `InvalidSave`；这与当前授权只读审计无关，不在本次实施。

## 恢复链证据

1. `SaveSlot.company_operations` 是权威存档字段，文档注释明确经营编排、行业账套均全量 serde 持久化（`session.rs:460-465`）；`GameSession::save` 将当前 operations 克隆入槽（`session.rs:2657-2660`）。
2. `decode_save_slot` 在字节上限与 schema header 检查后只做 serde 解码（`persistence.rs:963-974`）。四种行业变体在 serde enum 中明列，包括 `Bank`（`company/operations/config.rs:18-23`）；`BankBooks` 对 `ecl_policy` 直接 derive `Deserialize`（`company/bank/mod.rs:94-104`）。
3. `validate_save_slot` 的确会调用 `validate_company_domain`（`persistence.rs:936`）。该领域校验检查公司到 setup 股票的唯一映射/股本、经营日期、scheduler 镜像与到期队列、公共信息库（`persistence.rs:976-1044`）；没有检查 `CompanySpec.kind` 与 books/params 的对应关系，也没有调用 BankBooks/EclPolicy validator。
4. `GameSession::restore` 在 `validate_save_slot` 成功后新建 session（`session.rs:2716-2721`），仅核对重建 issuer ID 集合与保存的公司 ID 集合（`:2933-2941`），之后直接将 `save.company_operations` 克隆到恢复 session（`:2955-2959`）。这条赋值消费保存的行业 variant，而不是保留 `GameSession::new` 的默认账套。
5. `IndustryPairView::at_build_guard` 有 spec-kind/books-kind 检查，但这是新建装配守卫；存档恢复不调用它（`company/operations/config.rs:106-113`）。恢复后日推进的现有 guard 对 books/params 做配对匹配（`:116-132`），也不等同于 ECL 政策恢复校验。
6. 可达的输入不要求伪造 `GameSession::new` 能生成银行默认局：`SaveSlot` 是公开 serde 数据结构，decode 接受结构完整的 `CompanyOperations`；将合法构造的 Bank 公司序列化账套和配套 `Bank` flow/spec 放入存档，并保持已校验的公司 ID、挂牌股票/股本、日期和队列约束即可走同一 restore 链。再把 ECL 情景表编辑为空，结构仍可解码，但政策 validator 拒绝该业务状态。此结论不把合法编辑的持仓、分录余额或其他允许编辑的资产值认作非法；缺陷针对有明确量化契约且应由 validator 拒绝的 ECL 政策。

## 总账去重与反证

- **G36** 仍准确描述默认装配只建 Industrial、非工商 session/封账/披露/行业适用性闭环缺口。它不反证 restore 对携带 Bank variant 的 SaveSlot 可达；Q19 只聚焦 ECL policy 恢复 validator，不重复登记 G36 的生产闭环范围。
- **G73** 是保险 `ContractGroupState` 恢复后子账不变量校验，不覆盖 Bank ECL 政策；不应因存在其他行业恢复项而并项或关闭 Q19。
- **G79** 是 Industrial 授信计算错误折叠及贷款状态恢复校验，不覆盖银行 ECL 情景表的非空/权重和规则。
- 历史审计中“默认新局固定 Industrial”仍属真；但从该事实推出“BankBooks 不可通过现行完整 SaveSlot restore”不成立。反向也不应夸大：不主张普通默认游戏旅程可自然生成银行账套或已实现完整银行会话产品闭环。
- `EclPolicy::validate` 明确要求 stage1/lifetime 两表非空、单项有效且各自权重合计 `10000bp`（`ecl.rs:87-93`）；创建路径 `BankBooks::new` 调用它（`bank/mod.rs:106-112`），反映的是现行游戏政策契约。本审计未重新联网核验官方准则，不新增交易规则主张。

## 证据限制

- 本轮只读静态审计；未运行测试、构建或浏览器验证。上述具体“完整 SaveSlot 含 Bank 变体并恢复成功”的拼装链是由当前类型、字段、validator 与 restore consumer 静态确认的可达性推论；未执行动态端到端复现。
- 所读历史文档与当前代码的基线有别；对历史批次结果只按其文字作为旧审计证据，本轮没有重验其测试结果、哈希或其余来源。
