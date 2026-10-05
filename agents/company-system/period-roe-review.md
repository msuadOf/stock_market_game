# Period ROE 独立复核

- 日期：2026-10-06
- 范围：`packages/engine/src/accounting/period_roe.rs`、`period_roe/tests.rs`、`accounting/mod.rs` 导出；对照 Q14 §3、现有报告 scope／归母指标契约。
- 方式：未参与实现；静态检查完整最终代码/测试与导出差异，并亲读实现者提供的 basis 红测、绿测和重编译日志；未自行运行测试或回归。

## 结论

没有发现时间权重公式、精确比例或明确溢出处理的实现错误。最终实现以 `EquityBasis` 将权益及净利润绑定到相同归属口径，并将每个权益事件也绑定同 basis；先前发现的 scope 相同但归母/总额口径混配已由实现校验和测试闭合。实现把此结果清楚界定为分析用 `PeriodROE`，没有声称它是 CSRC 法定披露 ROE；净利润只用作分子，未按猜测将 `NP/2` 加入权益。自然日区间 `(start, end]` 与“权益变动自生效日计入”一致：事件日之前的旧权益计至前一日，事件日的新权益计入当天，期末事件计一天。Q14 所要求的期间、范围、平均权益及非正权益不可用边界与实现方向相符。

## 发现与建议

1. **归属 basis 与 scope 的合法组合仍是调用契约（低优先级）**：最终 API 校验净利润、期初权益、权益变动的 basis 一致，阻止 total/归母混配；但类型还允许 `ScopeId::Standalone` 配 `EquityBasis::AttributableToParent`。Q14 明确将“归母”用于合并归母口径。当前无消费者，未造成已观察结果，不阻断纯分析计算基础的通过结论；接入报表时需选用合法的合并归母 basis。

## 语义与范围核对

- `AccountingAmount` 以分表示，平均权益以“分×日 / 天数”保留为精确比率；ROE 以“净利润分×天数 / 权益分日总和”表达，单位约去。分数不约分但数值精确，调用者负责展示格式。
- 日期来自 `CivilDate`，按自然日而非交易日计权；这与 Q14 的财务期间/自然日语义相符，不是交易制度计算。
- 同日权益变动先合并，避免依赖输入顺序；合并后余额越过 i128 才报错，使用 BigInt 累积防止中间求和溢出被静默截断。
- 变更只新增一个会计指标模块和公开导出，未见对账簿、报告或其他指标的无关实现修改；当前文件未把分析指标扩称法定披露口径。

## 验证状态

亲读日志：`.tmp/company-system/checklist-common/basis-red.log` 显示 basis 测试按预期失败（7 项中 6 通过）；后续 `.tmp/company-system/checklist-common/roe-fresh-green.log` 显示包含 end+1 和净利润 scope mismatch 断言的最终定向测试 7/7 通过、0 秒；`.tmp/company-system/checklist-common/build-final.log` 显示 engine test profile 编译成功并生成测试二进制，只有既有 unused/dead-code 警告。红测→最终绿测证据一致。未自行运行测试、编译或回归；本次结论限定于纯分析用期间 ROE 基础，不核销 CSRC 法定标准 ROE，也不核销任何消费者接线。
