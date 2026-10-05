# 共享市场公开事件与本人金融事实投影

## 已确认方向

根协调者于本轮确认采用当前协议的新 `PublicTrade` 与 `PrivateEventOmitted`，不建立旧档兼容。
真实 Engine `Trade` 继续包含 maker／taker，撮合、结算、个人 execution confirmations 不改。
Server 对所有 viewer 的成交传输统一投影为 `PublicTrade { seq, code, price, qty }`，不输出账户身份。
本人订单／撤单／拒单／结算错误仅交给本人；其他 viewer 得到 `PrivateEventOmitted { seq }`。
控制授权不授予查看其他个人账户详情的能力，缺席经济成员的控制者同样只得到公共市场内容。

## 序号与事实身份

- 不删除事件，不收缩 `(seq_from, seq_to]`，每个已提交 seq 仍由一个事件表示。
- `EventFact.event` 和 `canonical_payload` 一并替换；不保留能还原私有现金、账户或错误原因的原字符串。
- `PublicTrade` 的 `EventStableKey` 使用原 Stock／Sealed 身份及原 local index，保持公开量价事实身份稳定。
- `PrivateEventOmitted` 使用 Session／Sealed 身份及 `local_event_index = seq`，避免泄漏 Account 实体与不同账户局部 index 碰撞。
- 本人原金融事件保留原 key；同一 viewer 的重放及覆盖区间保持稳定，不把跨 viewer 不同投影冒充同一原始私有字节。
- 为保证合法完整 u64 成员账户跨 Web 无损，本人 `OrderAccepted`／`OrderCanceled`／`IntentRejected`／`SettlementError` 的 `account` 与 `EventFact.key.entity.Account` 统一规范 u64 十进制字符串。Rust 内部 `AccountId` 不变，不扩大旧内部 `Trade.maker/taker` 编码，不接受数字 JSON 兼容路径。
- 同步投影 TickBatch 的 frames 与 CivilUpdate 的 events／facts **以及 refresh.intraday**，不能只修当前 tick 而日终再泄露完整日内金融事实。
- Snapshot／RuntimeDelta 继续只含本人账户；工作单 upserts 按 owner 过滤；removed 使用当前 `OwnerScopedRemovedOrder { id, owner }`，从上一权威工作单投影派生并按本人 owner过滤。缺席 viewer 的 upserts／removed均为空；不增加 per-viewer cache 或锁。

## 跨层范围

- Engine 只扩展传输事件 enum 与 seq／key 全匹配，不改变生产撮合或模拟规则；这些 variant 只由外壳公开投影产生。
- 全文搜索 Rust 对 `Event` 的 exhaustive match，包括 protocol timeseries／key／验证与测试；禁止通过默认 arm 掩盖未知 variant。
- 已定位必须审阅的 Rust 完整匹配：`session.rs` 的 seq、`session/pipeline/event_key.rs`、`session/pipeline/event_collection.rs`、`session/protocol/commit.rs`、`verification_evidence.rs` 的事件域／名称／股票／规范 payload，及 `diagnostics.rs` 的事件消费。verification harness 不得把外壳投影事件冒充内部真实账户收据。
- Web 当前 Event 类型、严格 parse、canonical、source identity、seq、effects／reducers 同步承认两种明确职责的 variant。`PublicTrade` 只用于公开逐笔量价；本人资金、持仓与执行结果不能从匿名成交重建，继续消费 RuntimeDelta 与本人 confirmations。
- Server 投影边界与缓存必须持续使用已验证 Auth 主体；token 失效或 generation 改变仍拒绝旧 cached frame。公开匿名化不替代认证。
- 不用 fake AccountId(0)，不将身份修改为 null 后冒充原 Trade，不把未知私有错误替换成普通成交。

## TDD 与独立复核

- 首先写独立 Server 短测，仍仅构造旧真实 Event variant，旧 `for_member_account` 应泄漏并失败：本人、其他成员、缺席控制者三种 viewer。
- 测试覆盖完整事件 JSON 与 facts/key/canonical 字符串私有字段泄露、完整序号验证、公开成交量价不变、原输入事件事实未修改、CivilUpdate 日内历史递归投影。
- Web 同步短测检查匿名量价消费、占位不触发订单／拒单 effects、私有字段严格拒绝及 canonical/key 一致。
- 根协调者统一编译并执行普通十秒外部进程树 deadline；取得真实红后实施，随后同短 fixture 绿测，整批由非作者重新审查。
- host21 在途期间只新增独立测试／本设计文件，不修改既有共享生产文件。

## 真实红测与实施状态

- host23 实际 compiler-artifact：`public_event_projection-d939f16aa82e1231`，`--list` 实际4项；并行4case、Rayon8、仓库外部10000ms进程树监督，4项真实失败，用时0.21秒，exit101。
- 失败分别证明旧投影仍发 `Trade.maker/taker`、另一成员投影没有 PublicTrade、缺席 viewer 的公开成交字段缺失、成功日终 `refresh.intraday` 仍泄露 OrderAccepted。原合法 batch／CivilUpdate 验证保持，未削弱断言。
- 同批独立监督的真实忙 NPC HTTP case 通过1/1、0.46秒；完整非0账户、真实 gate 未释放登记、Player／NPC receipt ordinal、一次受理与关闭／缺席／旧代负控均保留。
- 红后实施新增 variant、外壳递归投影、完整 facts／canonical／key 同步匿名化；内部 event collection 明确拒绝外壳 variant，内部 verification evidence 和 diagnostics 明确拒绝用匿名投影冒充真实账户执行证据。
- Rust 新源码等待根统一编译及同4case绿色；Web 协议同步由 UI owner 实施。尚未宣称整批通过或独立复核完成。
- host24 当前成功 PublicProjection artifact 后本作者独立执行同4case：4 passed、0 failed、0.20秒，Rayon8／threads4／外部10000ms进程树监督。随后新增本人4金融事件及事实实体 MAX u64 字符串短测，当前数字编码必然违反该断言，但尚未执行新case；等待根取得真实红后修改字段契约。
- 新增内部 collector拒绝外壳variant短测曾对现 Engine artifact 执行 exact，实际 `running 0 tests`，不能计为通过；已明确通知根必须重新编译并核对 `--list`。
- host25 当前成功 artifacts 经实际 `--list` 后，本人 MAX case 真实红1/1、0.00秒：旧 safe AccountId 编码在 `attach_facts` 就返回 FactIdentity，不能生成合法本人全 u64 事实。红后仅为四种金融 Event.account 和 EntityTag.Account 添加规范十进制字符串 serde／ts 属性，不改全局 AccountId 或内部 Trade。
- 同轮当前 Engine artifact 的内部 collector guard真实运行1项，1 passed、0 failed、0.00秒；此前0 tests记录保留，不混用旧 binary 结果。
- host26 当前成功 artifact 后实际 `--list` 5项；并行投影5/5绿色0.21秒，包含本人四金融事件／实体 key MAX u64 字符串往返与 numeric／非规范字符串拒绝。内部 collector guard同轮真实1/1绿色0.00秒，均外部10000ms进程树监督、Rayon8、test threads5／4。
- 完整合法 u64 账户路径仍发现更深根因：AccountId 本体原编码会拒绝 MAX Snapshot／SaveSlot mapkeys及内部 Trade事实，根已批准必要全类型当前规范字符串根修。本作者已写真实 memberMAX→snapshot/save/history恢复与真实 FIFO/T+1成交两case；root26列表确认尚未编入，不能将窄金融绿色宣称为完整执行／持久化全范围绿色。
- root30 成功 JSON中的实际 compiler-artifact并实际 `--list` 第6case：真实两玩家挂单→撤单的 removed投影真实红，旧输出 `[1,2]` 泄露另一成员ID，正确期望仅本人 `{id:1,owner:"0"}`；1 failed、0.20秒。随后实装 `OwnerScopedRemovedOrder` 当前结构、上一权威工作单owner派生、Server本人过滤及严格owner/id字段验证，未改该强断言；等待新产物绿色。
- root30 同步真实MAX成交case绿色1/1、0.55秒：已正确投递NPC收据，makerMAX／taker持有初始100股NPC、T+1、内部Trade JSON、完整SaveSlot恢复及本人confirmations保持原强断言。未手改旧sealed见证。
- host31 成功 JSON中的实际 compiler-artifact经 `--list` 后独立监督绿色：公开投影6/6、0.21秒；两玩家真实撤单的 owner归属身份只交给本人，缺席 viewer为空，原其余5项不弱化。`OwnerScopedRemovedOrder`／非0金融 guards 2/2、0.00秒，完整 owner规范字符串及安全订单ID边界保持。
- 与投影批次并行的真实 Engine成员11/11绿色0.57秒，包括MAX完整 Snapshot／SaveSlot／history恢复与真实 FIFO Trade／T+1／confirmations链；root统一执行跨端绑定生成，Web 当前 strict parser／effects／fixture迁移及非作者最终复核仍待根登记。
