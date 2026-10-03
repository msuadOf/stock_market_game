# 批次 216：tooling 历史复核记录交叉核对

- 基线：产品调用方工作树 `.worktree/implementation-reaudit`，HEAD `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。按 `scan-plan.json` 的 batch 216 / owner 1 连续读取三份来源至 EOF；实测行数和 SHA-256 与计划一致，详见配套 JSON。
- 本批来源是 tooling-05/06/07 的历史独立复核记录，不是源码实现授权或当前运行证据。未运行测试、构建或 Git 操作；不修改产品代码或 G/Q 总账。

## 来源内容

1. `tooling-05.md`（81 行）：记录 `EscrowVerificationRun` 的等价聚合候选已被限制在 matrix 单次运行生命周期状态；`validateArtifacts` 的 symlink/读前 containment 问题明确作为独立 D01，而非 A01 对象迁移动作。文中沿用的历史源码检查指出该 validator 在 canonical containment 判断前读取目标；历史 review 说明未修复，且测试没有对应 symlink 用例。该文件末尾只校正 canonical reviewer 与 item/module 哈希身份。
2. `tooling-06.md`（45 行）：部署 smoke、Pages smoke、WASM 构建薄入口及 workspace path 工具的历史文档复核。记录 `smoke-pages.mjs` 词法 containment 后直接读取、可能跟随 symlink 的边界；同时明确这是工具风险说明，不是产品/浏览器验收。metadata delta 补充 Windows/Linux/macOS 薄入口及 README 调用依据，结论限于文档与职责准确性。
3. `tooling-07.md`（17 行）：WASM worker harness 的历史独立复核；对象迁移结论为无 OOP 动作，另记尾部释放不在 `finally`（D01）及未知公司与 `page_size: 0` 并用导致拒绝原因不能分别证明（D02）。报告明确这些是历史 harness 的风险边界，不代表当前 API 兼容性验收。

## 当前裁定与去重

- **tooling-05 A01：** 已有复核 `batch-048.md` 将 `EscrowVerificationRun` 对应聚合定位为当前实现；不能把历史“候选/未实施”套用为现状，也不能把对象化当作 validator 缺陷修复。历史 D01 已在当前总账 G72 单独登记：要求验收 artifact canonical containment 先于读取；本批不重复新增 G，也不以本批历史文档声称修复完成。
- **tooling-06 Pages symlink：** 历史文档如实记录实现风险，但当前契约裁定 `candidate-resolution-03.md` 已拒绝将其认定为确定 G：Pages smoke 是开发者短 smoke，当前没有不可信目录或拒绝 symlink 的正式承诺。本批沿用该裁定，作为可选硬化观察，不并入 G72（其 owner 是 matrix artifact validator）。
- **tooling-06 构建入口：** 历史 metadata delta 只补证薄入口委托关系，不声称运行 WASM 构建或完整发布验证；不据此核销其他构建/发布 G/Q。
- **tooling-07 D01/D02：** 历史记录限定在该手写 harness 的资源释放与错误诊断断言，没有批准的 OOP 行为承诺，也没有当前兼容性运行证据。本批未发现可据此核销或新增 G/Q 的依据。

## 门禁结论

1. **大 A 语义：** 三份记录聚焦工具链与 harness 生命周期，不引入沪深 A 股交易规则、资金或股份单位主张；不从测试工具语义外推产品行为。
2. **必要性与最小范围：** A01 对象聚合、G72 validator 缺口、Pages 可选硬化与 worker harness 风险分属不同边界；按既有裁定去重，不扩大候选范围。
3. **边界与诚实性：** 历史 review 只证明其当时审查范围，不能替代当前测试、构建或安全复现。未发现本批材料可证明 G/Q 已完成、发生错误核销，或产生新的已批准承诺遗漏。

**结论：** 历史文书链条与现有复核/裁定相容；保留 G72，Pages symlink 仍是非 G 的硬化观察，tooling-07 的 D01/D02 仍限于历史 harness 风险。无新增 G/Q，无核销项。
