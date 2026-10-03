# 批次 219 独立复核

## 核验范围

基线为 `43b1aa5`。按计划完整读取三篇来源至 EOF；SHA-256 与行数均与 `scan-plan.json` 一致。复核了当前 `apps/web/src/host/protocol/parse.ts`、`validate.ts`、`reduce.ts`、`wire-values.ts` 中涉及的 owner、调用者和消费者。

## 结论

未发现历史复核结论与当前协议代码调用关系存在实质冲突；其范围限制仍需保留。

`parse.ts` 是 `EVENT_NAMES`、`EVENT_SOURCES`、`REJECTION_REASONS`、`CIVIL_KINDS` 的当前 owner：`event()`、`stableKey()`、`civil()` 等本地解析函数消费这些 allowlist；`parseEngineUpdate()` 导出供协议 reducer、normalize、WASM 更新交付以及相关测试/fixture 调用。当前 `reduceEngineUpdate()` 的生产路径确实先对未知值调用 `parseEngineUpdate()`，再调用 `validateEngineUpdate()`；`validate.ts` 中 `validateCivilUpdate()` 消费本地 `nextDate()`。因此，日期复核关于“typed DTO validator 单独调用的保证有限，生产解析路径另由 `parseIsoDate()` 校验”的区别仍成立。`wire-values.ts::parseIsoDate()` 检查 ISO 形状、日历有效性和 1900–2199 年范围，并被 civil 日期字段及 `CivilDateAdvanced` 日期字段消费。

日期检查只验证民用日期连续性与输入格式，不是交易日历或休市判断。常量复核限定于四类 wire 标签的完整性、消费点和 retain 必要性，不扩展为 A 股制度审查。历史入口明确标记候选设计未实施、仅通过所述调查材料静态审查；增量复核也只覆盖四组常量。未发现把候选审计写成源码实施或完整验收承诺的迹象。

## 边界

这批材料不覆盖 `parse.ts` 全部符号，也未对其所引用的整个 web-03 文件集重新审查；历史来源声明未运行测试、构建或生成器。本轮亦未运行测试或构建，没有据此判断更广泛的协议行为或交易制度合规性。
