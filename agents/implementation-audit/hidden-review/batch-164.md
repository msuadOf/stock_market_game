# 隐藏扫描批次 164

## 范围与读取证据

- 按 `scan-plan.json` 处理 batch 164，owner=4；source baseline `43b1aa5`（实测 HEAD `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`）。来源位于 `/data1/baiyifan/workplace/stock_market_game`，记录写入计划指定的 `output_root`。
- 三个来源均从首行分段读取至物理 EOF；`wc -l` 与 SHA-256 均匹配计划，详见 `batch-164.json`。长行造成的初次终端输出截断已通过分段读取补全。
- 当前消费者依据 baseline 源码、隐藏扫描中既有 caller 核对，以及当前实现总账追踪；历史材料里的“本轮未运行”或规则/会计依据声明仅作为历史陈述，不视作本轮验证或法源背书。

| 来源 | 完整性 | 范围 |
|---|---|---|
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/modules/engine-tests-04.md` | 157 行，SHA 匹配，读至 EOF | compute/config/consolidation/diagnostics/experience/replay/fundamental beliefs 等测试与跨文件关系。明确是未实施的测试支撑审计。 |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/modules/engine-tests-05.md` | 236 行，SHA 匹配，读至 EOF | fundamental-beliefs 估值测试、industrial-accounting 金样/失败面、industry-reports 测试与跨文件边界。明确测试/fixture 均不迁入生产对象。 |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/modules/engine-tests-06.md` | 246 行，SHA 匹配，读至 EOF | industry-reports、information-acquisition、insurance-accounting、market/orderbook/money/observations/pipeline 测试及其边界。明确没有重查官方规则。 |

## 当前调用与消费者

- Compute 与策略：`packages/engine/src/compute.rs:25-90` 有批量 CPU/backend 接缝；历史 `compute.rs` 测试不足以证明 session/宿主接入批量调用，G17 在现行总账仍开放。`packages/engine/src/behavior/mod.rs:93` 与 `packages/engine/src/strategy/zi_noise.rs:192` 是 experience 决策函数的消费者；纯函数/fixture 不等价于所有档案消费链完成。
- 费用与撮合：`packages/engine/src/config.rs:234-238` 计算过户费；买卖结算消费者位于 `packages/engine/src/account.rs:312-335,422-452`。现行实现将买卖双方按配置计算；历史单一 `GameConfig::transfer_fee` 算术断言本身不覆盖交易实际扣费。`packages/engine/src/market.rs:479-568` 包含 Market/限价实现与相关调用面，测试里固定 10%/1 分的普通 Market fixture 不代表交易所、板块和证券类别完整差异。
- 财务估值与披露：估值 owner 为 `packages/engine/src/strategy/fundamental/valuation.rs:156-170`，NPC/机构消费链包括 `packages/engine/src/session/decision_chain/roots.rs:430-447`；报告生成 owner `packages/engine/src/accounting/reports/mod.rs:187`，合并算法 `packages/engine/src/accounting/consolidation/mod.rs:85`。日终披露仍需按真实请求/消费者判断；历史测试金样不能凭自身核销 G06/G09/G28。
- 公司会计与保险：`packages/engine/src/company/operations/industrial.rs:85-100` 和 `packages/engine/src/company/operations/day.rs:79-85` 是经营调用链；独立账套会计测试不能证明期末折旧、税和债务支付调度完整（G35/Q23 已记）。保险测试只说明 InsuranceBooks API/账务行为，不能推出 session 组装与保障期调度完整（G36/G59 已记）。来源明确 VAT/CIT 测试使用合成税率；CAS 18 约束按简化处理，不能将其描述为现行法定税率/完整准则实现。
- 其他测试边界：diagnostics baseline 不能核销 G57 causal DTO 大整数契约；单 worker extraction replay 不能核销 G39 的同一实际受理轨迹矩阵；report/consolidation fixture 不能证明真实生产披露链完成。相关 G/Q 均已在现行总账和既有隐藏扫描记录中关联，没有因本批重复新建候选。

## A 股语义与结论

- 本批没有实施交易制度更改。与交易有关的限价测试仅固定通用 Market fixture；来源自身已说明该 fixture 没有按交易所、板块或证券类别建模，不能作为现行沪深规则覆盖或官方验收证据。交易语义仍须以 `docs/trading-rules.md` 与适用 ADR 为准；本轮未重新查询交易所/中国结算官方来源。
- 过户费测试对应当前配置中的游戏费率；源码调用确认买卖双方的账户结算均消费该计算。不能把该单元算术断言扩大解释为完整交易税费验证或当前官方法源核验。
- **新候选：无。** 本批文件的结论均属测试支撑/fixture 保留建议；其测试覆盖与生产消费链之间的边界、会计税率简化和并发 replay 限定已有明确总账登记，不构成遗漏的已批准生产迁移。
- 只新增本批扫描记录；未改产品文件，未运行测试、构建或回归，未执行 Git 操作。具体元数据及引用见 `batch-164.json`。
