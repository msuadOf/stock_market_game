# 历史确认缺口实施

## 范围与约束

在 `.worktree/implementation-audit-final` 的 `codex/implementation-audit-final` 分支，以 `542b6e8` 为实施起点，依据 [审计总账](../implementation-audit/implementation-audit-2026-10-02.md) 的 79 项确认 G 补缺。主工作区的并行修改不在本分支；Q 项不擅自定约，未来产品不启动。实现不是重复审计，也不以空接口或测试预期迁就错误核销。

79 项确认 G 均已在各自范围实施、短测并经非作者复核；逐项证据保留在总账原行，跨层收口见 [最终整合复核](final-integration-review.md)。这不表示 Q 项已解决、已跑完整验收或代码没有未知缺陷。用户补充的 A 股报表方向已按官方口径核验：scope 优先是明确游戏政策，归母数据缺证不冒充集团总量，见 [官方核验](report-scope-policy.md)。

仅运行定向短单测，不运行完整回归、长期市场、性能矩阵或发布验收。构建与测试执行分开；构建 `-j 16`，定向 Rust binary `--test-threads=8`、外部 `timeout 10s`，每次测试命令最多 10 秒。每批完成后安排非作者独立完整 diff 复核，再本地提交。

## 按依赖分批

| 批次 | 确认项 | 当前状态 |
|---|---|---|
| 恢复身份与日历输入 | G69、G71、G75、G76、G78、G80 | 已实施，15项定向短测通过；实现与复核见 [恢复批](restore-guards.md) |
| 独立模型/输入边界 | G06、G15、G54、G70、G74、G77 | 已实施，8项定向短测通过；逐期手算及语义复核见 [模型边界](model-boundaries.md) |
| 公开策略输入 | G55 | 已实施，4项定向短测通过；见 [策略输入](strategy-input.md) |
| 工商授信 | G58、G79 | 已实施，6项定向短测通过；见 [工商授信](industrial-credit.md) |
| 保险保障期限 | G59 | 已实施，1项定向短测通过；见 [保险期限](insurance-coverage.md) |
| 零NPC开局 | G29 | 已实施，5项定向短测通过；见 [零NPC](zero-npc.md) |
| 前端显示/导航/恢复边界 | G13、G44、G45、G53 | 已实施，34项相关短测通过；见 [前端边界](web-boundaries.md) |
| 资产与盘口 | G46、G49、G67 | 已实施，8项相关短测通过；见 [资产与盘口](portfolio-and-book.md) |
| 竞价曲线片段 | G47 | 已实施，14项相关短测通过；见 [竞价片段](auction-segments.md) |
| CI与Pages构建边界 | G26、G56 | 已实施，10项相关短测通过；见 [CI与Pages](ci-pages.md) |
| React渲染错误出口 | G50 | 已实施，12项相关短测通过，非作者复核通过；见 [React错误出口](react-errors.md) |
| 工具旅程、资源与case监督 | G60、G61、G62、G63 | 已实施并独立复核；短测及未跑边界见 [验收工具](acceptance-tools.md) 与 [独立复核](acceptance-tools-review.md) |
| 分时与逐笔 | G10、G11、G12、G14、G31 | 已实施，53项相关短测及SSR通过，独立发现修复并再审通过；见 [实施](chart-stream.md) 与 [复核](chart-stream-review.md) |
| UI契约与真实消费者 | G22–G25、G30、G32–G34、G48、G64、G65、G68 | 已实施，13文件定向短测通过，三项独立发现修复再审通过；浏览器矩阵未执行，见 [实施](ui-contracts.md) 与 [复核](ui-contracts-review.md) |
| 新局熵与基线配置投影 | G20、G21 | 已实施，Node与Rust定向短测通过、非作者复核通过；见 [实施](seed-baseline.md) 与 [复核](seed-baseline-review.md) |
| 公司子账与经营 | G73、G35、G36、G41、G28 | 已实施，真实Session/五产物/公告/恢复短测及非作者复核通过；见 [保险复核](insurance-restore-review.md)、[经营复核](company-operations-review.md)、[集团复核](company-assembly-review.md) |
| 策略与个人认识 | G07–G09、G38、G42、G43、G37 | 已实施，真实DecisionSnapshot捕获/DecisionShadow/root/预算/经历/DEV消费及非作者复核通过；见 [Retail复核](retail-beliefs-review.md)、[个人策略复核](personal-strategy-review.md) |
| 宿主控制、帧流与错误 | G01–G05、G18、G19、G40、G51、G52、G66 | 已实施，短测及非作者独立复核通过；见 [宿主实施](host-controls.md)、[宿主复核](host-controls-review.md)、[Remote实施](remote-chain.md) 与 [Remote复核](remote-chain-review.md) |
| 生产指标并行 | G17 | 已实施，native数值与WASM target编译通过、非作者复核通过；见 [性能实施](tick-performance.md) 与 [性能复核](tick-performance-review.md) |
| 生产tick所有权与诊断整数 | G16、G57 | 已实施，局部COW与跨层无损诊断短测及复核通过；见 [性能复核](tick-performance-review.md) 与 [整合复核](final-integration-review.md)，不冒称长期吞吐测量 |
| 验收契约与artifact读取 | G39、G72 | 已实施，保留真实收据/守恒/负控与先contain后read，JS/Rust短测和非作者复核通过；见 [工具复核](acceptance-tools-review.md)，未运行全矩阵 |

批次只是依赖组织，不替代总账单项验收；跨批共用文件按一个 owner 串行修改。Q、未来产品及未运行的浏览器/跨平台/长矩阵验收仍保留，不以确认 G 补齐宣称全部历史候选或未知缺陷已经消失。

## 语义边界

本批恢复检查仅约束现有类型不变量，不改变 A 股撮合、价格时间优先、T+1、费用、资金池、日终存档及本人信息边界。日历官方出处测试为合成 Fixture，不引入真实行情。机构个体参数与未定的双 profile 契约保留原边界。
