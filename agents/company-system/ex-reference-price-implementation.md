# 现金除息参考价基础模块实现记录

## 范围与语义

本模块只计算无交易所批准特殊调整时的纯现金除息参考价：登记日须为指定交易所交易日，除息日取该交易所下一交易日，参考价为前收盘价减税前每股现金红利。金额沿用 `Money` 的整数分表示，不进行额外舍入。参考价必须为正数；前收盘价及税前现金红利须为正数。

调用者必须显式提供 `CashDividendFormula`。标准纯现金公式使用 `StandardCashOnly`；`ExchangeApprovedAdjustment` 返回 `UnsupportedApprovedAdjustment`，不冒充完整调整公式。该模块不改历史行情，也不生成交易。

沪深差异测试通过测试专用 synthetic calendar coverage 注入上交所闭市日，验证日历查询按 `CalendarExchange` 区分。仓库默认日历目前没有可直接复用的官方覆盖条目，因此该用例不是官方休市事实测试。

## 测试与验证

测试覆盖下一个交易日、整数分精确计算、非正参考价、周末登记日拒绝、前收盘价/现金红利分别零与负数、大于前收盘的现金红利、日历适用边界错误、交易所日历覆盖差异、特殊调整公式拒绝。

曾经初次测试日期使用 2030 年 2 月并落入模拟春节闭市段，且预期的日期并非正确交易日；这是测试 fixture 错误，修正为六月交易日后通过。该次失败不记作需求红测。

之前一次 `node scripts/run-long-validation.mjs 300000 -- cargo test -p engine --lib --no-run -j32` 失败，日志见 `.tmp/company-system/ex-reference-build.log`。当时日志同时包含本模块一处非法函数调用 pattern（已修正）及其他并行修改中的模块编译错误；该失败不代表目标行为失败，也不作为当前最终验证。之后主 agent 完成共享 `engine --lib` 测试二进制编译，证据见 `.tmp/company-system/checklist-common/build-stable.log`。使用生成的独立 binary `target/debug/deps/engine-eb8b0dcc57b8995e` 执行 `node scripts/run-with-deadline.mjs 10000 -- target/debug/deps/engine-eb8b0dcc57b8995e company::ex_reference_price --nocapture`，退出码 0；`.tmp/company-system/ex-reference-short.log` 记录 9 passed、0 failed、0.01 秒。该命令由仓库 `run-with-deadline.mjs` 进程树监督，短测并发参数为 1 个独立 binary。

## 独立复核

独立审查指出交易所可批准特殊调整公式；已增加必传判别枚举及针对调整方案的类型化拒绝。复核者确认该契约能关闭发现；已提供 `.tmp/company-system/ex-reference-short.log` 供其最终复核。
