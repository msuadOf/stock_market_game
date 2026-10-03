# insurance 独立复核

- 日期：2026-10-03。
- 复核人：未实施此改动的 `/root/implement_domain/review_insurance` subagent（canonical 身份）。
- 基线：`b89afb3346743a4b4fccf26c9ac9ff108595f696`。
- 范围：`packages/engine/src/company/insurance/**` 的完整基线 diff，另全文阅读未跟踪的 `behavior_tests.rs`（351 行、10 个 case）。现有差异包含 `config.rs`、`mod.rs`、`claims.rs`、`groups.rs`、`service_release.rs`、`remeasure.rs`；无 `premium.rs` 改动。
- 权威需求：[action-index.md](../../oop-refactor-audit/challenge-2026-10-03/action-index.md) 中 `domain-N04` 的 insurance 部分与 `domain-R2-N18`；实施说明：[insurance-result.md](insurance-result.md)。
- 已阅读 `AGENTS.md`、`docs/principles.md`、`docs/architecture.md`、`docs/testing.md`、`docs/open-questions.md`、ADR-0002、ADR-0016、`docs/company-accounting.md §2.4`，核对完整 insurance 源码及现有场景测试的相关断言。

## 最终版本绑定

2026-10-03 补录时重新全文读取当前相对基线的 insurance 完整 diff，并重新全文读取 untracked `behavior_tests.rs`（351 行、10 个 case）。当前内容与上述已审版本一致，未发现期间发生源码变化，原复核结论继续适用于下列精确版本。实际改动文件共 7 个：6 个 tracked 修改文件和 1 个 untracked 新文件；普通 `git diff` 不包含该 untracked 文件，因此它单独全文 review 并纳入 SHA256 清单。

下列路径均相对仓库根目录，SHA256 针对实际文件内容计算；后续任一文件哈希改变，必须重新审查变化后才能沿用本结论。

| 实际文件 | 状态 | SHA256 |
|---|---|---|
| `packages/engine/src/company/insurance/claims.rs` | tracked 修改 | `c6d4c89b6d91ab488e2c3d493144eadc750c73a1cc3f41729b2c397f851eef99` |
| `packages/engine/src/company/insurance/config.rs` | tracked 修改 | `d30d14a440a2c470fc2aa2b0a3af6aa451de315aa0254a64116867f88eb2012a` |
| `packages/engine/src/company/insurance/groups.rs` | tracked 修改 | `dde8467e70664b439e66dbee4c8b8be9d2f7bf7a29afd11dfbb550f74c7d4e19` |
| `packages/engine/src/company/insurance/mod.rs` | tracked 修改 | `9c78d06a0f6cd4406804605bae31e18f2b81d27fe08924b49e50f08c2aa08f79` |
| `packages/engine/src/company/insurance/remeasure.rs` | tracked 修改 | `d5e817a9a1e1648500837cb73220777f6c28d5f7342986547231e403b5e33351` |
| `packages/engine/src/company/insurance/service_release.rs` | tracked 修改 | `87791ced250d599213feae331316ead55d76760c201127f895f8f173e114658a` |
| `packages/engine/src/company/insurance/behavior_tests.rs` | untracked 新增，已全文 review | `a6f81e12cda38b502efb1502dbc5c012ecf8c770a06bd26020d4960111032c01` |

## 结论与发现

本次静态复核未发现需要修复的行为回归、需求越界或重复权威状态。三个门禁问题均已独立检查，具体证据如下。此结论仅为源码审查结果，不代表测试或编译通过。

### 1. 大 A / 公司会计语义与依据

- 本批只重组 insurance 的状态 owner 和纯预览，未改投资者账户、证券撮合、shares、T+1 或板块制度。`AccountingAmount` 仍为 cents，`FractionUnits` 仍为 cents × 责任单元，没有混用股票 Money / 股数。
- `InsuranceConfig::check_opening_lines` 保持六项禁止清单和输入顺序的首个非法科目；`InsuranceBooks::new` 保持 Discount 校验 → 开局守卫 → 开局过账 → 对手方登记，未将清单改为允许清单。
- `premium.rs` 的建组和收款未改；赔案发生/支付仍分离，现金流方向、`BusinessKind`、`CashFlowClass`、LRC/LIC 科目与 event source 槽位未改。保费没有变为即期收入，也未新增投资者现金。
- 依据沿用 `docs/company-accounting.md §2.4` 登记的财政部财会〔2020〕20号 CAS 25（官方通知与全文 PDF；登记取证日期 2026-09-10），其他执行企业会计准则的企业自 2026-01-01 适用，默认 2030 开局已过门槛。GMM 的 D1–D5 游戏简化与 `TermProtection` 唯一支持范围保持原样。
- 本复核未重新联网读取官方原文；没有新增制度判断或会计规则声明。既有官方登记支持此次等价重构，不将游戏简化升级成完整 CAS 合规实现。

### 2. 必要性、范围与复杂度

- `ContractMeasurementState` 唯一持有五余额、units、coverage 日期、五 carry 和调节累计；`ContractGroupState` 保留保费身份并组合 measurement / `ClaimRegister`，符合 `domain-R2-N18` 规定的职责边界。
- `preview_release` 复用既有 `ReleaseBatch`；`preview_remeasure` 返回临时 `RemeasureDelta`。结果未长期保存，不引入第二份计量余额、动态派发、泛化险种框架或新依赖。
- `ClaimRegister` 唯一持有赔案 `BTreeMap`。当前生产写入经 `insert` / `apply_payment`，组查询仍返回引用/迭代器；Snapshot 只借用映射进行 Serialize，Deserialize 移交拥有权，没有克隆映射绕开 owner、补默认值或重建空赔案账。
- 扁平 Snapshot 增加显式字段装配是保持历史 serde 契约所需的局部复杂度；它沿用 derive 的 map/sequence 接受行为，避免 flatten 改变必填字段/重复字段语义。
- `premium.rs` 和 `operations/insurance.rs` 无需新流程：原 constructor 与 `InsuranceBooks` API 已真实接入新 owner。未扩展到外层存档版本、报表、宿主或其他行业。

### 3. 边界、跨层一致性与错误路径

- `groups.rs:428` 的 Snapshot 与基线结构逐字段核对：原 24 个字段、声明顺序、类型均一致，serde struct 名通过 `rename = "ContractGroupState"` 保留。`groups.rs:458` 借用 Serialize 映射和 `groups.rs:490` Deserialize 装配均与正确 owner 字段对应。
- 无 serde default / flatten / deny_unknown_fields：map 中未知字段继续忽略，重复已知字段继续拒绝，24 字段继续必填，sequence 继续按原声明顺序接受。新增 `behavior_tests.rs:104` 覆盖完整 sequence、逐字段缺失、未知 nested 字段和重复 `units_total`。
- `groups.rs:249` 的五次 `unit_release` 顺序、正 CSM/正 loss 分支、分母和 final batch 判断与原 `service_release.rs` 一致；`groups.rs:311` 的 ΔE、simple PV、checked ΔF、三向 CSM/亏损分流与原 `remeasure.rs` 一致。裸整数运算与 checked 运算没有互换。
- `service_release.rs:97` 与 `remeasure.rs:100` 仍先 `post_with_commit` 后 apply。预览只借用 `&self`；不存在过账前对子对象写入。新增 `behavior_tests.rs:296` 用重复 source 强制 post 拒绝，并比较完整保险账套序列化状态。
- `groups.rs:395` 的五余额递减 → 收入/财务累计 → 五 carry → units，以及 `groups.rs:415` 的剩余预期/CSM/loss 更新 → 三累计顺序保持基线。新增 `behavior_tests.rs:225`、`:329` 固定首个累计溢出的部分写入与 event 槽已提交事实。此次未将这些既有 apply 错误冒称为完全原子。
- CSM 因重估耗尽时，非正 CSM 分支继续保留原 carry；`behavior_tests.rs:313` 固定末批后的 `-145` 失活 carry。既有行为虽不同于头注的概括归零描述，但没有在此次重构中改变；实施说明也明确指出这一边界。
- 新测试还覆盖部分赔付恰清零后多一分拒绝、零重估占一 event 槽、恢复后继续分批释放/重估、LRC 恒等式、纯预览金样、正常末批余额/carry。既有 gold / remeasure / claims / guards / entities 场景继续保留盈利、亏损及转回路径，未弱化断言。

## 验证范围与剩余限制

- 按父任务限制，未执行 Cargo、产品测试、完整回归或 Git 写命令；未修改产品源码、未再派 agent。仅新增本审查记录。
- 未获取独立编译或测试结果；10 个新增 case 与现有保险集成测试须由父协调者的集中 runner 执行，结果另行登记。
- 新 serde 测试未直接断言原始 JSON 字节的 key 输出顺序，也未单独覆盖过短/过长 sequence；此次通过基线字段声明顺序和相同 serde derive 路径静态核对，未发现接受集或输出顺序改变。可在未来调整 Snapshot 时补强这类契约测试，此处不视为已发生的行为缺陷。
- 极值 apply 测试覆盖首个累计溢出；未逐项执行后续累计溢出的所有组合。现有迁移语句与顺序逐项等价，但本复核不据此宣称所有异常路径均有运行证据。
- 未审查本批其他行业、会计底座或整仓 diff；本结论不能替代它们的独立复核。
