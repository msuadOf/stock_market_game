# fixtures1 独立复核

复核日期：2026-10-03。复核者为未参与本批实施的 subagent。基线为
`b89afb3346743a4b4fccf26c9ac9ff108595f696`，范围限定于
`fixtures1/actions.json` 中六个动作的 11 个文件。已逐文件阅读完整 diff 与当前源码全文，
未修改源码，未运行 Cargo、普通测试或长验收，未委派其他 agent。

结论：本批静态审查通过，未发现需要修复的改动缺陷。结论不代表编译或运行时测试通过；
`fixtures1/status.md` 中待执行的定向验证仍须由协调者完成。

## 大 A 语义与依据

已读仓库 AGENTS.md、`docs/principles.md`、`docs/trading-rules.md`、
`docs/open-questions.md`、`docs/architecture.md` 及 ADR-0002、0009、0011、0012、0014、0019。
本批只改 integration test 的 fixture 所有权与调用组织，没有修改生产撮合、证券配置默认值、
领域类型、费用、申报数量、时段、T+1 或存档契约。

交易语义沿用 `docs/trading-rules.md` 现有官方依据：沪深 2026 年修订交易规则自
2026-07-06 起施行，文档记录主要条款于 2026-09-22 核对，价格时间优先另于
2026-09-25 核对。本复核没有重新联网查询，不能将该文档引用冒充 2026-10-03 的
官方来源重新取证。中国结算费用表的既有访问失败说明仍有效。本批没有增加或改变制度规则，
因此本次判断是验证原制度语义在 fixture 搬迁前后保持一致。

- `auction.rs` 继续显式区分 Shanghai/Shenzhen；开盘竞价、PreOpen、连续竞价、
  ClosingAuction 与日界失效的输入和断言保持。分、股、0.01 元 tick、主板/ChiNext/
  StMainBoard 分类、limit_pct 与 t1_enabled 值保持。沪市中间价与半价 rounding、
  深市开盘前收参照均仍由原测试锁定。深市盘中/收盘参照尚未完整实现的游戏简化没有被本批扩大。
- `behavior.rs` 保留 ADR-0011/0012 的市场时间与 tick 区分、本人成本/风险、
  期望差额与可执行差额区分、T+1、买入整手和已有零股余数。固定 current price、
  独立 thirty_minute_market 与假定市场宽度仍是原 synthetic 决策输入，不能称为真实市场观测。
- `Scenario` 和 `AnnouncementExposureFixture` 继续区分 Books 私有事实、PublicLibrary
  公开信息、discovery 候选与 NpcInformationState 本人已读；fixture mutator 未代替个人获知。
- `WeekendScenario` 的周末 Q1 排期是原合成游戏场景，没有新增“已验证法定披露期限”的承诺。
  周五真实 step、Session 日结、公司经营 finalize、disclosure dispatch 顺序不变；周六没有调用 step。
- `SeasonedSaveFixture` 保留完整当前 schema、真实两日经营与个人链状态，没有 legacy fallback、
  补钱、撤单、清空状态或其它恢复容错。

## 必要性、最小范围与跨层边界

| 动作 | 审查判断 |
|---|---|
| hosts-N03 | 已有 Scenario 自持 Books、ClosingEngine、PublicLibrary；构造及两个 mutator 搬到 impl，未新增字段或隐藏 record_acquisition。符合动作范围。 |
| hosts-N04 | WeekendScenario 自持原四元组；重复的三个日结步骤收拢为方法，真实 report/out 交回测试。shock、before snapshot、重复 dispatch 和公布断言继续显式。 |
| hosts-N05 | 固定 setup/seed 和只读 baseline 归 fixture；原 OnceLock 缓存边界保持，build_session 每次鲜建并真实运行两日。continuity/quiet-point 的独立 setup 未混入。 |
| hosts-R2-N01 | 公开库、published、codes、MarketView 归单个测试 fixture；exposed_at 每次按 as_of 查询，没有缓存 exposure 或公开私有 Books。只读 codes getter 有实际消费者。 |
| hosts-R2-N02 | 竞价场景 setup/seed 与合法 SaveSlot 构造流水线归 fixture。每个测试仍拥有运行 Session、订单输入和断言；没有通用 TestUtils 或生产对象层次。 |
| hosts-R2-N04 | 从同一 paths 键集派生的 MarketView 和 BehaviorMarketObservation 归 BehaviorScenario；两个原散改点以 setter 原值搬迁，原 engine API 仍接收借用的两个输入。 |

六项均有已登记动作授权。N05/N02/N04 中的可选对象化并不说明原函数式 helper 是生产设计缺陷，
本批价值限于具名表达测试输入的共同所有权。没有新增依赖，没有把 fixture 类型引入
engine 生产 API、宿主、UI 或存档 DTO；未发现跨层概念漂移或超出动作的附带改动。

## 原测试、seed 与断言保留

人工核对完整 diff 的场景构造、订单列表、seed、日期、金额和配置值；借用方法替换不改变原实参。
辅助静态核对按原测试函数提取 assert/assert_eq/assert_ne，归一化空白及本批 fixture 借用/变量搬迁后，
原断言均一致。没有删除或重命名原测试函数；save_contract 的 14 个宏生成缺字段用例及宏断言也保持。
这是源码比对，不是测试执行结果。

| 文件组 | 原断言保留证据 |
|---|---|
| information_acquisition | 三个消费者分别 21、16、13 处断言保持；fixture 年结 sequence=1、更正 sequence=2 的两处断言原样搬迁。OPS_SEED=11 和全部分录/排期值保持。 |
| weekend_publish | 两个测试 24 处断言保持；Saturday 搜索范围、calendar、OPS_SEED 与 TICKS_PER_DAY 不变。 |
| save_contract | 主模块共 22 处、failures 共 20 处原断言保持，含测试 helper/宏断言；SEED=0x27_C0FFEE 与两次 run_full_day 保持。 |
| attention_discovery | exposure 9 处、discovery failures 8 处原断言保持；公布前/后输入、个体抽样 seed 与统计阈值不变。 |
| auction | 26 个原测试与 104 处原断言保持；原构造 seed 及恢复 helper 的 99 均显式迁移。 |
| behavior | 39 个原测试保持，107 处测试内断言及 prior_range 的 assert_ne 共 108 处原断言保持；FixedRng、risk/experience 和策略值未迁入共享状态。 |

新增仅三个测试：缓存 clone 隔离、竞价证券身份/envelope keys、行为场景键集与 breadth setter。
未见通过改断言、删 fixture 数据或替换决策链使测试“通过”的行为。

## 合法 restore 与 invalid 分支

`auction.rs:140` 的 save_with_orders 从 fresh Session 保存，再用测试显式订单和 previous_close
构造合法竞价起始状态；market.last_close/last_price、auction_orders、next_order_id=100、
排序的 live_envelopes 都承接原 restored_with_previous_close/restored_on_exchange/
synchronize_v2_auction_envelopes 流水线。envelope key 使用真实 order.owner、stock、order_id、side，
排序的是 runtime 身份表，不对 auction_orders 做重排，不篡改受理先后。

多股 preopen 测试 `auction.rs:713` 复用同一流水线。新增显式 previous_close=10000 与该 fixture
原 initial_price=10000 一致，没有额外改变无订单股票行情。合法完成档恢复之后，
`auction.rs:750` 才删除 idle_code 的 active_daily_candle，并在 `auction.rs:753` 直接调用
GameSession::restore 要求 InvalidSave(active-candle)。坏档没有再次送入 save_with_orders 或同步 envelope。

`save_contract/failures.rs:17` 的 restore_tampered 仍仅序列化 → decode_save_slot →
GameSession::restore。缺字段、legacy/future schema、伪造 publication/belief 引用、未来观察、
个人状态缺失、price memory 矛盾、calendar digest 错误、账本失衡和 payload 字节上限用例的
实际篡改与错误断言全部保留。build_session 仅提供原鲜建源会话；clone_save_value 返回
篡改前 baseline，不参与篡改后的修补。

`information_acquisition/failures.rs:153` 的重复/乱序/跨公司重复记录、serde 错误仍直接送入
原 from_parts/Deserialize；Scenario::new 与 publish_correction 的方法化没有绕开拒绝路径。

## 新 fixture 检查的可靠性与覆盖限度

- `save_contract/main.rs:156` 比较 baseline、独立 tampered clone、后续 clone；篡改
  schema_version 后先 assert_ne，再确认缓存仍与 baseline 完整 Value 相等，具有区分力。
  Value 为 owned 数据，clone 不共享可变子树；OnceLock 只交出共享不可变引用，fixture 没有
  对 baseline 的写入入口。该检查只实际变更顶层字段，没有声称验证并发调度或深层篡改的全部组合。
- `auction.rs:187` 明确断言两市 code/exchange 配对，再以倒序注入的买卖订单检查排序后的
  account/stock/order/side keys 与 last_close/next_order_id。原恢复与真实撮合测试继续覆盖
  SaveSlot 可用性；新检查本身没有伪称执行真实 restore 或全部资源冻结校验。
- `behavior.rs:154` 检查 market 与 price_paths 同键集及构造比例，setter 后断言 total、observed、
  return 与保持的比例。原两个 setter 消费者都为 observed>0 且 observed=total，原输入完全保留。
  新测试的 breadth 为独立 synthetic 输入，不是从两只股票 path 重新计算出的等权市场统计。

边界覆盖限度：`behavior.rs:138` 新增 observed<=total guard 未有负向用例；setter 本身也没有
为 observed=0 重建 None 的语义。当前三处调用均为正 observed，原 from_paths 已使用独立
10/10 synthetic breadth，因此未发现本批实际调用的行为回归。这一小型 fixture 不能被理解为
适用所有 EqualWeightMarketObservation 输入的生产校验器；若将来新增零覆盖场景，应显式提供
完整合法 observation 并定向测试，不能沿用残留比例掩盖不可用市场历史。本项是覆盖限度，
不是阻塞本批的生产缺陷或要求扩大本轮实现范围。

## 静态检查与复核快照

正确参数顺序的定向 `git diff --check <baseline> -- <11 files>` 返回 0，输出为空。
没有执行 Cargo，未验证编译或运行时耗时；上述状态不能替代协调者的定向短测试。
复核时 11 个文件的 SHA-256 如下；相关文件再次修改后须核对新增 diff。

| 文件 | SHA-256 |
|---|---|
| information_acquisition/fixture.rs | b1ba50f808d1c9eef78902bb8c65ac5e7f63ef14cf67da18392d69eb029daf07 |
| information_acquisition/acquisition_gold.rs | 4885655e3a8874d0acdc277801b102481cc4fcdc5b36833d72ad8df3fd08ea63 |
| information_acquisition/failures.rs | 9931e4fe69909595139e8fa132e34a2ecf70e5d3aa8a08ed90d0cee44f2a9269 |
| information_acquisition/view_gold.rs | d88e3a1e386c38c26cd763ec099c2cc716b2038f7561f7a828548c0c1db7456a |
| publications/weekend_publish.rs | b3b53905e915a1719ac40d03e36fddf5ffa11685da014ab35b3aa956bce30279 |
| save_contract/main.rs | a613dd823200e69e8135b6376ce68a8e6429a9cfc57cd70f4202b6f14707e454 |
| save_contract/failures.rs | 0067fc91e96bf67f2e80e067270d05ab1a59a3efbd7c6b75fdfb225970d9a51f |
| attention_discovery/exposure.rs | 82e43ee8a67922ea6856555ec6035d7c0e85e3fa84d11ce154d47987af6346e5 |
| attention_discovery/failures/discovery.rs | 1f18c77cc73af9e621f874ecaefb95f829cf18c73a278d34064b5f19271a4df7 |
| auction.rs | b06572d1b65a4f0bf5060439e686196989a4884992650baa3676b1e9d259ea3f |
| behavior.rs | 6b5556e0f6f89cab1737749d737e53390a821665c67a08f609905a3b38564e87 |

表中文件均相对 `packages/engine/tests/`。
