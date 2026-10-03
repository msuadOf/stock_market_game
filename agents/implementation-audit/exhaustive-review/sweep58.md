# sweep58：OrderBook与地产owner迁移后续记录复核

源码对象 `b76ece3`（worktree审计merge产品树相同）。本批只写本审计文件，未修改产品、未执行Git写命令或测试。已读取根AGENTS/principles，正式领域范围优先于局部工作记录的结论。

## 全文读取

| 文档 | 连续范围 | 行数 | 全部章节 |
| --- | --- | ---: | --- |
| `agents/oop-refactor-implementation/domain/orderbook-review.md` | 1–88 | 88 | 结论、三门、F1、跨组caller初次待办/追加核销、指纹/执行边界 |
| `agents/oop-refactor-implementation/domain/real-estate-result.md` | 1–69 | 69 | 五动作、测试状态、语义失败面、未完成门禁、RE-R1及独立复核回填 |
| `agents/oop-refactor-implementation/domain/real-estate-review.md` | 1–96 | 96 | RE-R1、三门、验证限制、修复后复核、最终diff/文件绑定 |

共253行，已补读首次输出截断的real-estate-result全文；不以摘要代替正文。额外只读核对`domain/final-summary.md:1`至23的后续门禁汇总。

## OrderBook记录条款矩阵

| 原文行号 | 当前实现/调用位置 | 判断 |
| --- | --- | --- |
| 5–8：baseline、五项动作与完整审查范围 | `packages/engine/src/account.rs:95`、`market.rs:245`附近、`orderbook.rs:284`及`orderbook/book_state.rs:10`/`persistent_index.rs:6`实际类型 | 五项不是只有记录勾选，产品owner及真实caller存在。记录指向的旧challenge/action-index路径当前缺席，保留为历史引用限制，不把它当产品未实现；`domain/completion-ledger.json:339`等保存动作映射。 |
| 16–18：Money分、股数量、价时优先/maker价、身份树不决定交易优先、T+1/费用不变 | `orderbook.rs:400`取maker及其price、`:424`min qty、`:428`checked成交金额；`book_state.rs`持价格/seq索引；`persistent_index.rs:6`身份树独立 | 当前生产路径保留，不新增交易制度；共同单位/简化以正式trading-rules为准，不从review推导全部交易所模型。 |
| 22：A03 Account/Position私有化、无第二Wallet/Portfolio | `account.rs:95`Account、`:103`私有AccountState、`:124`cash、`:128`strategy、`:132`positions、`:141`restore_balances、`:152`restore_strategy，Position位于`:609` | 已实现；恢复入口不是新资产证明机制，仍先由save validation拒绝非法状态。 |
| 23：N02 Order只负责数量进度/checked值 | `orderbook.rs:181`validate_progress、`:206`filled_value_after、`:210`filled_qty_after、`:356`入口消费 | 已实现，未在Order owner新增时段/账户/FIFO规则。 |
| 24、28：N03 Market写口收窄与公共RustAPI有意迁移 | `market.rs:245`附近恢复入口、`:265`apply_auction_price为crate；`:269`fixture_set_last_price及`:274`fixture_set_last_close仅cfg(test) | 已实施，异常测试不能成为恢复public任意setter的理由；仓库外API兼容属于有意收窄，不是假定待实现兼容层。 |
| 25–26、35–37：BookState owner、OrderIdPersistentIndex去重复，保留失败次序/COW | `orderbook.rs:284`state owner、`:431`maker金额先checked、`:432`taker金额checked后`:438`更新maker；filled_orders.rs:5/resting_index.rs:13均组合同一persistent_index | 已实施；旧局部部分失败明确保留，不自动增列“此重构必须使裸OrderBook完全原子”的需求。tick rollback由外层候选提交保证，不能混淆。 |
| 32–34：账户Arc COW、restore后校验、AccountBook缓存失效 | `session/account_book.rs:131`get_mut失效及`:161`values_mut失效；`session/persistence/v2.rs:436`通过restore_strategy装回 | 生产caller已迁移，缓存边界存在；无AccountState第二可写owner。 |
| 38：边界新增测试且review未运行 | `orderbook/state_contract_tests.rs`身份/FIFO/clone/溢出/部分失败用例；`:171`合法u32界限 | 保留直接边界保护；本轮只读，不把测试文件存在称为本轮通过。 |
| 42–46：F1 taker Money overflow测试遗漏已修复 | `orderbook/state_contract_tests.rs:78`双side、`:91`taker i64::MAX、`:94`精确add/MAX+2、`:99`盘口/filled/cursor不变；产品`:431`maker先check后taker | 当前F1已修，不能重复列新测试遗漏。正常输入taker qty进度保持，filled_qty overflow不可达解释仍成立。 |
| 48–63：初次caller清单及追加核销 | `persistence/v2.rs:189`/`:282`/`:301`strategy getter、`:436`restore_strategy；`failure_tests.rs:131`fixture；`pipeline/decision_resources_tests.rs:338`等fixture；`account_book.rs:144`positions/T+1 getter | 初次待办已迁移，最新核销与当前源码一致；旧行号变动不等于漏接。 |
| 65–71：Market三例迁lib，原integration十例剩七，必须跑lib+integration | `market.rs:473`price_limit_state_tests含三指定case；`tests/market/price_limits.rs:8`至`:139`七case保留 | 搬移完整已核，不能以旧integration例数减少误报删测试，也不能只执行integration证明全部覆盖。 |
| 73–88：最终指纹与仅静态限制 | 本轮sha256sum确认market.rs=`00e2ccdb…8c9`、state_contract_tests.rs=`5131bafb…a9e`与原表一致；`domain/final-summary.md:11`搬移核销、`:13`后续构建/check、`:15`104短case历史报告 | 早期“集中编译待做”已被后续汇总补证，但只引用历史报告，不宣称本轮执行或全回归。 |

## 地产两篇逐章/逐动作矩阵

| 原文锚点 | 当前真实caller/代码 | 判定 |
| --- | --- | --- |
| result:13、review:36：N04自身开局行/首错 | `real_estate/config.rs:59`check_opening_lines；`mod.rs:82`new先policy→max_projects→`:89`opening→`:91`post→`:98`counterparty | 已实施真实owner调用，未新增开局接受集校验。 |
| result:14、review:37/46–48：N22单次BorrowingCostAccrualPlan/双链/槽位/提交顺序 | `borrowing_costs.rs:26`暂态Plan、`:36`plan_accrual、`:50`合同顺序、`:59`零日跳、`:62`项目分类、`:74`/`:84`两链；`:154`取分录、`:278`accrue实际创建/`:284`post→`:287`loan apply→`:290`汇总→`:295`project apply | 主体已实施，不是测试专用结构；`company/operations/dispatch.rs:48`地产分支实际调用books.accrue_interest。 |
| result:15、review:38/49：N25 ProjectState guard与preview_resume | `projects.rs:114`development/:139`suspend`/:159`resume`/:183`complete`；`development.rs:32`/:74`/:93`/:112`调用 | owner接线已实施。complete不新增日期前进guard是纯重构明确要求，不是漏实现已批准新交易规则。 |
| result:16、review:39/50：N26 ProjectLoanState预览/还本guard | `loans.rs:96`preview_interest_payment、`:110`validate_repayment；`debt_service.rs:24`/:68`真实调用；accrue`:287`直接state.apply_split | 已实施，目录/总账仍由Books协调，未留下只有类型的死代码。 |
| result:17、review:40/50–51：N27 PresaleContract守卫/余额，Books跨项目合同 | `presales.rs:54`remaining_payment、`:59`collection、`:89`delivery；`:175`collect调用；`delivery.rs:47`合同guard→`:63`remaining→`:65`cost preview | 已实施，非额外交付plan；记录明确已拒绝新交付协调对象，不按旧愿望重新要求。 |
| result:34–47、review:26–30：分/bp/套数、ACT/365F、双FractionUnits、预售负债/交付收入、CAS17游戏假设 | borrowing_costs两条accrue_act_365f；`docs/company-accounting.md:113`地产正式范围、`:121`资本化游戏假设；Books/operations不修改投资者Account | 套数不是股份；CAS17法源债已登记，不能由重构review提升成真实完整会计规则合规。 |
| result:39–42、review:48/53：post后部分失败明确保留、无任意异常原子承诺 | `borrowing_costs.rs:284`先post，`:287`apply可Err，`:290`checked项目汇总，`:295`project apply；`ownership_tests.rs`相应overflow保护 | 保留旧失败面是本动作约束，不作为新的“重构遗漏原子性”G项；正式GameSession失败面必须单独查外层候选/回滚，不从局部Books类型推断全会话污染。 |
| result:21–30、review:52/55–57：9短case、无迁移前baseline执行、不假称TDD红绿 | `ownership_tests.rs`当前九case；`domain/final-summary.md:15–19`后续104短case/并发deadline及增量失败限制；result:30已引用root短验收 | 本轮没有重跑；历史“未完成门禁”标题不能忽略同文件后续回填和最终汇总。未取得迁移前红灯不能虚写历史红灯证据。 |
| result:54–69、review:12–18/59–66：RE-R1早日期fixture修复 | `ownership_tests.rs:96`开发date(2)、`:117`完工date(1)、`:119–121`两日期及严格小于；共享developing fixture未变 | 历史有效发现已完整修，不新增。当前production complete守卫`:183`确实没有添加被禁止的日期校验。 |
| review:68–96：最终canonical复核/diff/hash绑定及后续变化需另审 | 本轮sha256sum ownership_tests.rs=`6fede901143cbf4f97b1a13d56d58680677be8bd5dcc8088dce8aff5cc65fc03`与原绑定完全相同 | 对该测试核对当前确为已审版本；未核全部9文件hash和完整diff，不能把单hash核对说成全部新版diff重新独立审查。 |

## 正式范围优先与新候选反证

地产内核与operations分支都有真实caller：`operations/dispatch.rs:49`计息、`operations/real_estate.rs:115`开发、`:129`完工，随后预售/交付接Books。与此同时正常新局`session/company_assembly.rs:341`kind仍Industrial，`:430–431`装配Industrial books/flow；因此上述owner refactor完成不能核销现行总账G36“四行业可自定义并公开报告跑通”，也不能新造一项相同“地产没有任何代码”。

本批未发现新未映射代码遗漏候选（**0**）。已明确排除：①初次Account/Market caller迁移清单已核销；②F1直接Money边界测试已补；③RE-R1 fixture已实际保护严格早日期；④局部既有部分失败是明确保留的接受/失败面；⑤独立review未执行测试不等于后续root从未测试；⑥地产完整会话公开闭环仍为G36，局部重构结论不越权核销。旧action-index缺引用只作为历史证据定位限制，不转换成产品功能任务。
