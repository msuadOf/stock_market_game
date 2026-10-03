# sweep17：初始流通盘与 Money 历史契约穷尽复核

审计基线：产品提交 `b76ece3`。读取时 worktree HEAD 为 `4ad5a2e`；对 `packages/`、`apps/` 和本轮三份原始文档执行 `git diff --stat b76ece3 HEAD` 无输出，确认本轮所审产品相同。已阅读根 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`，检查相关 ADR 与现行 `docs/trading-rules.md`。本轮只新增本工作记录，不修改产品或 Git，不运行测试；结论为源码与测试定义的静态复核，不能据此宣称测试通过。

## 全文读取与任务覆盖

三份原始文档均从首行连续读取至末行，合计 **1463 行**：

| 文档 | 全文行数 | 阅读范围 |
|---|---:|---|
| `docs/superpowers/plans/2026-06-29-initial-positions.md` | 525 | 1–525 |
| `docs/superpowers/specs/2026-06-29-initial-positions-design.md` | 149 | 1–149 |
| `docs/superpowers/plans/2026-06-29-money-fixed-point.md` | 789 | 1–789 |

下表原文行号均指上述文件，代码行号指本轮产品基线。

| 章节 / Task | 原文证据 | 实际实现 / 测试 | 状态 |
|---|---|---|---|
| initial spec §1 背景、通用设仓 | spec:10、16、20 | `account.rs:258` 提供 `grant_position`；`session.rs:1678` 新局实际调用；恢复改用 `session.rs:2735`、:2743 保留四项原始持仓事实 | 能力已实现；恢复共用同一 helper 的旧意图已被精确恢复需求取代，见后文 |
| initial spec §2 核心决策；plan Task1 类型/API | spec:24–39；plan:34–132 | `session.rs:885` 总股本字符串、StockSpec 中 float_shares；:936 配置；`account.rs:264` checked 成本；`lib.rs:63` 导出；`tests/account.rs:470`、:493 成本与溢出 | 已实现；旧隐藏 V 字段由现行个人认识架构取代，不作为遗漏 |
| initial spec §3 Random / ByKind；plan Task2 | spec:41–56；plan:136–281 | `session.rs:1652` seed、:1771 随机入口、:1775 Pareto 权重、:1798 末账户取余；`tests/session.rs:2359`、:2413、:2420、:2443、:2458 覆盖守恒/玩家0/确定性/成本/零float | 主干已实现；随机分布变更归 Q06 |
| initial spec §4 API | spec:58–96 | `account.rs:258` 返回 Result；`session.rs:1464` 新局透传 seed 错误、:1652 私有分配函数 | 已实现；Result 强于旧 void，不恢复旧静默溢出模板 |
| initial spec §5 错误；plan Task3 | spec:98–103；plan:285–427 | `session.rs:1090` 检查非负有限、:1097 检查正有限有效和；:1690 缺类归一；`tests/session.rs:2479`、:2514、:2536、:2549、:2566、:2585 覆盖比例/缺类/非法权重 | 已实现；零NPC与正float的提前拒绝为 G29；比例和≈1差异为 Q06 |
| initial spec §6 批次边界 | spec:105–109 | 无初始挂单在此批承诺；分配不扣现金、不改变 T+1；精确恢复见 `session.rs:2720`；挂单层独立处理交易单位 | 边界保留；lot 对齐并非本批要求 |
| initial spec §7 测试矩阵；plan Task4 集成 | spec:111–123；plan:432–509 | `tests/session.rs:2359` 至 :2585；:2614 集成真实持仓→注入NPC卖单→玩家买单→正常step，:2627 主动关闭随机arrival | 测试定义存在；现集成不再证明 NPC 自主产生第一笔成交，不能把注入 fixture 等同于旧自然策略测试；未据此认定产品缺失 |
| initial spec §8 文件布局、§9 DoD、§10 风险；plan Self-Review | spec:125–149；plan:513–525 | 所列模块/导出/外部测试存在；成本先checked再写仓；末账户/末类别余量使股数守恒 | 源码落实；clippy/build/全回归本轮未执行，不追认历史命令 |
| money Goal/Architecture/Global/File Structure | money-plan:5–30 | `money.rs:26` traits、:40 TS number、:41 serde transparent、:42 i64 newtype；`tests/money.rs` 外部测试 | 已实现；跨端范围归 Q01 |
| money Task1 错误类型 | :34–123 | `money.rs:10` ParseFailed/Overflow/InvalidRate，`tests/money.rs:6` Display 上下文 | 已实现 |
| money Task2 构造/访问/ZERO/traits | :127–211 | `money.rs:26`、:46、:49、:54；`tests/money.rs:26` 等 | 已实现 |
| money Task3 checked add/sub/mul_shares | :215–336 | `money.rs:63`、:75、:86；`tests/money.rs:51`、:66、:75、:82 | 已实现；u32→i64 使用无损 `i64::from`，不需要恢复旧 unwrap_or |
| money Task4 元字符串解析 | :340–488，尤其 :348–349 | `money.rs:99` 至 :203；`tests/money.rs:95`、:110 | 主干已实现；新增无数字小数点输入遗漏 N17-01 |
| money Task5 apply_rate / half-even | :492–630 | `money.rs:210` 非有限拒绝、:214 rate=1精确原值、:218 越界检查、:230 half-even；`tests/money.rs:139`、:151、:161、:172、:182、:198、:215 | 已实现；普通费率显式舍入，未将旧精确-5算成-4错误视为需求 |
| money Task6 裸 i64 serde | :634–700 | `money.rs:41`；`tests/money.rs:222` 往返、:235 裸整数 | Rust内部保真已实现；JS Number范围不一致为 Q01 |
| money Task7 导出/clippy/全回归 | :704–760 | `lib.rs:21` 导出 Money/MoneyError；`tests/money.rs:245` 根导入 | 导出已实现；本轮未运行 clippy/build/回归 |
| money Self-Review/Execution Handoff | :764–789 | 上述逐Task映射涵盖全部公开API；GameConfig独立实现且生产费用调用见后文 | 非独立产品功能；无漏映射 |

## G29 / Q01 / Q06 再核

- **G29 保留。** `session.rs:1097` 只用“存在正float”触发有效权重检查，没有“至少一名NPC”条件。NPC数全0时有效和为0，:1108 返回 InvalidSetup；:1652 的空NPC早退 :1661 因此前校验不可达。原文 spec:34 写“float>0且有NPC”才分配；plan:220 明确无NPC跳过。Random 没有该权重检查，合法同结构零NPC setup 可跳过。本问题是可配置边界，不描述默认NPC数量，不建议把股份赠送玩家。现测试 :2549 等检查“有NPC但全有效权重0”，不能覆盖此反例。
- **Q01 保留。** Money裸i64由 `money.rs:40`–:42 输出，Web `apps/web/src/host/protocol/wire-values.ts:48` 调 `signedSafeInteger`。真实caller包括 WASM `apps/web-wasm/src/lib.rs:29` 序列化、:332 setup反序列化、:355 snapshot、:483 save、:502 restore及:510 restore_json；server `apps/server/src/routes.rs:1405` 解码命令、:1487/:1530 输出publisher/baseline。Rust内部serde测试没有证明超JS安全整数金额可跨宿主读取。需要统一支持范围/无损表示，当前不把严格Web拒绝本身当错误，也不强迫库Money缩至JS范围。
- **Q06 保留并补历史证据。** 原 spec:32 要求比例和≈1；当前 :1090只要求单值有限非负，:1097只要求现存类别有效和有限且>0，实际 :1717归一化任意正权重。spec:45 均匀随机权重与当前 :1788 的Pareto有区别；ByKind散户 :1740 先40% eligibility筛选，再按kind指数分配。`git blame` 将eligibility/Pareto定位至 `3a58a9ff`（2026-09-09，提交说明 `feat: unify runtime updates and deepen market simulation`）；这是实现变更证据，不能代替用户批准依据。对全部 docs 搜索 `Pareto|pareto|eligible|float_allocation|ByKind`（排除原initial文档）未定位新的分布批准文本；测试 `tests/session.rs:2366` 明确锁定稀疏散户持仓。保留为政策确认项，不要求回退旧算法。

## 新增候选 N17-01：from_yuan_str 接受完全没有数字的输入

**建议合并为低优先级确定代码遗漏，和 Q01 不同。** money-plan:348 列合法 `.5`/`12.`，:349要求“非数字”Err；:389也承诺非数字防御。当前 `Money::from_yuan_str(".")`、`"+."`、`"-."` 走到 `money.rs:122` split_once后，int_part/frac_part同时为空，:143和:151仅在非空时校验数字，:159与:169分别以0处理，:203返回 `Money(0)`。外围trim后仍一样，因此 `" . "`亦被接受。

测试 `tests/money.rs:110` 当前只覆盖空串、abc、多点、多符号、非法尾字符、精度超限，没有“整数和小数部分同时为空”；实际 `.5` 和 `12.` 必须继续保持合法，只应拒绝完全无数字的文本。此结论由完整控制流直接确定，未执行临时测试，不声称实测通过。

**反证与范围：** 对 `packages/engine/src` 的实际调用检索仅找到Money自身定义；公司会计 `AccountingAmount::from_yuan_str` 为另一模块，不能混为本caller。当前Web金额解析也没有调用本Rust方法。因此该缺口影响公开Money解析API及未来调用方，不能宣称玩家输入框已经吞掉“.”。旧计划提供的解析代码本身也遗漏了该校验，但其行为承诺写“非数字Err”，应以承诺为准，不能用旧模板缺陷反证。

## 最新需求反证与跨层 caller

1. spec:20/:39的“精确存档也调用grant_position”不是当前缺失功能。恢复 `session.rs:2735` 使用restore_balances和:2743 from_restored_parts保留qty/t1_locked/invested/recovered。若改用grant_position，会把recovered和T+1锁清0、把历史累计成本改为单价乘qty。`docs/decisions/0019-draft-market-scope-and-capacity.md:33`–:39明确保留继续运行原始事实和T+1；当前专用恢复通路符合该较新契约。
2. plan Task1:79–81示例 `.checked_mul(...).unwrap_or(i64::MAX)` 违反自身铁律二与spec:101。实际 `account.rs:264` 调Money::mul_shares再写仓，先失败再返回，明确强于旧示例，无遗漏。
3. plan Task4:465–472旧负数拼接算法未正确对整数部分施负号；实际 `money.rs:183`先计算i128绝对值、:190整体施符号、:199检查i64范围，`tests/money.rs:98`–:100已含-1/-1.50/-12.34。因此不应恢复历史示例。
4. **费用确实有生产caller：** `config.rs:212`佣金使用Money::apply_rate再取min，:229卖方税，:234双向过户费；`account.rs:311`/:312买方费用，:420–:422卖方费用；`session/pipeline/transition.rs:156`–:183订单累计费用增量和结算；`session/envelope_projection.rs:214`–:223挂单投影；`session/persistence/v2.rs:664`–:673恢复费用校验。库级apply_rate并非仅写完无人调用。
5. 银行家舍入对应游戏费用精度；`docs/trading-rules.md:122`明确真实券商个性化佣金舍入不在范围。印花税现行0.5‰、过户费0.01‰记录在:41，实际config默认:193及:235与之对应。该审计未修改真实交易规则，未重新开展官方网页核验，也未声称官方规则已当日重核。
6. **市场转活旧50步随机验收：** 当前 `tests/session.rs:2614`用确定性fixture证实已有初始股份能卖、能正常撮合，但:2627关闭random arrival，不能证明策略自动生成首笔交易。较新ADR-0024与open-questions Q12允许零成交，不恢复“保证持续成交”的隐含承诺。可将旧自然策略验收变更作为测试证据限制记录；目前没有证据显示初始持仓生产功能未实现，故不新增产品G项。

结论：本范围已有缺口 **G29**、政策项 **Q01/Q06**仍有效；新增 **N17-01**（公开Money解析API的无数字文本被静默解释为0）。三文档全部章节/Tasks均有映射，无新增整块未实现功能结论。
