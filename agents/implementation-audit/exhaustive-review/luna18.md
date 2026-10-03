# Luna 18：market / Money 旧设计与现行实现复核

## 基线与全文读取

- 产品基线 08e4fc7；审计 HEAD a7c7ce3（审计 merge，产品源码同基线）。本轮只读产品文件，只新增本记录；没有 Git 写操作、产品修改或测试/构建。
- 已读取根 AGENTS.md 与 docs/principles.md。三份指定文档连续从首行读到 EOF：market plan **663 行**、market spec **152 行**、money spec **113 行**，总计 **928 行**。旧实现与当前产品契约不一致处，以正式更新的 docs/trading-rules.md 和后续决策为准，不把旧计划复活为要求。
- 本轮沿 Market 停止价、价格笼子、委托拒绝、撮合、开/收盘竞价过渡、日终清簿、Money 错误及调用者追踪；未重查交易所网页，交易制度依据按现行规则文档登记（规则原文链接与最近核对日期见 docs/trading-rules.md:97-110）。

## 全文章节映射

| 文档章节/原文行 | 对应当前实现与 caller | 复核结论 |
|---|---|---|
| market plan Goal/Architecture/Tech Stack/Global Constraints/File Structure :5-31 | market.rs:49-63 仍以 OrderBook 和整数分价格表示单股市场；隐藏 V 已由后续 ADR/公司信息与个体判断架构取代（旧记录 sweep18、luna05、sweep21）。Market 不负责 account 结算。 | 单股市场容器保留；共同 V、VParams、evolve_v 与 V 专用舍入函数不应恢复。旧 plan 的 apply_rate 涨跌停舍入被当前按 tick/bps 的价格规则取代。 |
| plan Task 1 :34-143 | market.rs:18-39 有涨跌停、OrderBook/Money 透传与当前构造/价格状态错误；lib.rs:52-53 导出 Market/MarketError。 | 类型功能对应已实现；V 专属类型/错误分支因 V 删除而移除，属契约替代。 |
| plan Task 2 :147-282 | market.rs:93-124 校验比例、精确基点、初始价及 tick；:127-238 计算涨跌停及笼子界限，i128 checked 结果回 Money。越界返回 Result，如 market.rs:128-135,210-238。 | 构造/边界路径已演进为整数 bps 与 tick 对齐；非有限比例被 (limit_pct > 0 && < 1) 拒绝。正常代码不靠 Money::apply_rate 生成涨跌停价。 |
| plan Task 3 :286-398 | market.rs:287-320 公共 place / 私有 place_recording 共用 place_inner；:300-319 先校验日涨跌幅、再撮合、末笔成交更新价格。连续竞价 caller 在 continuous_matching.rs:449-502 预检笼子并处理 LimitExceeded 拒单；orderbook.rs:338-474 校验订单、价格 tick、按 price-time 撮合并产 MatchResult。 | 越界价格先拒、不会改簿；book/money 错误显式透传。撮合成价由 maker price 决定。连续竞价另有配置笼子边界，当前规则明确笼子只适用于连续竞价。 |
| plan Task 4 :402-539 | 现行 market.rs 不含 V 字段或演化代码。 | 已由后续产品决定取代，非遗漏。 |
| plan Task 5 :543-638 | market.rs:448-450 日终将 last_close=last_price 并清当日挂单；深度由 :453-468 透传。auction_day_end.rs:1422-1425 记录竞价 clearing price；:1461-1477 收尾 DayEnd release 后调用日终；开盘余单滚入连续簿见 :1598-1628。 | 旧“盘口跨日保留”措辞已由当日委托日终失效的现行契约替代；收盘竞价价更新与清簿顺序可追到调用链。 |
| plan Self-Review/已知风险 :642-663 | 当前价格边界是 Result，不再依赖 up_stop/down_stop.expect；Money 舍入只在 money.rs:210-247，且无旧 round_half_to_even_f_to_i64 / None => 0。 | 旧风险核销；不能将旧实现样例视为当前代码。 |
| market spec §1–2 :10-25 | 范围、撮合驱动 last price、闭区间涨跌停见 market.rs:279-319；规则差异见 docs/trading-rules.md:18-43。 | 单股封装与拒单语义保留。真实板块差异在显式股票配置；不能据旧 10% 示例推广全部股票。 |
| market spec §3 :27-75 | Market、MarketError 和访问器见 market.rs:18-63,93-124,241-255,287-345,453-468；取消通过受控 cancel (:439-442)，不是旧 book_mut。 | API 有演进但能力在；无须恢复旧 book_mut 或 V 访问器。 |
| market spec §4–5 :77-101 | 日涨跌停 market.rs:127-135,210-238；越界处理 :295-319；Money 定点定义 money.rs:24-42。 | 当前比例按精确 bps 校验、边界按最小价位计算；和旧 spec 的 Money half-even 比率乘法不同，但现行 trading-rules 与 tick 语义优先。 |
| market spec §6–10 :103-152 | 竞价在 session/pipeline 编排；规则标出深市竞价参照仍以昨收的简化 trading-rules.md:30-35；测试现存于 market.rs:472+ 与各 pipeline tests。 | 集合竞价已实装，不再属于“延后”遗漏；深市差异简化已正式登记。验收命令本轮未执行。 |
| money spec §1–2 :10-23 | Money 内部 i64 分 money.rs:24-42；构造比例桥接 :44-94,206-225；各业务费率 caller 位于 GameConfig 与 settlement（旧 sweep17 已逐项列明）。 | 货币单位、整数权威值与银行家舍入在费用路径仍有效；涨跌停 caller 已转至 market 的整数 bps/tick。 |
| money spec §3–4 :25-61 | checked 加减乘股 money.rs:58-94；字符串解析 :96-203；GameConfig/交易规则适用范围见 trading-rules.md:18-49。 | checked 运算具名返回错误；旧 parser 合法 12.、.5 行为保留；无数字输入候选见下节。 |
| money spec §5–6 :63-75 | serde 裸整数 money.rs:36-42；apply_rate 非有限及溢出拒绝 :210-224；负数半偶算法 :228-247。 | 序列化与错误语义已实现；跨 Web 安全整数范围问题为既有 Q01，非本轮新发现。 |
| money spec §7–9 :77-113 | 现有解析/舍入/溢出测试见 packages/engine/tests/money.rs:95-220；格式测试并未覆盖单独小数点；根导出与 Money 类型已有。 | 主要矩阵存在，留意 N17-01/S18-01；本轮未运行测试或追认历史 PASS。 |

## 交易链复核

- 日涨跌停对所有 Market::place 调用生效：market.rs:295-319 取得 up_stop/down_stop 并以闭区间拒单。连续竞价价格笼子则由当前连续 worker 在撮合前显式校验 continuous_matching.rs:449-475；文档限定连续竞价 trading-rules.md:36-40。最高/最低符号限价根据方向与笼子配置解析在 market.rs:168-207，行情快照调用边界见 session/views.rs:14-15,93-105。未发现需把旧 Market::place 强行合并笼子或跳过其调用者校验的证据。
- 订单簿自身拒绝重复 ID、非法进度、非正价/非 tick 价，随后 maker-price 成交并 checked 更新成交金额/量，见 orderbook.rs:346-474。place_recording 仍复用相同撮合内核，只额外记录 maker 原像供事务投影。连续成交更新 last_price 由 market.rs:315-318；竞价单独应用 clearing price，再按阶段决定开盘余单转簿或日终清簿，见 auction_day_end.rs:1422-1425,1473-1479。
- 日终取消清簿与 docs/trading-rules.md:30-32,44-49 的未成交单失效及释放占用契约相符；交易簿 clear() 仅清未成交挂单（orderbook.rs:627-630），不据此推断成交身份历史也清零。
- 现行停止价 price_bound 的 half-up tick rounding 是明确实现；money apply_rate 的 half-even caller 留在金额/费用，不混淆价格 tick 舍入。未发现新边界测试遗漏足以构成单独交易缺陷；本轮没有官方网页复核，若需更改价格舍入规则，应先基于当前交易所规则资料重审。

## 旧结论核销与未结候选

- 旧隐藏 V / VParams / evolve_v / V 模块舍入器：核销，后续决策已删除共同 V；旧文档中的示例不构成待实现需求。
- 旧涨跌停使用 Money apply_rate 半偶舍入：核销为当前 tick/bps 实现；正式 docs/trading-rules.md 为较新规则依据。限价符号解析、价格笼子、撮合拒单、日终清簿均核到生产 caller。
- 旧 money 示例“负 5 分舍入为负 4 分”：按负数最近偶数计算应为 -4；现有测试记录见 tests/money.rs:172-177，代码一致，不是当前实现缺陷。
- **N17-01 / S18-01 仍成立，去重不另编号：** money.rs:121-153,158-203 对 .、+.、-.（以及外围空白）使整数/小数部分都为空，两个“非空才校验”分支跳过，最终返回零。Money spec :43,:72 要求非数字文本拒绝；计划明确接受 .5 和 12.，但不能据此接受没有任何数字的“.”。tests/money.rs:110-135 缺该反例。旧审查 sweep17.md 与 sweep18.md 已记录同一遗漏；当前全局 caller 检索只发现 Money 自身 API/测试/历史计划，未发现 packages/engine/src 的业务调用，所以影响限于公开解析 API及未来调用方，不能夸大为现行下单或玩家资金入口漏洞。建议并入既有候选处理，不作为本轮新候选。
- Money 其余检查：add/sub/mul_shares checked；apply_rate 对非有限率、结果溢出显式 Err；舍入负数对称，没找到额外拒绝路径或 Money 数值漏项。现有跨端 JS 安全整数候选 Q01 不在本轮重新定级。

## 结论

当前 market 实现与现行市场规则、撮合、日终生命周期的主要调用链对齐。共同 V 及旧价格舍入约定已替代；没有发现新的可确认 A 股语义遗漏。仍应保留既有 Money 无数字字符串候选 N17-01/S18-01，等待总审计合并处理。静态复核不等于测试、官方来源重核或整体验收通过。
