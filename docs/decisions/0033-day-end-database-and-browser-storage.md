# ADR-0033：统一日终存档接口与宿主存储介质

- 状态：accepted（用户已确认方向；代码和验收尚待逐项实现）
- 日期：2026-10-05
- 决策者：用户；AI 记录
- 关联：ADR-0025、ADR-0027、ADR-0029、ADR-0030、ADR-0031

## 运行位置与存储位置

统一日终存档接口，不把统一接口解释为所有平台使用同一种数据库。

| 运行模式 | 交易模拟位置 | 自动日终持久化 |
|---|---|---|
| 浏览器本地 WebUI／静态 Pages | 访问者浏览器的 WASM | 访问者浏览器的 IndexedDB |
| Rust Server | Server 内存中的 Engine | Server 文件系统中的 SQLite |
| Desktop 本地 | 本机 Rust Engine | 本机文件系统中的 SQLite |
| WebUI／Desktop 远程连接 | 远程 Server 中的 Engine | 远程 Server 的 SQLite |

远程 WebUI 不运行浏览器 WASM 交易模拟，也不把 Server 的日终档另存到客户端 IndexedDB
冒充服务器持久化。静态 WebUI 服务所在机器不是浏览器本地局的模拟或存档位置。

## 生命周期与接口

每个成功自然日日结自动写入当前存档槽；交易日必须先完成交易日收尾。休市自然日仍沿
ADR-0025 的成功经营／披露日结边界保存，不能以交易所休市为由丢失已推进的公司状态。
日内只运行内存事实，不写订单、待受理请求、临时挂单或更正队列，不启用 WAL。

打开游戏或用户明确选择其他存档时才从存储载入市场；随后继续使用内存，不因每次 tick、
暂停、恢复网络、读取列表或自动日终保存重新载入市场。接口区分存档元数据查询、明确加载、
成功日终候选写入及槽位管理；读元数据不等于重建正在运行的市场。

写入使用完整成功日终候选和事务。失败明确展示并保留上一份有效档，不先报保存成功，
不 fallback 到旧 LocalStorage、临时 JSON 或其他未选择介质。换局／换档后的旧异步写入
不得覆盖新时间线。手动 JSON 导入／导出仍可作为明确的文件操作，不能代替自动数据库存档。

只接受当前完整 SaveSlot 和当前数据库结构；旧档失效时明确提示，不引入 schema 代际、
旧字段默认补齐、兼容或迁移。不可用的 IndexedDB／SQLite 明确报错，不静默换存储。

## 实现方向

Native 使用独立适配层的共用 SQLite repository，Server 与 Desktop 复用，不让 Engine
依赖 SQL、文件系统、网络或 UI。SQLite 通过 `rusqlite` 的 bundled SQLite 集成，保证
部署成品不要求另外安装数据库服务；不使用 WAL，也不设置可自动迁移旧数据库的版本链。
浏览器适配器使用 IndexedDB 原生 API，在同一存档语义下完成异步事务与生命周期隔离。

### Native 独占 writer 与平台锁

同一物理 SQLite 文件只允许一个 Native writer owner；同 owner 的 Arc clone 共享连接与事务。
独占锁必须绑定真实文件 inode／Windows 文件身份而不是单纯路径字符串，第二连接、第二进程和
hard link／symbolic link 别名均须显式拒绝；最后一个 owner clone 析构才释放锁。
先用不截断文件的 OpenOptions 打开并取锁，再运行 SQLite bootstrap，不对锁句柄写数据库内容。

Unix 平台采用标准库 `File::try_lock` 的非阻塞 `flock`。Windows 不能直接锁整份 SQLite 文件：
Rust 1.96.1 标准库 Windows 实现使用 `LockFileEx` 锁定从 offset 0 起的整文件范围，而 Windows
byte-range lock 可阻止 SQLite 另一 HANDLE 的正常读写。Windows 适配层因此使用
`windows-sys 0.61.2` 的 `LockFileEx`，在 offset `i64::MAX - 1` 锁定一个 byte，配合
`LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY`；不写该 byte、不增长文件、失败不 fallback。
该 offset 远离 SQLite 标准 `PENDING_BYTE`／`RESERVED_BYTE`／`SHARED_FIRST` 锁区（1 GiB 附近），
也高于 SQLite 最大数据库范围（约 281 TB，最多 `2^32 - 2` pages，每 page 最多 65536 bytes）。
同文件不同 HANDLE 仍竞争这一个应用专用 byte，HANDLE 关闭时由系统释放锁。

以上依据是本机固定 Rust 1.96.1 的标准库平台源码及 `libsqlite3-sys 0.35.0` bundled SQLite
amalgamation；属于宿主文件并发控制，不改变任何 A 股制度或经济存档格式。
`windows-sys` 仅用于 Native 适配层目标依赖，不进入 Engine。源码复核和 Linux 短测不能替代
Windows／macOS 实际运行验收；未运行的平台必须明确记录为未验收。

登录身份与市场档案仍按 ADR-0030 分离：用户名＋密码及匿名主体不等于持仓，重复登录
不发放资金。账号凭据的持久化不能借机落盘日内市场状态；密码只保存安全哈希，不保存明文。
当前市场控制授权也独立于日级经济账户存档，加载其他档不得撤销当前控制主体的权限或
从导入档授予新控制权。缺席主体只有在本人确认重新加入后才创建资金账户。

Server 身份适配使用 `argon2` 的 Argon2id 与每个密码独立的随机 salt；`rand_core` 的
系统随机源产生 256-bit Bearer credential，`sha2` 只对高熵 credential 作 SHA-256
摘要后持久化。三个依赖只属于 Server 安全适配层，不进入 Engine；不能以快速摘要代替
密码哈希。SQLite 的身份表独立于市场日终槽，退出只撤销当前 credential，不删除主体或持仓。
REST 只在注册／登录成功响应交付 credential；请求密码、credential 及密码 hash 不进入
日志、Debug 或错误响应。实际 HTTP 不提供传输保密，公网部署建议在反向代理使用 HTTPS。

本决定登记产品方向与必要适配层路线，不宣称数据库、多人账号、云同步或全部宿主已经实现。
