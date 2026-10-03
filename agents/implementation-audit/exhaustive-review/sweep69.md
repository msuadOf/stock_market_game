# sweep69：pipeline receiver 实施记录与后续承诺复核

产品审计基线 `b76ece3`；checkout `4ad5a2e` 为审计 merge、产品相同。只新增本文，未修改产品、未运行测试/编译、未执行 Git 写操作。本轮结论是静态 owner/caller 核对，不冒充重新执行历史验证。

## 全文阅读

| 文件 | 行数 | 范围 |
| --- | ---: | --- |
| `agents/oop-refactor-implementation/pipeline/continuous/implementation.md` | 44 | :1–44，所有四章、表格、测试与验收备注 |
| `agents/oop-refactor-implementation/pipeline/core-implementation.md` | 17 | :1–17，全部自然段 |
| `agents/oop-refactor-implementation/pipeline/projections/implementation.md` | 60 | :1–60，所有六章、表格、filters及待核对 |

三文件共121行，首行至EOF连续完整读取。AGENTS/principles 已在此前批次读完。补读同主题 summary 全文与 continuous/core 独立review，定位 resources/projections review 与最终validation里的特定case；不宣称全文读完全部验证JSON或所有其他review。

## 条款矩阵

代码路径除标明外相对 `packages/engine/src/session/pipeline/`。

| 原文 | 当前 owner/caller 与状态 |
| --- | --- |
| continuous:3、:5–13，A01 Processor、E03 fill builder | 已有生产接线：`continuous_matching.rs:250` 构造 `ContinuousStockRoundProcessor::new(...).run()`；:1016 调用 `ContinuousFillReceiptProjection::capture(...).run(...)`。states/ordinals/receipts 属单Place临时builder，逐envelope检查audit；:1146–1165仍委托唯一 `FillTransition::buy/sell`。旧cfg(test) fill_receipts不是生产漏迁。 |
| continuous:11、:21，N01数量与Submitted/Fill/terminal依赖 | `continuous_lifecycle_projection.rs:125–133` 先完成order_lifecycle_events再一次extend；:137私有Batch、:238/:248 emit_fill、:287 finish。所有临时依赖在Batch内，未引入第二套真实委托；只对同OrderId的诊断依赖整理，不代表同股票市场撮合优先改为ID排序。 |
| continuous:12、:19，R2-N06跨轮shadow/失败消耗边界 | `incremental_continuous_stock_shadow.rs:176–188` round错误令coordinator failed；:266–270 Rayon分股实际调用consuming shadow.apply_round；:349–373 consuming finish_for_tick拒绝failed、依次stock.finish、核对恰一fact/operation。每股:385–400移交market/ledger到Processor，Err后不提供局部重试；:455完成全簿/ledger检查。 |
| continuous:13、:23，R2-N04 DayEnd生命周期与交易日转换 | `continuous_tick_finalizer.rs:212–227`先日终LifecycleProjection后TradingDayEndTransition；:300–329严格按causal→parent→NPC→retail→checked event index，原部分candidate写入时点保留，外层丢candidate。`incremental_continuous_stock_shadow.rs:457–473`清簿前冻结last/bids/asks，再生成DayEnd release再end_of_day；不是清簿后读取空深度。 |
| continuous:17、:25，P0/P1/P4/P9、state/getters/candle/participant迁移 | private state已有、getter调用已迁；`adaptive_plan_chain.rs:55、59`供finalizer只读contains。fee/Money分与qty股保留；P4释放不加回密封预算。不新增制度、不因receiver迁移重开官方规则范围；适用ADR0017/0018现行接受顺序。 |
| continuous:27–40，新增和既有精准短filters | 源码测试入口存在，例如 `continuous_matching_tests.rs:1737` symbolic+两maker、`continuous_lifecycle_projection.rs:692` 混合首错、`continuous_tick_finalizer.rs:469、480`空日终/index溢出、`incremental_continuous_stock_shadow_tests.rs:1360`清簿前depth。最终validation含对应case（:1209、1284、1809）；本轮未执行，不将测试名称或源码视作运行结果。 |
| continuous:42–44，worker未测试/root需编译与独立review | 这是实施时状态，已有后续 `pipeline/review-continuous-core.md`：独立复核发现mod.rs两个旧state caller并修复复核关闭，保留静态review限定；`summary.md`实际验证章节记录默认及features cargo check、278唯一Rust短case。故不能仅据旧“root仍须运行”报当前未实施，亦不能据278代表性短case宣称全回归。 |
| core:3–5，PlanChainFactConsumption四私有集合与prepare/commit | `adaptive_plan_chain.rs:47–51`聚合operations/receipts/candidate_keys/sealed_indices；:80–115、117–146每fact先四身份集合再payload（:99、136），receipts检查在全部fact后。:603/705 prepare，:696/861在相应完整round投影/同步末尾commit_round。增量可丢弃，不提前把失败round写入正式consumed。 |
| core:7，NpcOrderLifecycleBook原Vec借用/global OrderId/三字段移除 | `quote_expiry.rs:22–74`只是窄编辑器借原Vec；ensure_order_absent:33全局ID检测，remove:56–59按account/code/order，due_at:63–69不排序；`session.rs:2377、2390、2512`真实注册/移除manager caller，`auction_day_end.rs:2163`DayEnd clear。无新serde/index。 |
| core:9，ExpiryOutput先加资源后append、失败局部边界 | `quote_expiry.rs:78–85`先entry.or_insert(ZERO)，再checked_add，最后push；溢出可保留零值entry但不append，未擅自增加局部原子回滚承诺。P0释放通过报价过期live影响下一P1快照，不把released_by_account再补一次密封预算。 |
| core:11–17，跨组getters、六既有test、两features、10000ms与独立审查 | caller/state封装已有；`quote_expiry.rs:302、333、350–364`等新增边界fixture存在，`adaptive_plan_chain.rs:1683–1706`prepare可丢弃/重复身份测试。后续review已说明迁移漏点关闭；统一summary记录多核与短测时限。worker当时不跑cargo诚实记录不构成持续缺口。 |
| projections:3、5–14，四动作/依据/单位/T+1/P1 | 已有对应owner，限定结构与原运算次序；`ExperienceUpdateMode`是私有投影控制模式，未另立制度或放宽真实T+1。SelfView观察可替换现金不成为P1可用预算；因它没有真实撤单不得把该加回量写进settlement。 |
| projections:20，10-A01 institution moment绑定 | `retail_projection.rs:332` institutional入口显式moment；:336–338 enum将moment与mode绑定；:529机构分支按该moment调用dated处理。Retail:519仍调用legacy record_fill_with_order，原范围明确Retail无moment，因此不会修复/核销既有总账G08散户dated生产缺口。 |
| projections:21，N11 AccountFillProjection及双阶段错误优先 | :405–414 Rayon各账户new/apply_order/finish；:418先collect全部即时Result；:421–423才检查deferred final_position_error。:438私有累积对象；:542/:547/:585仅from_restored_parts临时投影Position，无真实结算/T+1解锁；:624 finish只产生投影。不同账户早deferred错误不能压过另一账户真实fill错误。 |
| projections:22，N05 capture/consume同源experience/risk/SelfView | `decision_snapshot_capture.rs:171–185`先capture再consume，:186–193最后导出strategy；:273–286枚举绑定experience/risk/self positions，:305–336从同一市场价格与Position::sellable构造；:347仍先观察position；:400 SelfView先于:401–417 risk。非Retail有experience仍有positions/no retail risk，NoExperience分支另建SelfView。 |
| projections:23，N13 reserved/replaceable Money顺序与阶段窗口 | `decision_snapshot_capture.rs:430–445`私有owner先reserved加再可replaceable加；available_cash为sub→add。:457–459仅CallAuction前1/3可撤；:481 continuous仅Continuous可替换；:504 auction用该window。:512–518消费available现金并保留Money错误位置，非预先改变阶段受理集。 |
| projections:25–29，公开入口/临时具名输出/state/容器/participant迁移 | `account_settlement.rs:196`真实消费Retail投影，institution projection mode有独立入口；capture相关对外结构/输入未变。CapturedAccountObservation是瞬态输出，不新增save字段。candle/attention owner由CommittableSessionState维护，未凭仅搜索session一种receiver判定全部字段迁移。 |
| projections:31–50，13 characterization、既有断言与短filters | 测试源码保留包括 `retail_projection_tests.rs:528、624`final disagreement与processing首错；snapshot capture/receipt persistence/actual fee institution case已有入口；最终validation:1734记录institutional净费用退出case。无新增TDD红灯声明：characterization先写于已有正确行为，不代表曾红；本轮未运行。 |
| projections:52–60，静态format/fullread与root待核对 | 静态格式化/EOF声明是历史worker证据；后续 `pipeline/review-resources-projections.md:30、32` 独立核对N11首错/N05同源与时点，summary记录统一check与代表性case通过。未运行复杂回归/E2E/长期/性能矩阵及doctest在summary明确保留，不把本小批产品遗漏伪装成已验收全部。 |

## 新候选反证与残余

- “只有新struct、生产不调用”被Processor、builder、coordinator、finalizer、capture、settlement的真实caller反证，排除。
- “root待编译/独立复核所以仍未实现”是旧进度语句，后续review与统一validation已有；不重复新增实现缺口。审查报告明确有静态与运行证据范围区分，278为代表性case，不是全量业务回归。
- “临时Position::from_restored_parts等于绕过T+1”缺乏证据：它属于经验投影running_positions，真实Settlement仍使用权威账户、可卖/T+1，临时字段没有解锁入口。
- “SelfView replaceable现金回补本轮预算”缺乏证据：代码只是观察现金，P1 sealed resources独立，不因self-view算术产生钱或撤单。
- “每个worker Err必须保留shadow供重试”与原文不符：明确consuming失败丢candidate/coordinator失效，不能擅自补局部重试或不同失败状态承诺。
- 现行散户dated衰减缺口G08仍存在，重构明确保持Retail原writer；同源封装不能代替新业务接线。本批没有新候选，可见的后续承诺已由真实owner/caller或后续统一证据解释。

本轮未确认三份121行之外的新实现遗漏；未执行测试，也未验证本机GUI、吞吐或全量回归。
