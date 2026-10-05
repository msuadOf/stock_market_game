# Native SQLite 日终存档实施记录

## 契约与范围

- 依据 ADR-0025、0029、0030、0033；不改 A 股 Money（分）、股份（股）、T+1、费用、受理或竞价规则。
- `packages/native-store` 是独立 I/O 适配层；Engine 不依赖 SQLite。Server 与 Desktop 共用 bundled SQLite，不要求 SQLite 服务。
- 只保存成功完整自然日日结候选；交易日完成收尾后保存，休市日完成经营与披露后保存。日内不写订单、ingress、更正队列。
- 数据库只接受当前完整结构，不加 schemaVersion、迁移链、旧字段默认或其他介质 fallback；journal 使用 DELETE，不使用 WAL。
- 登录身份与市场存档独立；身份 CRUD 由身份实施 Agent 在同一数据库连接上实现。

## TDD 与验证状态

先写 `native-store/tests/archives.rs`、Server `native_archives.rs`、Desktop `actor/archive_tests.rs`。
夹具来自真实 `ProtocolSession` 的周末自然日日结，覆盖物理 reopen、多槽管理、事务失败保旧档、时间线隔离、日内 checkpoint 拒绝及当前结构拒绝。
编译与 Cargo.lock 由 root 集中安排；实施者未自行运行 Cargo、生成绑定或 Git。

### 实际红绿证据

- `native-store-build-red.jsonl` 首次编译被并行 Engine `memberships` 粗边阻断，不作为有效红测。
- `native-store-build-red-2.jsonl` 编译成功（45.63 秒）。实际 `archives-72e95318a0040d04` 的五个 exact case
  各由进程外 10000ms supervisor 运行，五进程并行、每进程 Rayon 8：四个正例因 repository 明确 stub 真实红，旧结构拒绝负控绿。
- `native-store-mixed-*.log`：八个 repository 既有 case 绿；两个新增选择元数据 case 因明确 stub 真实红。
  `native-host-red-*.log`：Server 两个 case 真实红（没有自动档、没有写失败广播）。
- `native-desktop-red-*.log`：真实 Desktop 二进制两项 exact case 真实红（没有自动档、写失败通知为空）。
- root `host19-binaries.json` 当前产物实际执行：十项 repository 与四项 Native Host 全绿；新增活动槽删除续建 case
  因明确 stub 真实红，随后才实装。各进程外 deadline 为 10000ms、Rayon 8，十五进程并行批次耗时 3.92 秒。
- `native-p1-red-*.log`：非作者发现 repository 候选类型与独立物理连接 writer 隔离缺口后，
  先取得两个真实红测（第二物理 open 未拒绝；typed candidate capture 为明确 stub）；已有普通日内快照拒绝负控本来绿，不冒称真实红。
- `native-host22-*.log`：实际十四项 repository、三项 Desktop、两项 Server 全绿，二十进程批次耗时 5.11 秒，
  每进程 Rayon 4、外部 10000ms deadline。独占锁父 case 真正启动第二测试进程（继承父批次 deadline）；
  ignored child helper 不当作额外普通测试通过数。
- `native-cli-recovery-red.log`：已批准显式 CLI 恢复入口先因 unknown option 真实红；
  `native-cli-recovery-green-*.log`：root host23 当前二进制三个定向 CLI case 全绿，三进程并发、每进程 Rayon 8、外部 10000ms。
- `native-lease-red.log`：非作者发现旧 writer 可 renew 抢占新 owner 后，先取得旧 owner 被错误接受的真实红测；
  在同 generation mutex 的最前方增加 owner guard 后，`native-lease-owner-green.log` 实际一项绿。
- `native-metadata-owner-red.log`：同级入口审计发现旧 writer 的 rename／copy／非活动槽 delete 仍可修改元数据；
  真实红测后统一在 generation mutex 下先检查 owner，再执行 SQL。删除复用不重入 generation 锁的 private SQL helper。
- `native-host26-*.log`：实际十六项 repository 普通 case 与一项 writer byte 常量 unit 全绿；
  十七进程并发、每进程 Rayon 4、外部 10000ms，批次耗时 2.99 秒，ignored child 不计普通 case。
- `native-tauri-metadata-red.log`：非作者追踪到 Desktop rename／copy transport 直接操作 DB 的旁路，
  先写真实 Tauri IPC stale generation／替换 owner 失败测试并取得红（0.88 秒），再改为 generation-bound Actor 单命令。
  `native-host27-archive_metadata_ipc_rejects_stale_generation_and_replaced_session_owner.log`：实际当前 root27 产物该 IPC case 绿，
  并同时复跑三项 Desktop archive case 全绿；四进程并发、每进程 Rayon 8、外部 10000ms，批次耗时 1.64 秒。
  前端 generation 透传由浏览器作者记录真实红后 25 项短绿（`tauri-mutation-{red,green}.log`）。
- `native-windows-bounds-green.log`、`native-host25-bounds-green.log`：Windows 专用 writer byte 的常量范围短测实际一项绿。
  此测试在 Linux 上验证边界常量，不代表 Windows LockFileEx 已在 Windows 执行。
- Windows 专用 writer lock 新源码与 bounds case 的后续编译／绿测、最终非作者完整 diff 复核仍由 root 安排；
  不把此批短测扩大为完整回归、Windows／macOS 实际运行或生产发布验收。

## 已实现生命周期

- 首次 SQLite bootstrap 事务建立当前五张完整表；现有结构须精确匹配，不兼容、不迁移、不使用 WAL。
- `archive_slots` 只保存经过真实 `ProtocolSession::restore` 深校验的完整自然日日结候选；事务保留旧有效档。
- `subjects`、`credentials`、`market_control_grants` 是独立身份／授权元数据。控制授权不从经济存档授予或撤销。
- `archive_selection` 是启动槽元数据：Engine 读档成功后显式记录选择；选择失败不谎称经济状态回滚。
  删除已选槽清空 pointer，不默选其他档；删除活动保存目标后只建立新 UUID 内存 writer，下一个成功日终才写新档。
- writer lease、seq 与 Actor generation 拒绝旧时间线操作；切换 ingress 失败不失效旧 writer。
  经济写入只接受 `DayEndCandidate`：私有字段、不支持 Clone／Deserialize，唯一 capture 路径来自真实成功 CivilUpdate key。
  物理文件强制 single Native writer owner，真实文件身份上的非阻塞 OS 锁持有至最后 clone 析构；
  Windows 使用独立 byte-range lock，Unix 使用 flock。这个协作锁不宣称阻止任意外部非遵约 SQLite 软件修改文件。
- Server CLI `--database` 指向真实文件（默认 `data/stock-market-game.sqlite`）；启动恢复一次已选档。
  删除启动槽或坏档不会静默新局；可明确 `--archive-slot ID` 或 `--new-market` 恢复选择，二者互斥且不删除身份／授权。
  Desktop 生产使用 appData `stock-market-game.sqlite`，读取已选档一次并向前端返回真实 setup／seed。
- 日内内存不 reload SQLite；元数据列表不重建市场。自动日终保存由 Native Actor 自行触发，不依赖客户端保存请求。

## 限定交付门禁

非作者已在 `native-final-review.md` 完整复核并关闭 candidate、物理 writer、renew、metadata 和 Desktop IPC 的有效发现，
当前限定 Native 存档范围 PASS。实际短测为 repository 十六项、writer byte 范围 unit 一项、
Server 日终档两项、Desktop 日终档三项与真实 metadata IPC 一项、CLI 三项；跨批次当前产物与红绿路径见上文。
完整 workspace 回归、Windows／macOS writer-lock runtime 及 Server 身份安全 routes 的另一独立复核不由此记录代替。
