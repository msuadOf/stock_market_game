# 批次 212：历史审计工具复核

## 范围与来源核验

基线为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。按分派连续读取三份来源至 EOF，并核对行数与 SHA-256；结果见配套 JSON。另对照当前实现审计总账、工具复核记录、ADR-0001、ADR-0027，以及当前 `agents/oop-refactor-audit/exhaustive/` 中的同名工具。未运行工具、测试或构建，未执行 Git 写操作。

| 来源 | 行数 / SHA-256 | 全文结构 |
|---|---:|---|
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/root-audit-tools-final.md` | 62 / `3ef3780148874672216af394d007591ef146c7f82525303ae6972c9efbe9c4d5` | 核验范围与指纹（5–21）；完整元数据、索引、闭包绑定核对（23–48）；A股语义、最小范围及限制（50–62）。 |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/root-audit-tools.md` | 22 / `a417aca3fb29fa8fb06a3d887ce0bc925682ea58e847dc809f70fceb0bf4b54f` | 对 validator/renderer 的范围与指纹（3–10）；工具限制（12–18）；必要性与边界（20–22）。 |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/root-closure-tool-final.md` | 17 / `dc57c959378131ba35d248271241fae3d56e61d2104d3cfb8ad69648b8ec446a` | 复核基线与范围（3–5）；缺少 review digest 修复及边界（7–17）。 |

## 历史结论与当前状态

- 历史材料将 `validate_inventory.py`、`render_indexes.py` 和 `verify_closures.py` 限定为审计材料工具：前两者校验/呈现审计元数据，closure verifier 校验版本绑定与成员归属；不执行游戏逻辑，也不成为 engine、web、server 或 desktop 的生产 caller/consumer。当前总账的 G39、Q05 涉及的是另一类 K7/测试工具契约，不能与这些历史 inventory 工具混同。
- 根工具最终复核记录其版本绑定核对通过，并明确哈希不能证明源码语义、`full_read`、复核独立性或结论真实性；closure digest 缺失修复也被限定为狭窄的审计材料校验修复。现行总账没有将该历史审计闭环提升为产品功能或 A 股交易规则保证，符合该证据边界。
- 当前 `agents/oop-refactor-audit/exhaustive/` 下三个同名脚本的 SHA-256 分别为 `e302a0c9a65eae09608aca99bf4c1cc42b9b83b9ddf9530585ea4b9f31b69ea4`、`0bf61b30d1124d39a47c572417170a8a3185607ef4ef32f3fa8d9bcb991e669e`、`27be253dad339848eb9034b6dfdf79e674701859316bcf5ac15d3fd385ae1aed`，均不同于历史报告列出的指纹。因此历史复核只绑定其记录的旧版本，不能外推为当前同名脚本的独立复核或运行通过。
- 历史报告已披露 `validate_inventory.py` 对受控 manifest 路径的信任、renderer 对结构的信任、schema 不完整和 inventory 不以非零退出强制失败等边界。当前 validator 源码仍直接将 manifest `path` 与 `ROOT` 拼接；closure verifier 则对部分 path 使用工作区内解析并校验批次 `items/modules/review` 的 path/hash，但自定义嵌套引用格式仍可能异常退出。它们是审计辅助工具在受控输入假设下的限制，不足以推导生产代码缺陷；本批不提出 G/Q 候选。
- `ADR-0001` 要求决策可追溯且 accepted ADR 只追加；`ADR-0027` 明确编译、完整回归与发布范围边界。该批审计材料工具没有改变任何 ADR 契约、交易代码或交易制度；不涉及沪深市场规则，也没有重新查验官方规则。

## 三项门禁复核

1. **大 A 语义：** 三个工具只作用于审计材料与源码字节指纹，不运行交易逻辑、推导或改写 A 股规则。历史复核明确限定自身不替代领域审查；当前同名脚本指纹不同，本批也没有据旧指纹宣称当前实现获独立复核。
2. **必要性与范围：** 历史结构校验、导航生成、审计闭包版本绑定均服务穷尽审计，报告没有把这些工具包装成产品能力。已记录的输入/schema 局限是受控审计输入边界，不因本批材料升级为产品 G/Q。
3. **遗漏、漂移与复杂度：** 发现历史复核与当前同名脚本指纹漂移，故历史结论不可直接覆盖当前版本；当前工具的路径/schema 限制与历史所述大体一致。未发现跨层交易语义漂移或足以改变现行总账状态的新证据。候选状态未升级，未核销或重分类任何 G/Q。

## 结论

三份来源均与分派指纹和行数相符，且全文读至 EOF。它们记录的是历史审计工具的版本审查与范围限制，不是产品实现验收。现行总账中的 G39、Q05 保持原状态；本批不新增、不核销、不重分类 G/Q。当前同名脚本与历史复核指纹不同，应把其审查结论视为仅适用于历史版本。
