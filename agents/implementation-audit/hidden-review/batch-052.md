# 隐藏扫描批次 052

## 来源与完整性

按 scan-plan 中 batch 52 分配读取主仓来源连续全文至 EOF；计划行数与 SHA-256 均匹配，未发现来源漂移。逐篇章节矩阵如下：

| 来源 | 实测行数 / SHA-256 | 章节及 EOF |
|---|---|---|
| `agents/oop-refactor-audit/exhaustive/reviews/contracts-01-final.md` | 31 / `36d38fdac95145b8dc1b04b5f35cb1fb0422d2dd774b492b6ad1dcd52bead093` | `contracts-01 候选 delta 独立最终复核`（1）；审查范围与证据（7）；领域语义、必要性与边界（15）；继承范围（23）；最终核验指纹（27）；读至 EOF（31）。 |
| `agents/oop-refactor-audit/exhaustive/reviews/contracts-area-final.md` | 20 / `2d14bae64f778d2fbd953fbc3a6993f8076cb7e8ef6d05cb0890684a9b1ebd06` | `contracts 区域候选最终文档复核`（1）；结论（3）；核对结果（7）；范围限制与指纹（15）；读至 EOF（20）。 |
| `agents/oop-refactor-audit/exhaustive/reviews/domain-closure.md` | 26 / `8e94cb86d025914fafe5e999b675dea13851286e8b8aa5ad58df4d52245c9fff` | `Domain closure 最终独立复核`（1）；最终指纹与追溯（5）；核验（14）；三项门禁与范围限制（20）；读至 EOF（26）。 |

产品源码依 scan-plan 基线 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad` 从 `.worktree/implementation-reaudit` 只读。遵循该基线 `AGENTS.md`、`docs/principles.md`。主仓 `AGENTS.md` 与 principles 要求领域语义一致、证据诚实和审计记录归档；本批不触交易制度实现，不据此主张 A 股规则变更。未改产品文件、未运行测试/构建、未作 Git 操作。

## 当前实现与总账核对

- `docs/decisions/0007-three-deployment-frontend-framework.md:33-42` 明确 `GameSession` 不可序列化、跨界使用 handle API，仅 Snapshot/Event/Intent/SessionSetup 跨界；serde 类型以 ts-rs 生成到 Web generated 目录，`engine.ts` 保留前端别名和 WASM 方法契约，不为迁就生成器更改三宿主协议。
- `.worktree/implementation-reaudit/package.json:17-18` 当前 `types:generate` 为 `cargo test -p engine export_bindings`，`types:check` 再执行 `scripts/check-generated-types.mjs`。这是现行调用/检查入口；它不证明未纳入检查的根 `bindings/` 历史生成来源或当前等价性。
- 当前生产语义由 Rust engine 和 host 协作持有；`packages/engine/src/session.rs:1-5` 声明纯逻辑、无 I/O、无全局状态。`GameSession` 的运行态不是 DTO。审计材料提到的 `ProtocolSession`/`GameSession` 职责描述仅可作为职责边界提示，本批没有逐个追踪它们所有方法与 caller，不据历史文档声称类实现已独立完整验证。
- 现行总账 `agents/implementation-audit/implementation-audit-2026-10-02.md:139` 的 Q01 仍记录 Money 为 transparent i64 JSON 整数、Web `parseMoney` 要求安全整数，跨端共同支持范围未统一。它与生成 TS 的 `number` 映射相关但不等同于本批所述 `bindings/` 来源问题；不能由旧候选复核核销 Q01。未找到本三篇要求新增的 G 项；G01–G68 不因 DTO retain 或生成脚本存在而被核销。

## 逐源判断

- `contracts-01-final.md`（全文）：审查结论明确只限候选及继承范围，候选尚未实施；旧 256 项及旧 review 结论不能扩展为当前 256 个源码的重新语义审查。生成目录、入口、large-int 映射和 freshness 范围以仓库配置为证；根 `bindings/` 的创建、消费、线格式等价仍未证实。其 retain 类型/接口、Money=分与交易数量=股、交易所和证券类别区分没有承诺运行时校验。与 Q01 关联，需保留现行疑问。
- `contracts-area-final.md`（全文）：只核对候选文档之间的条目数量、职责、单位与生成器说明，没有读取源码或完整调用点。`SaveSlot` 恢复事实、schema 形状校验与 engine 深度校验的说明不构成当前实现验证；同样没有证明 `ProtocolSession` 公共状态就是恢复权威。256 项、1+130+125 的划分只是指定历史清单一致性，不是生产覆盖结论。
- `domain-closure.md`（全文）：本体是 foundation-01 A02/A03 分类和 closure ledger 指纹核验；另外八批仅比较历史指纹，不重新语义复核。A02 非 OOP 组织、A03 行为保留属于候选分类，不代表产品实现漏项。条目明确未重新核费率官方依据、未复核其他八批语义或源码，也未运行测试。不得把 closure 的“通过”泛化为当前源代码或整个 OOP 改动已通过。

## 反证、限制与结论

候选通过记录自身提供了必要反证：根 `bindings/` 默认输出目录/整数映射只是 ts-rs 默认机制，不可推定仓库文件来源、消费者或历史命令；generated DTO 存在不等于运行时输入校验；`SaveSlot` / protocol 文档说明也不能替代对 owner/caller、serde 和恢复行为的实现核验。较新的基线文档 ADR-0007 给出明确类型同步契约；当前脚本入口支持其 Web generated 检查边界。较新的总账 Q01 仍开放跨端整数范围，必须保留，不能由旧复核摘要关闭。

本批三篇均为受限范围的历史复核/closure 材料，没有已批准 OOP 实现遗漏的证据，也没有足以新增 G 的现行行为承诺。判断为：文档审查结论在其狭窄范围内有效；不核销 Q01，不推断根 `bindings/` 的生成或消费者，不把未重读的源文件和代码实现记为本批已验证。未核实项包括 `ProtocolSession` 所有当下调用点、根 `bindings/` 消费者与历史产出过程、当前生成结果是否与源码一致，以及 domain closure 所列其他八批语义。
