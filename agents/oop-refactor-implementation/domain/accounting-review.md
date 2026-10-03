# accounting 9 动作独立复核

- 复核日期：2026-10-03。
- 基线：`b89afb3346743a4b4fccf26c9ac9ff108595f696`。
- 复核者：未参与本批源码实施的独立 subagent；canonical 身份为 `/root/implement_domain/review_accounting`。
- 范围：`accounting-result.md` 所列 13 个源码文件的完整基线 diff；无新增源码文件。另只读核对 Industrial caller 与相关既有测试。
- 结论：未发现需要修复的新增行为回归、跨层语义漂移或超出这 9 个动作的实现。三项门禁在**静态复核范围内可接受**；本记录不代表编译、测试或整批验收已通过。

## 依据与范围

已读 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`、
`docs/architecture.md`、ADR-0002/0003/0005、`docs/company-accounting.md`，
以及 challenge `action-index.md` 中 N05/N06、R2-N01/N02/N03/N04/N06/N07/N08
的动作正文。完整 diff 覆盖以下文件：

- `packages/engine/src/accounting/closing/mod.rs`
- `packages/engine/src/accounting/closing/save.rs`
- `packages/engine/src/accounting/consolidation/eliminate.rs`
- `packages/engine/src/accounting/consolidation/sale.rs`
- `packages/engine/src/accounting/fixed_assets.rs`
- `packages/engine/src/accounting/inventory.rs`
- `packages/engine/src/accounting/reports/balance_sheet.rs`
- `packages/engine/src/accounting/reports/consolidated_window.rs`
- `packages/engine/src/accounting/reports/income.rs`
- `packages/engine/src/accounting/reports/mod.rs`
- `packages/engine/src/accounting/reports/notes.rs`
- `packages/engine/src/accounting/reports/window.rs`
- `packages/engine/src/accounting/tax.rs`

## 三项门禁

### 1. 大 A 语义与依据

本批改变行为归属和调用组织，没有新增交易制度或会计政策判断。
`AccountingAmount` 保持 i128 分，Inventory 数量保持整数件，FixedAsset 寿命保持月，
集团 `issued_shares/parent_held_shares` 保持股，minority 比例保持 bp。
不引入投资者 `Money`、分红、补钱、成员现金抵销或证券 T+1 变化。

官方依据的可靠性结论限定为沿用项目已有登记：`docs/company-accounting.md`
记载 2026-09-10 取证，CAS 33 固定控制等简化、财会〔2016〕22号 VAT 会计处理、
CAS 30 列报框架继续保持。CAS 1/4/8/18/28 原文及 VAT/CIT 税法取证受阻，
移动加权平均、减值后剩余寿命摊销、全额 DTA、工作底稿流量归属当期等仍为既有
游戏假设，不能把本次方法化称为真实准则或税法完整验收。本次未重新联网核验官方原文；
因为没有改规则，未产生新的规则依据缺口。

### 2. 必要性与最小范围

9 个动作都在 action-index 授权的 owner 边界落地：Entry/State 持有自身迁移，
Policy 持有纯算法，Register 持有重述索引，ReportClassification 持有分类约束，
EquityPresentation 持有列报派生值，Builder 持有一次构建的中间事实。
没有新依赖、动态派发、全局服务、第二份账本或新的持久化 schema。

N05/N06、R2-N01/N04/N06/N08 本来是可选内聚性改进，并非产品缺陷修复；
当前实施确有多个相关行为和生产 caller，未退化为空壳类型。
`EquityPresentation` 不存储可重算合计，比较期也不增加额外 checked 求和；
公开 tax free API 和 `merge_assignments` 保留兼容，生产报表路径消费有效分类对象。
`reports/mod.rs` 的分号是局部格式调整，未改变逻辑。

### 3. 边界、跨层一致性与复杂度

| 动作 | 独立核对结果 |
| --- | --- |
| N05 | `Accumulator::add_current_worksheet` 保留 worksheet/line 输入顺序、Debit 正/Credit neg、closing→movement→quarter→ytd 的 checked 写入顺序。cash、prior_year、prior_year_end 均不摄入。错误仍由 Builder 包装为相同 ReportError。新增空输入、符号、neg/add 溢出及跨桶部分写入断言对应真实风险。 |
| N06 | 三个 free API 只委托 Policy，旧算法主体各只保留一份。VAT 先 input_rate 落分、再 deductible_share 落分的两次 half-even 未合并；没有新增 TaxPolicy validate。IncomeTax 保留 saturating_sub、严格大于结转年限才到期、稳定 origin_year 排序、零/正/负 pretax 分支、先 current_tax 后 loss_added/pool_total/DTA 的失败顺序，输入 pool 只读。 |
| R2-N01 | `correct` 仍先使 hash cache 失效，在 `Books::post_batch` 成功后才 record，随后 generate 失败仍可能已有 Books/register 写入。EngineSave 字段名称和声明顺序、Vec 行类型不变；新 Register 自身不直接参与 serde，save_rows/from_rows 与原 BTree 投影相同。重复 Scope/source 最后覆盖、空 Scope 行跳过且不删除既有非空行、BTree 输出顺序及原反序列化接受集保持。 |
| R2-N02 | `apply_depreciation` 查存在→剩余月数→base/rhe→累计折旧 add→月份推进，顺序与旧实现相同；add 失败不会推进月份。`validate_impairment/apply_impairment` 保留非正金额优先于 UnknownAsset，再查 floor、累计减值 add，不增加 post 后校验失败面。新增 5 分/2 月的 2、3 分落分及残值保留断言与既有 FullyDepreciated/超 floor 拒绝相互补充。 |
| R2-N03 | receipt 的正数量→正成本→AccountMismatch→quantity checked_add→quantity 写入→cost add 顺序保持。新增测试诚实锁定 cost 溢出后已写 quantity 的既有失败行为，没有偷改为原子提交。issue 仍 UnknownItem 先于非法数量，preview 完成后 quantity 扣除、cost checked_sub；末批守恒由既有 subledgers 用例覆盖。 |
| R2-N04 | validate_for 原样保留成员→自指→收入/成本/库存科目→invoice→cost→unsold 的首错顺序。Validated 只借用申报，字段 private，仅成功校验可在生产获得；利润公式仍 checked margin×unsold/invoice 单次 rhe。零利润仅两行，非零追加 inventory 行；原 eliminate_sale 删除，唯一生产 caller 改为 validate→生成。IntercompanyBalance 配对与重复申报拒绝逻辑未动；不存在两份 sale 算法或重复抵销。 |
| R2-N06 | paid_in 符号求和、BTree 次序扣 non_root capital、retained、paid_in+retained、加 minority、减 minority 求归母的 checked 次序均保持；liabilities_and_equity 仍由原 generate 计算。prior_split 缺失仍保留已生成资产/负债行且省略权益，存在时按 root_capital/parent/minority 原样列示。EquityPresentation 自身没有比例运算；minority half-even 仍在原 Consolidation/PriorSplit 计算中。 |
| R2-N07 | ReportClassification map private；from_industries 仍先按 industry/assignment 顺序拒绝冲突，后按 defs BTree 顺序拒绝首个缺分类，额外行业映射保留。同目标幂等、四行业入口与原 merge_into 未变；income/balance_sheet/notes 迭代顺序相同，未改金额或缺余额为零的既有约定。 |
| R2-N08 | Builder 先按 members 顺序建立 defs/sub_ids，再按原 members/journal/line 顺序扫描。先选首个同码 def 的既有行为保持，集团校验仍由 consolidate 负责；历史净权益、非根贡献、root_prior_capital 没有增减项。worksheet 应用→prior_split→Scope 检查的次序相同；PriorSplit 总额 BTree 求和、每 subsidiary 先 apply bp 再加总保持，未合并舍入。新增多子公司和有/无历史断言对应拆分边界。 |

N06 的只读 caller 核实：Industrial `sell_credit` 在计算 revenue 后调用
`self.tax_policy().vat.output_vat_on`；`purchase` 在 goods 后调用
`vat.split_input_vat`，原含税应收、不可抵扣归成本与过账顺序保持。
`IncomeTaxPosition::preview_income_tax` 调 `policy.compute` 后再读取 posted DTA，
`accrue_income_tax` 仍在非零分录 post_with_commit 成功后 commit ending_pool，
零分录分支也照旧提交；Policy 不接管 Books 或 loss_pool。
FixedAsset Industrial caller 仍 preview/validate→post_with_commit→apply；
Inventory caller 仍由行业层协调过账。

## 测试证据与局限

新增 13 个短单测的 diff 已逐项阅读。另阅读既有 `subledgers.rs`、`tax_gold.rs`
相关完整断言，并核对 correction_restatement serde、classification failures、
upstream/downstream intercompany gold 和 consolidated gold 的关键入口。
未发现本批重构所需但完全缺失的主要行为断言；同年 loss 稳定排序、错误组合的全部排列、
恶意 serde 状态仍没有穷举，本次判断依赖算法和接受集的逐行等价核对。
四行业分类新增用例验证 mapping 覆盖/重复性质，实际 chart 的跨层正确性仍须由行业
gold/公开财报集成测试验证，不能把该单测视为替代。

遵照任务约束，未执行 cargo、测试、Git 写命令；也没有重新产生红灯、绿灯或性能结果。
实施顺序和测试先写的历史不能仅由最终 diff 证明。root 应记录统一编译和实际短测试结果，
再决定整批验收结论。本报告只新增复核记录，没有修改源码。

## 最终源码版本绑定

2026-10-03 按总审要求补记身份与版本。当前 `HEAD` 仍为基线
`b89afb3346743a4b4fccf26c9ac9ff108595f696`，因此只写 HEAD 不能代表工作树实现版本。
重新读取当前基线 diff，并逐文件计算内容 Git blob；13 个文件的 blob 均与首次完整
diff 阅读时记录的 `index` 新 blob 前缀一致，未发现 accounting 源码变化，原复核结论
继续适用于以下 SHA256 绑定的工作树内容。首次审查时未独立保留 SHA256，以下是本次
最终内容快照的 SHA256，不冒称为首次审查当时已经生成的证据。

`git diff --no-ext-diff b89afb3346743a4b4fccf26c9ac9ff108595f696 -- packages/engine/src/accounting`
的输出 SHA256 为 `20528bfbac995e7628c0bc1b4755a39ceb1afd3d1a7c188940750d681735929a`。
文件路径以下均相对 `packages/engine/src/accounting/`；内容以原始文件 bytes 计算。

| 文件 | SHA256 | Git blob |
| --- | --- | --- |
| `closing/mod.rs` | `cbef8f9363d485909decfbeaabd50ca41784347ae7f6c83692b8976992859bdb` | `b02232cccc1315af7c919e6dfd1039dda7494958` |
| `closing/save.rs` | `5c29bd62dc8efbc68f1831ed573825365d15731a4ee05c1f1a25c3983be3e9dd` | `3548c8110099126153cb5f2cf9ee759ad41f01f2` |
| `consolidation/eliminate.rs` | `5ea1c1bea31d8c2d2103d98a270ba18b7288d7f32aad182d5750c0455c9948a9` | `46b710bfd46d5346e2487d40e8174bb8976dd2be` |
| `consolidation/sale.rs` | `61818a89559649263560da19f146b195c158edd5287b5e52b7f593141cb75d63` | `5ca4e9134e8a423754385e53414e07cad5ea36d2` |
| `fixed_assets.rs` | `f26c9cffefdd54e7e9d1aba1308b4affc80ab909bb680b8858d9d224b8eeff1b` | `9363c6336eb354daf0d9bc9eebb93bb75230cee1` |
| `inventory.rs` | `ab6f387d25fff4e4f08471ee55ba68b36ab5b3ba20f37fa26be02ec1131740ad` | `c844ed9c356eba6f4e3a122f7c3112346c5a79a7` |
| `reports/balance_sheet.rs` | `469c184215923db7e5915fbe16aa95a3206b11f19c20ebf8bfdc05e5d8c751f9` | `e8bfca04fd63d5f68860d48bc3a2e74166189a37` |
| `reports/consolidated_window.rs` | `68c92af612106930dd07e2ce15eeba17ad81ee01669f06bb4c2709aae0132d2f` | `cda78f88a3d3c0cd351a18a8adeb00ebf977529d` |
| `reports/income.rs` | `d65999d234c4dcc0c605ada74433487ef2a12768406ee4c1803903c4716881ea` | `e8033054caaa572807660d86162f5cee58067e28` |
| `reports/mod.rs` | `fa15e3bcafe7737568c39240a9465477c740c446b975bf4132db83cd99d4a0bf` | `8016a536e615133c1cf65c14b33776b796c3fb7c` |
| `reports/notes.rs` | `076da9ee56a672bf01a71153b415467373dff8074d835546e85f1b4c5b3757b2` | `f2551a49f2b3a76b1002dd3c73ec93e428a75a52` |
| `reports/window.rs` | `4213b053b7d9081892121431b21c8496a8c8fa860bf15cd1a4358285f3d55098` | `be714d64f0e3d3e0cac65e1aecfda02368935a78` |
| `tax.rs` | `baaa3eb811c27ca35a78023771ea1cecf0c5cb13d57a96aa8741819c0c113a7b` | `c488366f5c88c7232658e88c9ce2ba9803828c09` |

上述文件若有后续内容变动，本记录不自动覆盖新版本，须再次复核。此次仅修改工作记录，
未修改源码、未执行 cargo 或测试。
