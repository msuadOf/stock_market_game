# Formal Docs 最终独立复核

日期：2026-10-06

## 范围与方法

作为未参与实施的复核者，以 HEAD 为基线阅读全文并核对当前 diff：`UX-CONTRACT.md`、`docs/architecture.md`、`docs/company-accounting.md`、`docs/trading-rules.md`、`docs/simulation-calendar.md`、ADR-0030、ADR-0031、ADR-0033、`docs/open-questions.md`、`docs/work-status.md`。复核聚焦新增声明是否与已批准边界、实现状态及沪深 A 股语义冲突；没有改动正式文档、Cargo 或索引。

## 结论

未发现阻断合并的过度声明或已知领域语义错误。Simple 与旧 `CompanySimulation` 的边界、真实持股人的分红/认购/回购约束、严格存档及宿主限制均有明确区分；文档没有宣称 simple 追踪公司真实经营现金，也没有把尚未接线的共同股本行为写成已完成。

- `docs/company-accounting.md` 明确区分既有 `SessionSetup.company_operations` 四行业仿真接线与 `SimpleFundamentals`，且将 Simple 五个汇总科目标为游戏专属、`NonCash`、不代表实际合同或经营事实。银行利息/贷款/存款、保险保费/赔付/LIC、地产交付等没有从汇总科目推导出来。
- `docs/open-questions.md` 与 `docs/work-status.md` 一致说明共同财务、查询、披露、严格存档已接通，而实际投资者股本结算仍待接线；分红的可分配利润条件、认购真实现金限制、回购真实成交及幂等要求没有被展示 cash 混淆。未把 Q12“投资者现金池可收缩”误写为禁止公司行为分红。
- ADR-0030/0031 对 `AccountId` u64 规范字符串、Money i64 分、`AccountingAmount` i128 元、累计额 u128 分各自边界独立描述，未把 JS safe integer、金额格式或其他 ID 的格式相互扩张；也明确拒绝旧格式兼容和迁移。
- ADR-0033 将确定的产品/架构方向与跨平台验收状态分开，明确无 WAL、无 schema migration、失败不 fallback，并声明 Linux 源码/短测不能替代 Windows/macOS 实测。架构表的宿主分工与此决策相符。
- `docs/simulation-calendar.md`、`docs/trading-rules.md` 保留交易所分开判断、共享市场时钟、逐证券交易日语义及默认日历为模拟回退的边界；交易制度来源和适用日期沿用现有引用，没有新增无依据的 A 股规则断言。公司税及 CAS 限制同步保留游戏简化说明和来源缺口。
- `UX-CONTRACT.md` 的指标源/交割单和周期文字明确描述接口与用户契约；指标源不能认证 caller 自提交样本，Host capability=false 不回退；本人交易历史绑定身份且非公开 Trade 缓存。季/年/分钟 K 在 trading-rules 中仍注明后续实施，未将 UX 产品目标冒称为已经接入。

## 验收限制

本复核仅审阅文本与所述实现状态，没有运行完整回归；当前已知验证是 workspace/WASM/Web 构建及限定短测试通过，完整回归未通过/未完成。本结论不替代未执行平台的原生存储锁运行验收，也不表示完整交易/公司业务矩阵验收通过。
