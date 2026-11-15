# 日终归档：从观察流分离的三宿主设计

状态：待实现的架构附录。范围仅为归档所有权、交接和错误语义；历史粒度由共同业务层决定，不在 LibOS 分叉。用户已确认已结束日只留日 K 与日级状态；此设计不擦除旧存档、不改变交易规则。

## 已验证边界和仍须验收的内容

- Browser 真实 Chromium、Native 测试已经能证明：停止接收或确认观察流后，权威仍可越过日结并进入下一个交易日。此测试不依赖先迁移归档，也不能证明归档独立。
- Browser 和 Tauri 的当前 `DrivePort::stage_civil`（`apps/web-wasm/src/market_runner.rs`、`apps/desktop/src-tauri/src/actor.rs`）只交接观察事件。真正写快槽发生在 `apps/web/src/App.tsx::connectProtocol/persistCheckpoint`：观察事件到达后才 `host.save(reference)`、校验、写压缩 IndexedDB 快槽和授权文件；基线重同步只补最后一次引用。因而永久停收包时，现实现没有持续写快槽的保证。
- `packages/engine/src/session/protocol/civil/session.rs` 目前在每个完成的自然日，将 `Arc<SaveSlot>` 放入无界 `pending_save_candidates`，并保留最近的 `day_end_save`。Browser/Tauri 当前只有 UI 捕获才从 pending 移除；重同步只读最新会遗留旧的候选。虽然短跨日已通过，但停收包时间足够长时可能积累完整旧候选，故不能据此宣称长时间推进所需内存有界，或宣称全量重构已经验收。给旧 pending 加容量并直接丢弃项会违反归档约定。
- Native Server 的 `actor.rs::stage_day_end` 已在观察者处理前提取同版 Arc，经 `actor/archive_lane.rs::ArchiveLane` 上的独立有界 FIFO 持久化，并回送写回执；但队列策略、错误分类与会话换代管理仍只在 Server。这是共同合同的参考实现，不是跨平台证明。
- 测试用 `apps/desktop/src-tauri/src/actor/archive_tests.rs` 描述了 Desktop 无客户端保存的 SQLite 行为，但当前未接入 actor test 模块，并引用已不存在的 `archive` 字段。不能将它作为已经运行的证据；按新的异步回执合同重接。

## 共同合同与改动位置

| 位置 | 目标 |
| --- | --- |
| `packages/engine/src/session/checkpoint.rs`（新增） | `CheckpointKey { generation, writer_epoch, seq, tick, settled_date }`、`CapturedCheckpoint { key, slot: Arc<SaveSlot> }`。共享控制器将完成日结的已验证候选提取成不可变 Arc，为每个活动归档绑定验证严格顺序、接收入队结果、识别陈旧回执和故障归类。配置决定目标能力，不按 native/wasm 判定。 |
| `packages/engine/src/session/protocol/civil/session.rs`、`session/runner.rs` 及共享单步入口 | 在共享的 civil commit 交接点为每个成功自然日提取恰好一个候选（含休市日），先暂存候选再交接观察事件；失败日结无写入。观察路径不得消费候选。单步、自动与显式 civil advance 必须走同一提取合同；处理 view 更新失败与存储失败互不污染。 |
| `packages/libos/src/`、`packages/libos/browser/` | 提供泛型 `try_send`/独立有序 I/O lane、读写事务与唤醒/关闭，Native SQLite 和 Browser IndexedDB 只实施机制。边界传不可变引用或小令牌，不在 authority 序列化、压缩、做恢复校验，不持业务锁等待存储或观察者。 |
| `packages/native-store/src/lib.rs`、Server `actor/archive_lane.rs`、Tauri `actor.rs` / `lib.rs` | 两个 Native 宿主共用一套 archive 执行器和已存在的 SQLite archive schema/校验。保留 `ArchiveWriter` 在事务内的 lease/seq 检查，不将其替换成只在宿主回执处比较。SQLite、路径/文件访问可由 LibOS Native 模块包装（应用只选取获准配置、传入狭窄端口）；SQLite writer 内部业务校验移成共同层。 |
| `apps/web-wasm/src/market_runner.rs`、`lib.rs`、Browser LibOS Worker | 把共享内存中冻结的 checkpoint 放入**独立存储 Worker** 专用有界 FIFO，由该 Worker 取令牌并完成校验/序列化/IndexedDB 事务；它与 authority 和 publisher 都不同 Worker。不允许发布队列拥塞、发布 Worker 崩溃、ACK 永不抵达时阻止存储。复用 LibOS tombstone、generation 和 lost-wakeup 守则，启动须在任何日结前注册归档通道。 |
| `apps/web/src/App.tsx`、`app/AppRoot.tsx`、`app/useSaveCommands.ts`、`app/useSessionHostLifecycle.ts`、`save/day-end-persistence.ts` | 去掉观察者 onApplied/onBaseline 对自动快槽写入和候选消费的所有权；UI 只显示带 generation 的独立回执、发显式读档/选择目标命令，读档等该 I/O FIFO 内此前已排定的必要操作。提供独立的授权文件目标通道，不能依赖行情观察流来触发。 |
| `apps/web/src/save/indexed-db-save-repository.ts`、`save-repository.ts`、`save-file.ts` | 现有验证与 gzip 编解码可复用/提升至公共存档层，不复制 JS 业务。IDB 映射包装进 LibOS；文件获取授权留 UI。未获授权不写文件，API 不支持重复覆盖时显式告知，不能用下载队列冒充。 |

## 交接、租约、失败

1. 日结产生先前已有的已验证 `Arc<SaveSlot>`，使用同版 `seq`/自然日；只把轻量消息交给有界 FIFO。为无归档配置定义显式“不启用”端口，不保留永久等待不存在 UI 的 pending 条目。用户显式导出最新成功日结可通过只读 Arc 引用/当前日结档，不必借用旧的一个消费者出队接口。
2. 同一目标只有一个活动 writer lease；入队项保存 generation/lease。写入和租约的原子比较在持久后端的事务中完成：SQLite 复用 writer lease 和 seq；IndexedDB 在同一 readwrite transaction 内读取活动租约记录并校验 epoch、目标身份与 seq，再 put/commit。仅 JS 的 `isCurrent()` 闭包无法跨 Worker/标签页建立这个围栏。新局、恢复、选槽、停止采用同一个换代 FIFO 命令，过期回执不能改变新局状态。自动推进不等待该队列的完成回执；显式读档或切换目标添加顺序屏障后可等自身回执。
3. 发送成功只意味着“已排队”。只有事务成功才是“已写入”；结构、权限或 I/O 失败区分展示并保留上一个有效档。归档满/故障允许共同业务策略在已提交边界显式进入归档故障，不与 observer overflow 混成一个状态；保留失败候选或明确可恢复范围，禁止盖掉旧档、伪造最新已耐久游标、或为下一天无限扩容。旧租约被拒视为替换取消，不等于新会话的写失败。
4. 观察流代码不接收上面任何耐久 ACK 或 I/O 阻塞；I/O 失败用独立、代际隔离的控制回执报告，未来 UI 重连可读当前归档状态。测试必须区分“只有前端不收包”与“存储故障/满队列”；仅后者受归档策略限制。退出关闭分别 drain/停止三条 lane，不用发布 Worker ACK 清理归档。
5. 授权文件是显式配置的第二目标，成功/失败逐个报告；快槽已经提交时不能把授权文件失败解释为快槽未写。Browser 文件句柄如果能在 Worker 中继续使用，授权由 UI，传句柄后在独立 lane 做事务与 close；依赖 Tauri 文件授权 UI 的平台可走 LibOS Native I/O，有效作用域必须在真正提交前核验。无法与撤权原子协调的文件后端须明确定义“已开始写事务”的生效边界，不能宣称事后撤权能收回已完成的 I/O。

## 格式兼容与切换

- 保持 Browser 现有数据库/存储键、`gzip:` 值、严格存档格式；新 lease metadata 用同一事务可读的独立键，不把旧存档重写成不同正文字段。保留现有“主库无值才读有效旧 LocalStorage 同格式槽”的只读入口；坏档明确错误，不向下 fallback。
- Desktop 当前快槽实为 WebView 内现有 IndexedDB，而 ADR-0035 指定目标是 SQLite。不要默认丢失已有内容：在 SQLite 槽缺失时支持一次性恢复入口只读旧快槽（验证后供显式恢复或受控选槽）；仅下一次成功自然日更新新 SQLite，旧 WebView 数据不删除不覆盖。现有分支仍可读取旧存档；若未来要转换存档结构，另立迁移设计。
- 保留 ADR-0025 的自然日日结触发、日内不写、逐目标报错和旧代际不得覆盖。新日终快槽不再保留已结束日分钟序列，兼容字段为空；旧档仍能读取，不执行原位置回写或批量迁移。日 K 和成交、账户、费用等权威事实不因分时清理而删减。
- 与旧 ABI 的显式导出保持逻辑兼容。UI 不再自动用 `save_candidate` 自快槽取并消耗；对仍需请求精确历史候选的生产/测试调用，逐一迁移为从明确权限槽读取已耐久档、只读最新 Arc 或测试夹具接口。禁止在同一个目标上同时启用旧观察者写法和新归档 writer。

## 联调验收

1. 共享短测：日内无任务；同一自然日成功只准确入队一次；跨周末每个自然日按权威游标提交；失败日结零入队；旧 generation / 旧 epoch / 不增 seq 的写入和回执不影响新局；队列满暴露明确故障。覆盖手工/自动生成相同序列。
2. 三端同一短 fixture：启动后一条观察更新都不确认，至少跨两个日结；用读档 API 或直接后端校验最新 `settled_through`/tick/seq/存款与日 K，比较手快观察者的基线事实，确认存档成功与 UI 无关；检查 pending 不随日数增长。Native 跑真实 WS，Browser 跑真实 Chromium 内的 SharedMemory WASM+IndexedDB，Desktop 跑接入 Tauri actor 和 SQLite 的测试后，再用真实图形环境验证产品链路。
3. 逐端注入仅存储慢、仅发送慢、只关闭观察者、事务失败、替换发生在旧事务写入前后。断言延迟与异常只在对应 lane，发送慢不减权威推进；失败后可读上一份有效档；不得无界缓存。真实 Browser 必须在仍扣留 UI ACK 时，通过独立 IndexedDB 读取事务确认已提交的槽正文、元数据和 current 指针，核对日结日期和日 K；然后走正常产品读档流程验证重开恢复。写入/选槽竞态单列：让旧世代写入与恢复或换槽交错，事务完成后检查当前指针和新档正文均未被旧写入覆盖。不要只凭 Worker 回执判断落盘。
4. 使用项目 10s/300s deadline 规则。同负载报告权威推进吞吐、发布延迟与归档 I/O 耗时；依据真实运行，不把新档序号跳跃当作游戏吞吐。完成整批代码之后统一编译、端到端验证、独立审查，再修联调发现。

## 读盘按需分页是一条独立的完整接线

现有 `MarketHistory` 三端都调用 `GameSession::market_history_page` 或带账户读记录的变体。日 K 存在 `SaveSnapshot.daily_candles` 并在恢复时进入 `SessionCandleBook`；当前查询按需生成页面，但仍读取内存中的已恢复/已结算日 K，并非直接从 I/O 后端分页。 `NativeDatabase::load` 只读取、反序列化和校验完整存档，不能作为高频历史查询接口。Browser 快槽也是整档 gzip。若需按请求读取真正持久化的日 K：

- 定义同一个不依赖 I/O 的历史分页规则（查询合法性及已结算日状态、日级区间、排他游标、稳定读取水位），权威侧仅授权、记录现有 `history_reads` 副作用并提交细小查询描述（generation、writer lease、stock、checked date range、精确游标）。读取结果在回复时以活动 generation 校验，不通过堵住市场等待结果。
- SQLite 以归档槽+时间线+证券+自然日为键保存严格日 OHLCV/开市状态；IndexedDB 对应独立 object store。日级写序与主快槽更新同一次事务，遵循选择槽/恢复的 lease。同一页面的结束水位取该存储事务视图；日结已提交、但归档写入未完成时需如实区分，不能拿内存值冒充已写盘。老档初读用兼容投影构造页面或显式受控建立索引，不静默修改原始存档。
- 接线：`apps/server/src/actor.rs::MemberRequest::HostQuery`、`apps/desktop/src-tauri/src/actor.rs::SessionCommand::HostHistoryQuery`、`apps/web-wasm/src/lib.rs::market_history` 及 `apps/web/src/host/wasm-worker.ts`。三端使用共同分页转换与校验，异步存储机制由 LibOS 提供，`CurrentMinuteHistory` 仍走当日内存。无归档部署不冒充耐久读取，须明确配置能力；用同一接口返回有来源/水位约束的结果。
- 高频页面不得解压整档、重建世界、或扫描全部股票。Snapshot 当前也对外带日 K 序列；若要把旧日 K 从常驻内存挪出，需同时限定 Snapshot 的最近窗并保留策略所需的近窗，补入加载前史、持久化恢复后日序连续及旧客户端兼容验收。
