# CompanySystem Simple final period core 独立复核

## 范围与依据

本次仅审阅 CompanySystem Simple 核心，不包含其他作者的 finance 汇总实现或 actions 接线，也未运行 Cargo。
核对了 `AGENTS.md`、`docs/principles.md`、ADR-0035、ADR-0036、Q14 最新设计及本轮 `company/{api,config,identity,persistence,system}.rs`、`simple/{config,state,period,environment,growth}.rs`、`spec.rs`、`rng.rs` 和相关 Simple 单测。

## 结论

- 当前核心对齐已确认的四种自然结算周期、按完整周期边界结算、自然月趋势分段、周期专属扰动、年化趋势定点复利、BigUint 临时高精度根比较、金额 half-even、严格恢复、身份唯一性和候选结果先完整计算再提交的要求。
- arbitrary start date 在初始化时保留完整自然周期边界；当前未结周期不会生成/披露候选。环境变化敏感度仅加入营收趋势，不直接改写费用。负利润、超过 100% 的营收比例开支和显式零营收复业均保留。
- CompanySpec/spec 的 `deny_unknown_fields`、OperatingRng 的严格字段约束、唯一映射解码及 strict 公司身份重校验与无兼容存档决定一致。
- CompanySystem 仅能创建 Simple；Simulation 明确 Unsupported，没有静默降级。没有看到交易所制度或 A 股撮合语义被本核心改变，因此无需新增官方规则依据。

## 有效发现

未发现 P1 或足以阻止该核心合并的语义缺陷。

关于期间噪声叠加造成负增长因子的情况，设计允许增长因子非法时显式报错；候选事务保证错误不会部分提交状态。本轮没有要求裁剪或重抽，因此不将此前参数组合建议登记为未闭合的缺陷。

## 复核限制

此审查不覆盖 SimpleFinance 账簿映射、公司行为执行，也不代表整个 session 集成验收通过。未运行 Cargo，未实施代码改动。

## Final 增量复核（尚未通过全核心门禁）

增量审阅确认 `SimpleFundamentals::validate` 已要求历史 `explanation.cycle` 等于配置周期；`advance_trend` 仍按自然月抽样并推进 `remaining_months`/RNG，随后先合并相邻同年化率段，再执行因子计算；恢复校验拒绝相邻同年化率的非规范分段。`cash_settlement: false` 与未完成的共同 actions 接线状态一致。

后续增量已为 `CompanySpec::listed_stock` 和 `CompanySpec::group_parent` 两个 `Option` 字段添加 `required_nullable` 反序列化属性；显式 `null` 仍表示无映射/无母公司，缺少字段会被拒绝。Serde 标注没有转换或规范化身份字符串，也没有改变 `CompanyKind` 及其标签。`CompanySystem::capabilities` 保留 `cash_settlement: false` 并说明实际投资者结算待共同接线；这准确描述当前实现，不表示取消两种模式共同支持公司行为的产品要求。

## Fresh limited-core 验证

亲读 `.tmp/checklist-wave4/host63-core-owned-green.log`、`host63-period-owned-green.log`、`host63-growth-owned-green.log`：分别记录 15/15、10/10、6/6 通过，0 failed、0 ignored。覆盖严格 nullable issuer 字段、自然周期和历史恢复、初始基准、重复映射、RNG 恢复、失败原子性；相邻趋势合并、年率边界、自然月持续时间、四周期噪声及只作用营收的环境敏感度；定点复利、half-even、大额与 signed minimum 边界。三日志报告测试执行分别为 0.03s、0.00s、0.00s。结合本轮对冻结源码差异的静态复核，limited CompanySystem Simple final period core gate 通过。

该 PASS 只适用于上述 core 文件与三份定向日志；不涵盖 SimpleFinance 全范围、共同股本行为的实际投资者结算、所有宿主/会话路径、Simulation 或完整回归。未运行 Cargo、完整回归、WASM 或浏览器验证，也未复核 A 股行为制度变更（本核心没有引入相关规则改动）。
