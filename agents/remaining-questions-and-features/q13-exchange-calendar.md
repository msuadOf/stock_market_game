# Q13：共享自然日与逐交易所开闭市

## 用户回答与领域依据

自然日期共享，沪深交易所分别判断开市；休市交易所不接受交易，不生成假成交。经营、到期收付、月末封账及公告继续按自然日推进。Q07 的虚拟前史和日 K 日期沿用各证券交易所的同一日历，默认自然周／月聚合不变。

本轮沿用已登记、已核验的现行法源：上交所《交易规则（2026年修订）》2.4.1–2.4.3、深交所同规则2.3.1–2.3.3，2026-07-06起施行，原文链接与原核验日期见 `docs/trading-rules.md`、`docs/company-accounting.md` §5。交易日为周一至周五，国家法定假日和本所公告休市日休市；两地现行股票竞价时段相同，但休市事实不能由首只股票替整个市场决定。本轮不改变撮合时段、集合竞价规则或收付制度，不把合成测试公告当作真实年度公告，也不宣称重新联网核验全部规则。

默认 `CalendarPolicy` 仍没有已核验的年度 `Official` 覆盖；未来与缺证历史年份继续使用明确登记的模拟回退，不采集真实行情。测试可构造带明确 `synthetic-test` 出处的不同交易所覆盖，只验证机制，不声称预测真实沪深假期差异。

## 源码映射与实施方案

- `session/civil_clock.rs` 原只保存首只证券所属交易所，`phase`、共享会话数和 `next_status` 因此错误依赖配置顺序。现接入本局实际交易所集合：任一开市则共享交易时间推进，全部休市才是 `ClosedDay`；集合由当前 setup 重建，不增加冗余存档计数。
- `pipeline/account_validation_context.rs` 和 `account_validation.rs` 在权威校验起点捕获逐股开闭市；休市的新申报和撤单明确返回 `ExchangeClosed`，不冻结资金或股份。
- `continuous_matching_adapter.rs`、`stock_auction_adapter.rs` 只为本日开市证券产生股票工作输入。闭市证券保留真实上一价与持仓估值，但不产生行情、分钟序列、日 K 或集合竞价结果，不改写昨收。
- `pipeline/auction_day_end.rs` 的共同日界只归档真实活动日 K；T+1 只解锁当日开市证券。共享 day 是至少一家交易所开市的会话数，不再冒充每股累计开市数。
- `session/persistence.rs` 按各证券实际开市日期、当前自然日和是否已经完成本日市场会话核验历史长度、活动日 K 与分钟序列；保持严格当前结构，不迁移、不补旧字段、不引入 schema 版本。
- `ProtocolSession`、Server、Desktop、Browser 共用同一 engine 判断和事件；公共日终档依然仅在成功自然日日结后保存，经营／披露不因某一交易所闭市而停止。

异步入口保留唯一成功发布日期与非持久 publication epoch，不用普通 checkpoint 的最早日期冻结日历。
设计独立审查明确拒绝了“所有活跃只读 checkpoint 都 pin 日历”的候选：该方案会让 UI 已见新日但
玩家仍被旧日期假拒绝。实际方案使用 `with_publication_transaction`；仅真正未发布、可整批回滚的
Native 批次延迟日历安装，全部可失败准备结束后才发布并允许 UI 观察。普通 checkpoint 跨已发布
自然日或跨 source 回滚明确拒绝，未发布事务则由 RAII 在错误或 guard 丢弃时恢复，保留实际收件。
Protocol 披露 observer 同样在成功安装后交付，不让失败协议候选留下外部 observer 事实。

账户 `ExperienceMoment` 与 `PersonalHistoryReadLedger` 仍使用跨市场共享事实时间，不能将
单股较慢时钟写进全账户单调经历队列。`PersonalPriceMemory` 各证券独立 entry 使用该股分钟，
与共享历史访问 ledger 的记录参数明确分开；跨证券最近接触驱逐使用共享 `last_touched_minute`，
不按异长的股票本地分钟排序。估值锚点、计划日期与报价期限使用该股开市序列。
closed 持仓净值包含其上一真实价格，但不为它追加新价格经历或主动读历史事实。

## 验证状态

TDD 使用每交易日2个 tick、零或一个机构和真实公司装配的短 fixture。初次4个 case 在恢复 fixture
处失败，不是目标业务红：对2030年度仅关闭1月2日的合成 `Official` 覆盖会替代 fallback，让1月1日
从默认闭市变为开市，导致旧前史日期与新政策不一致。已将合成闭市范围改为1月1日至2日，维持开局前
日期完全相同；没有修改真实日历规则、恢复守卫或前史算法，原失败日志保留且不计作目标缺口。

修正后实际 build6 的6个 exact case 均出现目标业务红（并行6进程，各 `RAYON_NUM_THREADS=8`，
每 case 进程外10000ms，实测0.72–0.82秒）：首股决定了 `ClosedDay`；闭市股仍产生行情；闭市
公开收件未拒绝；闭市计划错误到期；闭市计划仍获报价与软预算；复市证券 day0 请求被共享 day1
拒绝。日志为 `.tmp/checklist-wave4/q13-build6-actual-red-*.log`。首次 shell 收尾误用 zsh 只读
`status` 变量只影响尾部打印，真实 case 日志已经写完，均明确1 failed、0 passed 和 bounded 101；
后续改用 `result_code`。另3项旧实现真实红覆盖闭市 attention 信号、behavior 选股和持仓的新价格
观察，实测0／0／0.75秒，绑定同一实际 build6 binary；未将未构建的 integration strategy case
登记为已执行。

统一 host-build-8 已编译成功，实际 engine binary 为 `engine-48220fcc6e1a5073`。本轮以6进程并行、
每进程 `RAYON_NUM_THREADS=8`、每 case 进程外10000ms运行43项逐股日历、behavior、attention、
持仓观察和邻近 Protocol rollback 短测，37项通过，其中全部20项新增日历与 publication 边界通过；
日志清单为 `.tmp/checklist-wave4/q13-build8-results.log`。另6项失败如实保留：2项旧fixture
默认1月1日直接调用市场 step，现改成1月2日合法开市日且保持原断言；4项暴露全闭市初始化仍消费
NPC 市场观察、生成空 equal-weight 样本的实际遗漏，现修为 `ClosedDay` 不消费 attention／due，
以已有 `None` 表示没有市场观察，不伪造零收益样本。修后 host-build-10 编译通过（51.94秒），
同样并发条件的46项 exact 中44项通过，包括全部23项新增边界；
`.tmp/checklist-wave4/q13-build10-results.log` 保留完整结果。另2个 malformed frame 重试 case
在旧开局1月1日触发闭市拒绝，已经同样只改合法开市fixture为1月2日，原历史、事实、状态和重试
断言不变。最终 host-build-11 编译通过（38.90秒），以实际更新的 engine binary、相同并发与
deadline重新运行46项，全部通过；两项内置策略闭市 integration exact 也以对应新 binary通过。
结果分别见 `.tmp/checklist-wave4/q13-build11-results.log` 与 `q13-build11-strategy-*.log`。
其中包含最后两个 malformed fixture 与此前真红的双时钟边界，不以旧 binary冒充覆盖新fixture。

独立审查另外发现跨证券驱逐混用了本地股票分钟；新增
`prune_uses_shared_contact_time_when_exchange_clocks_differ` 已在同一实际 build8出现目标真红，
错误淘汰当天复市证券。日志为 `.tmp/checklist-wave4/q13-build8-price-memory-red.log`；修复已将
观察样本的本股时钟与跨证券接触的共享时钟明确分离，host10 的差异时钟驱逐、双时钟回拨、
全闭市不观察并在复市处理原due 三项实际短测全部通过。全文独立复核继续发现两个有效遗漏：
ClosingAuction 的恢复守卫以 `%240` 错把完成240分钟归零；价格触发的活跃计划复核仍以共享day
比较本股期限。host-build-12 两项新短测分别真实红于合法价格记忆恢复和空ready roots（1.23／
0.81秒），日志为 `q13-build12-session__exchange_calendar_tests__*.log`；随后已改为从day_tick
重算连续阶段完成分钟、逐股交易日复核并显式跳过closed。修后绿测与最终完整门禁仍待完成，不能以
host11旧绿冒充新边界通过。host-build-14（实际编译1分07秒）对这两个修后exact实际通过
（1.53／0.81秒），日志为 `q13-build14-session__exchange_calendar_tests__*.log`。

邻近计划25项短测在host11有2项旧fixture仍直接改共享day／闭市观察；现在改为真实开市tick及
自然日日结、保留全部原断言，host12逐项全部通过，证据为 `q13-plan-host12/results.txt`。Native
两个新增calendar case已在host11真实通过；邻近Desktop fatal case将已关闭receiver用于续行，现
以仅测试启用的verification副本保留30帧与停止保护断言，Server burst的休市开局亦只改为开市日，
其中64条请求顺序断言保留。完整 diff 复核还要求direct闭市step既然返回 `StepFatal` 就保留原
poison契约，新增测试在host14真实红于 `None` 不等于 `Some(fatal)`（1.01秒），随后仅在该闭市
分支调用既有 `poison_failed_step`；仍待下一实际binary验证，不把可恢复precondition擅自定义为
fatal例外。最终host-build-15编译通过（58.32秒），当前actual binary按相同6进程、Rayon8与
每case10000ms外部deadline重新运行全部49项engine exact及2项strategy integration，全部通过；
结果为 `.tmp/checklist-wave4/q13-build15-results.log` 与 `q13-build15-strategy-*.log`。最后三个
findings各case实际1.00／1.56／0.80秒通过。Native修复后的13项短测已在host12全部通过（最长
0.81秒），证据为 `q13-build12-native-*.log`。

非作者 `review_calendar_publication_design` 已审完整Q13 diff、相关生产路径与新增／修改测试，
核对host15原文后最终Gate通过：现行沪深语义、需求必要性、时间单位、publication／scope／
rollback／observer与fatal保护一致，全部有效finding已修复，无未解决发现。Q08／Q22／Q23
非关联改动不属于这次批准；未改历史测试块没有冒称重新全仓审计。未执行完整回归。

Web 的明确闭市拒单文案已真实红绿：旧实现返回 `undefined`，与“该证券所属交易所今日休市，不接受委托”断言不符；实现后 `utils/format.test.ts` 的7项短测全部通过，Node case 与进程外 deadline 均10000ms，并发4，实际0.20秒。日志为 `.tmp/checklist-wave4/q13-web-format-red.log` 与 `q13-web-format-green.log`。首次命令漏传 `--` 只产生 CLI 用法错误，已修正后重跑，不计作测试。Protocol decoder 与 effects 同步接受 `ExchangeClosed`；实际 Rust 类型生成已由 root 导出且 `tsc` 通过，本记录不将这个 Web 子项说成 Q13 整项完成。
