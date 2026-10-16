# 有意分歧与登记缺口总账（std-thread 迁移收官）

> 2026-10-17 汇编，供 **Sol 终验收**（flip-runbook §4 逐条裁定）与 **批 10
> trading-rules 同步**使用。只读汇编，未动任何入库文件。
> 来源：`.tmp/` 各 verdict/台账（multi-tick-design.md、d5-triage.md、
> handoff-*.md、sol-*-verdict.md、flip-runbook.md、semantic-traceability.md）+
> `docs/trading-rules.md` 现有登记 + `docs/decisions/0042`。
> 状态标记：`[docs]`=已入正式文档；`[台账]`=仅 .tmp 台账登记；
> `[批10]`=未入正式 docs，批 10 需同步；`[已收口]`=曾是登记差异，已修复闭合（Sol 核对收口提交即可）。
> 行号基于当前 HEAD=bb00c048。

---

## 一、有意分歧（新引擎≠旧引擎，有登记依据）

| # | 分歧 | 来源 | 依据/裁定 | 登记位置 | 状态 |
|---|------|------|-----------|----------|------|
| A1 | **Trade maker/taker 字段语义=腿方向**（maker≡卖腿/taker≡买腿；maker/taker_order_id、filled_value_before 三对字段成组互换；旧管线=到簿序"主动/被动"） | ADR-0042；d5-triage 拍板清单 #1（ed3bacc3）；multi-tick-design Sol 六审；desk.rs:1109 备忘 | A 股无 maker/taker 费差（佣金双边/印花恒卖腿/过户费双边），腿方向无歧义且费用归户唯一化；前端消费面全量扫描零适配（.tmp/maker-taker-consumer-scan.md） | docs/decisions/0042-trade-maker-taker-leg-semantics.md（accepted 2026-10-16） | **[docs]**；trading-rules.md 未加指针——批 10 可补一行引用 |
| A2 | **日终未成交单失效=显式 OrderCanceled 事件**（旧管线静默 clear、资源退回无事件；清簿事件形态同族：desk drain 的 Entity::Closed/终结事实显式上行） | sol-flip-review-verdict（方向维持 A2）；flip-runbook §4.3；翻转批裁决 2 | 失效是真实交易制度事件（沪深收盘后订单簿清空）；显式呈现铁律；资源语义两侧等价（退款退股一致） | docs/trading-rules.md「日终未成交单失效」（2026-10-15 登记） | **[docs]** |
| A3 | **自成交限制=撮合引擎硬排除**（新引擎独有：(a) 拍卖同主配对撤卖侧保买侧、可配对量<选择量、公开量=实际量；(b) 连续竞价 taker 交叉档含自单终止撮合——游戏额外限制，会阻断第三方，与旧管线不等价；条款版本未核验声明保留） | sol-flip-review-verdict P1-1/维持裁决；sol-recheck-verdict（"等价双撤"结论撤回、trading-rules 登记合格） | 真实沪深由监控/警示处理异常对倒、引擎不硬拦截；本项目实现为游戏额外限制（如实标注）；"深交所 2025 修订"援引矛盾已撤回 | docs/trading-rules.md「自成交限制」（2026-10-15 登记，Sol 终审修订） | **[docs]** |
| A4 | **desk 分段记账口径**（每笔成交累计费用分项 charged=Σ分笔名义费用、每笔可各自触最低佣金下限；买腿 purse 扣减=最坏情况预留的边际口径；分段累计≥累计式 nominal；实扣以 purse 为准，分段承载存档审计 fault2228 下界+宽松上界[bb00c048 真界=nominal 费率部分+n_max·min，n_max 按最小手额推导]；旧管线 ledger=累计式 nominal(Σ)） | d2116991/26cfac23/bb00c048；D5 终审 1 Major+1 Minor 处置 | A 股双边佣金/过户费+印花恒卖腿制度面；口径差在存档校验分段形态显式建模 | docs/trading-rules.md「同族补充（desk 分段记账口径）」（2026-10-16） | **[docs]** |
| A5 | **散户经历成交事件面迁移期简化**（runtime 组内不承载 consecutive_failed_buys/post_exit_cooldown/持仓峰值参考权益反馈——旧管线入口在结算投影 record_fill 家族，runtime 下不运行；观察面与决策输入 dated 快照已承载；业务 hash 不含经历行域，对拍结构性盲区，回归以专项测试承载） | D5 第 11 任独立复核 Major 2（3dd2583b 登记） | 迁移期如实登记的功能缺口（非永久简化） | docs/trading-rules.md「散户经历反馈的成交事件面」（2026-10-16） | **[docs]**；专项测试仍欠（semantic-traceability R31 runtime 零覆盖） |
| A6 | **frozen_cash=0**（runtime 所有权转移口径：预算现金已扣在途、组态无冻结现金域；旧管线 escrow/envelope 预留口径。三层现金语义=可分配预算/总现金观察/desk 实占） | multi-tick-design D3a-2c 登记差异 #3；sol-mid-d3-verdict 裁定"可接受（再减=重复扣）" | 用户资源实体模型拍板（memory：现金/持仓可转移实体消除共享写） | .tmp/multi-tick-design.md + Sol 裁决 | **[台账][批10]** |
| A7 | **复审触发=due 拍子集**（runtime 计划复审/报价只在链 due 拍的 run_chain_decision 内跑；旧管线每拍复审。record_review/quiet 门控已移植对齐复审语义本身，但触发拍集合仍是 due 子集——决策频率结构不同；首子单 urgency=initial+pause=Unavailable 已随 D3b 件 4 收口） | multi-tick-design D3b 各节（件 4 接线+record_review 根因）；d5-triage #2（"组内每 due 必决策"叙述；evaluate 公式已真迁 fb39814e 消除公式差，触发拍面仍异） | 组线程调度模型固有（一股一持久线程+内联决策） | .tmp/multi-tick-design.md / d5-triage.md | **[台账][批10]** |
| A8 | **civil 日历推进编排**（runtime=world 自动日结（run_ticks 内 (tick-1)%tpd）+driver.day_end 仅 civil 桥接，last_day_end_tick 守卫显式互斥（重复手动=Err/自动跳过/Abort 回滚清空）+Drop 兜底关停；旧管线=宿主显式 end_civil_day。恢复契约=ADR-0025 日界静点；WorldSpec.initial_committed_tick 贯通相位） | multi-tick-design §1/§5 风险 1 + D0 实施记录 | ADR-0025 日终存档/静点契约对齐；双触发=不可解释行为，显式互斥（铁律二） | .tmp/multi-tick-design.md D0 节；ADR-0025 相关但编排差异未单列 | **[台账][批10]**（ADR-0025 补记或 trading-rules 存档节注记） |
| A9 | **集合竞价公开盘口不揭示（attention 可见性门禁）**（desk 广播门禁：CallAuction 公开簿空、ClosingAuction 揭示 public_cutoff_serial 前老单、PreOpen/Continuous 全公开；session desk_book_mirror 瞬态镜像+截点跨档链；这是真实交易制度面——A 股竞价时段不揭示买卖盘） | handoff-d5-finish §5.5（协调者明示翻转前硬门）；handoff-d5-11th-r2（c1e5dd62 真迁） | 真实沪深集合竞价不揭示买卖盘（与 R08 窗口面同族但公开面语义此前未登记） | .tmp 交接文件 | **[台账][批10]**（trading-rules R08 补公开面语义） |
| A10 | **bound_fallback ±10% 近似**（runtime 镜像形态下 StockView 的 daily_upper/max_buy/min_sell 无 desk 精确源（desk 广播 StockPublic 有），views.rs 以边界回退近似——待投影收口） | handoff-d5-11th-r2（Minor 4 登记） | 已知近似如实登记；attention 断言面不涉及 | .tmp/handoff-d5-11th-r2.md | **[台账][批10]**（近似声明） |
| A11 | **投影权威策略：空簿不覆盖/terminal 覆盖**（driver 计划/个人态投影：plan_ids 非空即装+terminal 事实覆盖 session 旧 Active 镜像；空簿保护=迁移期 session 注入面保护） | multi-tick-design Sol 中点整改 4/三审 3/八审收官件 | 权威状态控制（组=权威、session=镜像），防旧镜像滞留 | .tmp/multi-tick-design.md | **[台账]**（机制细节，批 10 低优先） |
| A12 | **wasm32 保持串行旧管线（方向 A）**（engine wasm32 编译：world wasm 门控移除+session 在 wasm 上 runtime_driver=None 走串行管线；Web 目标 Stage 1 形态） | c5bb4a0b；.tmp/wasm-gap-verification.md；handoff-d5-finish §6.2 | std::thread 无法跑纯 wasm32；用户拍板方向 A（批次盘点 §3 产品级决策） | .tmp/wasm-gap-verification.md | **[台账][批10]**（ADR-0005/0027 补记候选；非 trading-rules 域） |
| A13 | **NPC 到达不再保证成交/构造期观察面差异**（runtime 等价锚=restore 双轨对拍，不逐位对拍旧管线——构造期观察面（首拍空簿快照）与 session 簿 signal 面不同；风控行为不被到达门控，"到达率 0 零单"类断言需收窄为买入受理） | handoff-d5-11th-finish §5.3 | 迁移期测试口径如实登记 | .tmp/handoff-d5-11th-finish.md | **[台账][批10]**（测试口径声明，低优先） |

**已收口（曾是登记差异，Sol 只需核对收口提交，不计待同步）：**

- 在途卖单 qty 口径差 → 收口=含在途读面统一"在手+在途卖余量"（Sol 中点整改 3，含 purse 边际费用口径——7106eb79/740e4b3e）。
- 零量条目面（组侧无"曾持有"）→ 收口=ever_held 三处置位（3643ebaf）+清仓回写删条目（c8b6c263 03-flat）。
- exposed_stocks 空集起步 → 收口=exposed_stocks_shared 共享（798455d6 整改 5）。
- 买腿成交落账一拍差 → 裁为必修等价缺口并修复=本拍公开期末投影 apply_same_tick_fill_projection（740e4b3e，sol6th 双测试）。
- epoch/attention evaluate 概率门 → 真迁同式（fb39814e：signal 概率+observe 门+wait 采样+共享 rng 续传）。
- 成本基保留会话侧（D1 登记）→ 闭合=GroupAccount invested/recovered+回写同步（6a5ed768）。
- live purse 按 desk 实际占用（价优差额）→ 修复（Sol 三审 1，01-* 红绿）。

---

## 二、登记缺口（旧引擎缺陷，随旧管线退役消亡）

| # | 缺口 | 来源 | 处置 | 登记位置 | 状态 |
|---|------|------|------|----------|------|
| B1 | **自成交限制缺失**（旧管线撮合引擎对同账户买卖委托互相成交不做任何排除——真实沪深虽非引擎硬拦截但游戏裁定应有限制；旧管线 fixture 曾依赖同账户自成交设计，自成交拍板 A 后双轨 fixture 化） | d5-triage「自成交拍板」节（desk.rs:944 索引路径 vs 旧 fixture）；sol-flip-review/recheck | runtime 补硬排除（见 A3）；旧管线缺口随退役消亡，不再回补 | docs/trading-rules.md R01（依据状态段隐含"本项目实现为硬排除"） | **[docs]**（建议批 10 把"旧管线无限制"一句写实，供退役审计） |
| B2 | **日终失效静默不可见**（旧管线失效无事件、用户不可见——显式呈现铁律下的可观测性缺口） | sol-flip-review-verdict（A2 方向）；trading-rules.md R02 | runtime 显式 OrderCanceled（A2）；旧管线形态随退役消亡 | docs/trading-rules.md（"旧管线静默 clear"叙述） | **[docs]** |
| B3 | **maker/taker 字段语义误导**（旧管线 Event::Trade 字段名承诺"主动/被动"但 A 股无对应费用/结算差异；Sol 在 D3a 复审指出 DeskTradeFact 字段名承诺了它没有的语义——旧管线同病） | ADR-0042 上下文/备选方案 B 否决理由 | ADR-0042 统一腿方向收口（A1）；前端可从 taker=买腿获真实主动方向（登记为后续增益项——价格对比染色近似仍在） | docs/decisions/0042 | **[docs]** |
| B4 | **挂单后现金变动拍中透支（staged 重验防御）**（旧管线 NPC 准备域需"队列批在下一拍重验现金"防挂单后现金变动致拍中透支；runtime purse 所有权模型下单即扣、从根上消除该缺陷形态） | npc_tick_preparation_tests.rs:348-352 pin 注记；d5-triage #5 | 缺陷随退役消亡；等价防御=组端 purse 校验 | 代码 pin 注记+.tmp 台账 | **[台账][批10]**（候选：trading-rules 资源占用节补一句"purse 所有权消除跨拍透支重验"） |

**非交易语义债（备注，不入 trading-rules）：**

- simulation-diagnostics 面自 c79f051c 起编译失修 17 错（先于本批）——翻转批后单独立项（d5-triage D 登记；handoff-d5-finish §6.4）。
- causal/aggregate 主动腿选腿修复（ed3bacc3 复核阻断修复）无测试覆盖，逻辑审读背书——登记待补测试（d5-triage）。
- verification_evidence 证据 JSON 基准在卖主动场景互换，基准重建须重算——ADR-0042 后果节已登记 **[docs]**。
- Event.ts 重生成（注释变更）待翻转批统一做（handoff-d5-finish §6.5）。

---

## 三、pin 等价锚（旧管线专有机制——runtime 下跳过，锚点注记）

> 判定两式并存（翻转批必须统一）：既有 `var_os("ENGINE_STEP_RUNTIME")` "存在即跳过"（=0 时生产走旧管线但被静默跳过——语义差）与新式 `step_runtime_enabled()`。当前 32 处 skip 站点=18 处 step_runtime_enabled+14 处 var_os（1047e693 登记在案）。
> 交接计数"12 例"与代码注记站点有出入（首批 bd3aa85a 曾报告失实、6e265f92 补齐）——**Sol 以 `ENGINE_STEP_RUNTIME=1 cargo test -p engine --lib -- --list`/实跑名单为准**。

| # | pin 组（例数） | 机制（为何旧管线专有） | 等价锚 | 代码位 | 提交 |
|---|---------------|------------------------|--------|--------|------|
| C1 | committable_state ×2（defer_swap/Job 计数） | 旧管线关键路径 tick shadow 状态转移计数——std-thread 所有权模型无 shadow 交换，机制面不存在 | 翻转批 batch1-5 tick/账户/事件对拍承载权威等价 | session.rs:5193/:5265 | 6e265f92 |
| C2 | npc_tick_preparation ×2（completion receipt 溢出 fault1004；队列批下一拍重验现金） | 旧管线 staged 准备域注入/检视面——runtime 组内决策不经 ingress cursor，staged 仅构造期消费一次 | 组端 Abort/undo 可逆日志（batch4 civil 回滚全量对账）+purse 所有权消除透支+batch1 现金对拍（ADR-0022 尺度） | npc_tick_preparation_tests.rs:303/:348 | 6e265f92（bd3aa85a 首登） |
| C3 | plan_root_quota_wave ×1（staged 注入事务原子性） | staged handover 桥专有（runtime 首步消费面一次性，注入中途失败重放不适用） | 组端 Abort/undo 可逆日志（batch4 civil 回滚全量对账）承载原子性 | plan_root_quota_wave_tests.rs:1218 | bd3aa85a/6e265f92 |
| C4 | npc_request_cash ×1（staged_decision 桥+ADR-0022 断言） | staged_decision 桥"驱动中检视"形态 runtime 不适用 | 组端 purse 校验+qty0 拒收承载下单侧防御；ADR-0022 尺度复核面=batch1 现金对拍 | authoritative_tick_tests.rs:270 | bd3aa85a |
| C5 | quote_expiry ×1（post_shadow_failure 注入） | 旧管线 tick shadow 提交段失败重放面——runtime 无 shadow 提交段 | 组端 Abort/undo 可逆日志（batch4 civil 失败回滚全量对账） | quote_expiry_tests.rs:458 | 6e265f92 |
| C6 | book_auction_boundary ×1（账户校验信封"密封批预算"） | 旧管线 staged/envelope 批量装配段预算复用面——runtime 无密封批（purse 按单扣减、撤单即释放，无批预算再分配） | 组端 purse 校验+Abort 可逆日志承载下单/撤单资源语义 | book_auction_boundary_tests.rs:251 | 6e265f92 |
| C7 | decision_chain restructure ×1（commit_injected_plan_roots 注入） | 旧管线 tick 事务注入面（runtime 形态 session 旧簿恒空使 ledger/book 对拍崩）；组内拒收回执消费面未迁移 | 组内定向撤单路由的不存在单拒收（d3b3） | decision_chain.rs:5597 | 1047e693 |
| C8 | candidate 证据面 ×2（step_with_commit_evidence 注入失败回滚；auction_finalizers 证据计数） | candidate 投影契约恒走旧管线为 fallback——runtime 失败回滚=组端 Abort、竞价编排权威=desk 相位面 | batch4 civil 失败回滚全量对账；desk 竞价语义（desk_semantics_tests）+翻转批对拍 | authoritative_tick_tests.rs:619/:704 | 1047e693 |
| C9 | ordinary_adoption ×2（外来单认领计划腿——record_child_submitted 普通来源入口/超出余量拒绝） | 旧管线计划委托域采纳防御——runtime 外来单走独立路径，计划子单由组内链 chain_quote_children 管理，无认领面 | 组内计划链余量推进（solmid 独立源对拍）+desk purse 校验 | plan_owner_tests.rs:654/:683 | 7c1909e1 |
| C10 | live_plan ×1（身份链错位——save 镜像簿 serial 1 vs 计划 active OrderId(2)，fault1042 显红） | 计划在途子单 OrderId↔desk serial 身份链在多源挂单下错位、不可映射；修复需 desk serial 域全链审计（超半天） | session 计划同拍成交合入通道（组簿空 fallback，已落地）——身份链闭合后可转真迁 | continuous_tick_transaction_tests.rs:535 | d2116991 |
| C11 | cross_stock ×1（注入 OrderId 域与 adopt 重编 desk serial 不可映射；组内 MutatePlanBook 通道改造超半天） | 同 C10 身份链错位族 | 同 C10（通道已就位） | plan_owner_tests.rs:421 | d2116991 |

**D2 登记（显式走旧管线的契约入口——非 skip-pin，退役时随语义迁移，批 9 C3）：**

- `step_pipeline_contract`（failure.rs:738 doc：旧管线专有注入面契约测试入口——staged handover/旧簿 fixture 经 step() 混跑只会把注入面缺失误报为语义失败）。
- 调用点：shared_ingress.rs:263/471/515（staged handover）；quote_expiry_tests.rs:124（旧簿 fixture——锚=desk `orders_expire_at_ttl_with_entity_refund`+翻转批撤单面）。

**pin 处置归属**：pin 清单不入 trading-rules（代码注记+本台账承载）；翻转批第 1 件=判定式统一为 `step_runtime_enabled()`（含 =0 语义差）；批 9 旧管线退役时 C1-C11 机制面随语义迁移或删除（flip-runbook P5、批 9 计划 C3）。

---

## 四、批 10 同步清单（汇总）

**trading-rules.md 直接受领（6）：** A6 frozen_cash=0 所有权口径；A7 复审触发=due 拍子集；A8 civil 编排（或 ADR-0025 补记）；A9 竞价公开盘口不揭示；A10 bound_fallback 近似声明；B4 purse 消除跨拍透支重验（一句）。

**其他正式 docs（3，低优先候选）：** A11 投影权威策略（可留台账）；A12 wasm32 串行形态（ADR-0005/0027 补记）；A13 测试口径声明（可并入迁移收官记录）。另 A1 可在 trading-rules 加 ADR-0042 指针行（可选，不计入待同步数）。

**总账计数：** 有意分歧 13 条 + 已收口核对 7 项；登记缺口 4 条 + 非语义债 4 项；pin 11 组 15 站点 + D2 登记 5 处。已入正式 docs = 8（A1-A5、B1-B3）；批 10 待同步 = 9（A6-A13 共 8 + B4，其中 A11/A13 低优先）。

## 统计

- 有意分歧：13（已入 docs 5；待批10 8）
- 登记缺口：4（已入 docs 3；待批10 1）
- pin 等价锚：11 组/15 站点（代码注记承载；翻转批统一判定式；批 9 随语义迁移）
- 已入正式 docs 合计：8；批 10 待同步合计：9（含低优先 2）
