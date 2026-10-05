# Engine 成员与本人账户接线

## 范围与契约

- 依据 ADR-0030／0031／0025／0033；部署运营审计总账第234行。没有改变 A 股价格时间优先、T+1、申报数量、费用或 NPC 初始股份分配规则。
- `OpaqueSubjectId` 是外壳认证后传入 Engine 的独立主体标识，不承载密码、token 或硬编码角色。Server 的 UUID 身份由身份适配器生成。
- `MarketMembershipState.members` 只保存主体→本人 `AccountId`、一次性 `AdmissionFunding.external_cash` 等经济事实。市场控制授权独立位于 Actor／身份授权注册表，不进 `SaveSlot`，不从导入存档授予或撤销。本地新局默认 `local-owner` 经济账户；Server 创建新市场时将该初始账户绑定为真实创建主体。
- 玩家共享一个 `GameSession`、市场时间、盘口及 `SharedSessionIngress`。新成员为独立 `AccountKind::Player`，零股份。动态加入在同一收件入口登记新账户，不新建市场或替换收件入口。
- `join_market` 对已有成员幂等。`set_admission_cash` 仅修改之后新成员的初始资金；外壳独立检查控制授权，已有账户与记录不变。资金来源显式命名为 Admission，而不是 NPC 补款。
- 成员事实随 `SaveSlot.market_memberships` 保存／验证／恢复，进入完整业务 hash 与私有 shadow／commit；不产生新 I/O，不启用 WAL，不新增格式迁移。
- 恢复后缺席主体必须确认重新加入；不删除外壳 Auth 身份。确认后新建零股份账户且只拨一次当前市场配置资金。缺席的原控制主体仍可暂停、重置、读档和修改设置；拥有档内经济账户不等于获得控制授权。

## Server API

- `bind_creator`、`join_member`、`resolve_member`、`snapshot_for`、`enqueue_for`、`working_orders_for`、`history_for`、`confirmations_for` 从主体派生账户；网络请求不传任意可执行 `AccountId`。
- `enqueue_for` 不等待 Actor 处理成员查询；现有 ingress lifecycle 短锁内保存同 generation 的派生主体→账户索引，读取后直接登记真实共享入口。`join`／`bind` 在该短锁内更新，`restore`／`reset` 与 source 关闭及新 generation 同一次交换。索引不是持久化数据，也不是新增授权事实。
- `restore_for`、`reset_for`、`set_speed_for`、`set_running_for`、`set_pause_preferences_for`、`set_admission_cash_for`、`shutdown_for` 在 Actor 应用操作时检查独立控制授权及 generation。`restore_for` 保留既有交易时钟校验与 generation 隔离，加载不替换控制授权集合。
- `market_context_for` 原子返回当前 generation、可选经济成员、独立 `can_control`、`needs_rejoin`、真实当前 setup／seed。`public_baseline_for` 原子过滤本人账户；缺席主体得到空 accounts 与公共市场，而不是另一人的账户。
- `EngineUpdate::for_account` 与 `PublicBaseline::for_account` 供 WebSocket 外壳投影本人余额和委托。内部公开运行投影包含全部 Player（不含 NPC）；`PlayerWorkingOrder.owner` 使账户工作单可按主体过滤。
- 新契约 `MarketMembership.account_id` 和 `PlayerWorkingOrder.owner` 使用规范 u64 十进制字符串；范围包括 `18446744073709551615`，拒绝数字 JSON 和非规范字符串。没有改旧 Trade 的账户字段或建立旧格式兼容。
- 原有可信宿主／测试使用的 legacy handle 方法保留；生产 REST／WebSocket 必须接主体感知方法。该接线由身份实现 Agent 负责，不能仅因 Engine 方法存在而宣称真实多人验收完成。

## 验证与限制

- 新建 `memberships_tests` 测试文件在实现前；按统一资源协调约定没有自行执行 Cargo 红测。首轮统一编译暴露遗漏的 `commit_from` 成员字段，已补全真实转移，未使用 `..` 隐藏。
- 定向短测涵盖重复入场／入场设置变化、旧档缺席确认、当前成员余额与历史恢复、伪造账户／负资金、真实共享撮合 FIFO、T+1、他人撤单拒绝、空 NPC 流通盘边界、新 ID 最大值与非法表示。
- Actor 独立控制短测使用真实成功日终候选加载，验证当前缺席控制者保留控制且未自动获款、档中经济成员不获控制、旧代加入拒绝、确认一次零股份入场及控制者独占重置。
- 增加 `shared_market_restart` 独立短集成 case：真实文件 SQLite 的身份／控制 grant 关闭重开、重建 `SessionManager`，加载另一主体的真实周末成功日终经济档。验证原控制者缺席仍可控制但没有自动账户资金，档内经济成员不能控制，缺席主体确认一次零股份入场，原身份 token 仍有效。
- Actor 定向用例覆盖独立本人快照／工作单／确认、控制拒绝、动态入队及 scoped 广播；普通命令与 case 由根协调者执行10秒外部 deadline，多核并发。
- 新增真实忙 NPC case：真实 guest Auth、真实 Actor 注册与 production NPC verification gate，HTTP router oneshot 请求在 gate 尚未释放时登记非0玩家收据，比较实际 Player／NPC receipt ordinal，并验证仅一次受理及旧代／缺席／关闭入口拒绝。另有真实 `replace_timeline` 跨线程 lifecycle 锁及缓存／源代际负控，不用 JavaScript sleep 或时间戳冒充收据证据。
- 修复 scoped `enqueue_for` 等待 Actor 的 Q22 回归时，生产修先于新增忙 NPC 测试文件编辑；未执行业务红测，已向根协调者明确登记此 TDD 证据缺口，未声称旧路径已经实测失败。
- 本记录不宣称编译／短测／跨客户端最终验收已经通过；结果由根协调者集中登记。完整 diff 必须经非实施者独立复核。
- 首轮 `control-separation-build` 编译发现旧 generation 7 fixture 遗留二元 ingress tuple，已补主体派生 map；真实 SQLite restart 建议短测随后补写，等待统一编译及定向执行，不将源码补齐当成实测绿色。
- root 转述 host18：经济／控制分离与 scoped subject cache 两项短测通过；忙 NPC HTTP case 被十秒外部监督终止，不能计为通过。使用实际 binary 独立复现超时，并以 GDB 启动断点确认真实 NPC gate 已进入，测试的 Tokio gate轮询等待受到 owner 同步 tick 阻塞 timer driver 的影响。修为主线程有界轮询与独立 HTTP current-thread runtime 的250ms计时，保留真实 gate、receipt、一次受理及负控断言，等待新 binary 短测。
- root 转述 host19：真实重开 SQLite 的 `reopened_sqlite_keeps_control_grants_separate_from_loaded_economic_members` 已通过；本作者未独立执行该产物，仍需非作者增量复核结果后闭环。
- 本作者使用仓库进程树监督再次执行 busy case，外十秒终止；核对发现 host20 build 因其他 Engine libtest 编译错误 terminal101，Server binary 的16:31时间早于 fixture16:40修改，未产生当前成功 artifact。因此该次仅重复旧产物问题，不能作为新 fixture 修复失败或通过证据。下一轮必须绑定成功 compiler-artifact。
- host21 当前 Server artifact 的忙 NPC HTTP case 实际失败0.43秒；GDB 定位真实 `RuntimeDeltaMismatch`，不是 timer 问题。根因是 protocol `validate_accounts` 仍只接受 `AccountId(0)`；移除该硬编码而保留全部现金／冻结／持仓／T+1金融校验，新增非0合法与非法冻结／负现金专门短测。
- host23 成功 artifact 后本作者独立运行忙 NPC HTTP exact case，进程树10000ms监督、Rayon8、test threads4：1 passed、0 failed、0.46秒。HTTP 使用独立计时 runtime，不依赖 busy Actor 所在 timer driver；严守真实新成员非0账户和 receipt 断言。
- 调试 GDB 启动外壳超时后发现一个本任务创建的测试子进程未随外壳退出，已按精确 PID 强制清理并核对不再存活；不隐瞒该调试监督不足。后续实际短测使用仓库 `run-with-deadline.mjs` 进程树监督，不以 shell timeout 外壳替代。
- 默认入场资金增加独立零 fixture 测试，要求 `1_000_000_000_000` 分（100亿元）。记录时 `GameConfig::proposed_defaults` 仍为旧单玩家 `10_000_000` 分；已交根协调者先执行该真实红测，再修改其负责的默认配置与 Web 默认值。未伪称默认值已修或红测已执行。
- host25 成功 artifact 后本作者独立执行 Engine `memberships_tests` 实际8项：8 passed、0 failed、0.41秒，包含根已修100亿元默认值、共享 FIFO／T+1、越权撤单、零股份、配置不追补、重复入场、旧档确认及规范 member／owner ID。并行另跑 non0 `RuntimeDelta` 金融 guards 专门短测1/1、0.00秒；每命令外部10000ms进程树监督、Rayon8、case线程8／4。
- 本人金融 Event.account／事实实体 Account 的完整 u64 路径属于后续必要协议修复；其 host25 MAX case真实红，当前属性新源码尚未编入该轮产物，因此不把8项成员绿色扩张为全部跨层 u64 金融事件绿色。
- root27 成功 Engine artifact实际 `--list` 两个新 MAX真实链case，独立监督并发2项得到真实红：Snapshot JSON 在18446744073709551615账户 mapkey编码时拒绝 JSsafe，实际成交 step_frame 在 state_hash.serialization 因同旧 AccountId编码 fatal；2 failed、0.19秒。
- 根明确批准必要全类型根修后，`AccountId` 统一当前规范 u64 十进制字符串；Rust内部 u64不变，numeric JSON明确拒绝，无兼容／迁移。原Session严格codec原代码移至orderbook低层并复用，避免反向依赖；OrderId、seq、tick、Money单位不改变。
- 增加通用 AccountId值／mapkey严格边界测试（0、MAX_SAFE+1、u64MAX往返，numeric／前导零／符号／空白／小数／科学计数／overflow拒绝）；真实MAX成员的经济事实、FIFO/T+1、Snapshot/save/history/confirmations链等待根下一次编译实测绿色。旧密封见证未手改、未重新钉hash。
- host28 当前成功 artifact 后成员11项实际10 passed／1 failed，0.57秒：真实MAX Snapshot／SaveSlot JSON／history恢复case已通过；真实MAX成交case尚无成交。GDB核读真实 frame只有PriceTick与MAX本人OrderAccepted，定位测试 fixture只调用 `receive_private_intent` 登记NPC收据，却未将返回收据投递 `PendingNpcBatch`。已依现有真实跨来源receipt测试方式补完整NPC批次投递，仍保留100股初始化守恒、真实 makerMAX／takerNPC1、T+1、serialized Trade及恢复confirmations全部强断言，等待新产物。
- 同轮 PublicProjection5/5通过0.21秒，真实忙NPC HTTP1/1通过0.43秒，使用各命令10000ms进程树监督、Rayon8及内部多线程。没有把成员10/11写成全绿。
- host29 虽有成功 Engine artifact，实际 binary18:40:37早于 fixture投递补齐18:40:46，旧binary断言仍指向447而现source该行已为批次赋值；再次执行仍无成交只重复旧 fixture问题。没有把它归因新 fixture失败，已交根重编当前冻结源码。新增removed身份短case同轮 `--list` 不存在，未执行0-match冒充红／绿。
- host30 当前MAX真实 FIFO Trade／T+1／JSON／SaveSlot.restore／confirmations case通过1/1、0.55秒。host31 当前成功 compiler-artifact后本作者独立执行完整成员短组11/11绿色0.57秒，覆盖所有新增实际MAX链及严格 AccountId值／mapkey边界；与公开投影6case并行，Rayon8，test threads8，每命令10000ms外部进程树监督。
- 同轮 RuntimeDelta非0金融 guards与OwnerScopedRemovedOrder严格codec短组2/2绿色0.00秒。Web经济成员严格parser、全AccountId当前fixture／类型及非作者完整增量复核仍由根协调；没有执行完整回归，也没有更改旧sealed见证。

## 协作边界检查

- 指出 Native `renew` 在失败 ingress 替换前失效旧写代的问题；Native 作者改用 `renew_after`，先预检、成功替换后再发布 lease，并新增失败替换仍可保存下一日的真实短测。此为协作检查，不替代非作者完整 diff 门禁。
- 指出创建者先获 SQL grant、随后 writer 激活失败会遗留控制授权的问题；Native 作者调整为先完成 writer 激活预检，再读取／授予授权，最后注册无后续可失败步骤。
- `ProtocolSession::shared_ingress` 为只读封装入口，替换闭包不要求可变捕获；曾提出的闭包 `mut` 怀疑不成立，未修改源码。
