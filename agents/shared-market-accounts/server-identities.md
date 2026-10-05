# Server 身份服务接线记录

## 当前契约

- `POST /api/auth/register` 严格接收 `{username,password}`，成功 201 `{subject,token}`。
- `POST /api/auth/login` 使用同样字段，成功 200 `{subject,token}`；每次生成独立 credential，主体不变。
- `POST /api/auth/guest` 严格接收 `{}`，成功 201 `{subject,token}`。它表示明确创建新匿名主体；刷新或重连应使用已有 token 调用本人查询，不重新调用 guest。
- `GET /api/auth/me` 使用 `Authorization: Bearer <token>`，只返回 `{subject_id,username}`。
- `POST /api/auth/logout` 使用同样 Header 与 `{}`，成功 204，只撤销当前 token，不删除主体、其他登录或市场持仓。
- `username` 大小写敏感、1—64 个 ASCII 字母／数字／下划线／短横线；密码 8—1024 个 UTF-8 字节，输入限制在 Server 显式校验。

## 存储与安全边界

身份 SQL CRUD 复用 `NativeDatabase`，在与市场日终槽分离的 `subjects`、`credentials` 表执行。
注册及匿名创建主体与 credential 使用一次事务；已有用户名不会被替换。密码使用独立随机 salt 的
Argon2id PHC hash；系统随机源产生 256-bit token，仅 SHA-256 digest 落 SQLite。
登录 payload、LoginResult 和 SubjectRecord 不实现 Debug；错误不带外部 payload 或底层 SQL 信息，
JSON rejection 不直接回显 serde 错误中的未知字段或输入。HTTP 不提供传输保密，公网建议 HTTPS。

身份不发放资金、不加载市场、不删除账号。AccountId 与 capability 必须由经过认证的 subject 和
市场成员关系解析；身份 REST 本身不接受客户端 AccountId。对应 actor 接线由市场成员实现作者负责。

## 共享市场 REST 当前契约

- `POST /api/new` 使用身份 Bearer，接收 `{setup,seed}`，只有 Server 尚无活动市场时创建并绑定 creator；已有市场返回 409 `MARKET_ALREADY_EXISTS`，不得为每个主体暗开独立市场。
- `GET /api/markets` 使用身份 Bearer，返回 `{markets:[context]}`；只读元数据，不加载市场、不发放资金。
- `GET /api/market/context?session_id=...` 使用身份 Bearer，返回该主体的完整 context；不自动加入或发放资金，供 WS timeline 切换后重新同步实际设置与本人关系。
- `POST /api/markets/join` 接收 `{session_id,generation,confirmed_rejoin}`，根据同一主体显式加入；重复加入不重复拨款。
- `context` 为 `{session_id,setup,seed,resumed,generation,member,can_control,needs_rejoin}`；`member` 可以为 null。`generation`、`seed` 为规范十进制字符串。
- `POST /api/market/admission-cash` 接收 `{session_id,generation,admission_cash}`，金额为分字符串，仅市场控制主体可修改，成功 204，只影响之后入场的玩家。
- `POST /api/market/reset` 接收 `{session_id,generation,setup,seed}`，显式重置所有玩家所处的共享市场；保留同一个 session_id 和独立控制授权，但失效旧 timeline 请求，客户端必须先明确告知全局影响。
- `POST /api/speed`、`POST /api/running` 和 `POST /api/load` 现在必须携带当前 `generation`；`DELETE /api/session` 查询也必须携带 `generation`，明确销毁整个活动市场，不能作为普通 logout 或 host.dispose。
- 本人 snapshot、working orders、trade confirmations 和 SubmitIntent 均由认证主体解析账户；请求不能附加任意 AccountId。

控制授权持久化在独立 SQL 表，不从 SaveSlot 经济成员生成、撤销或恢复。Controller 即使不在
载入存档的资金账户中仍可控制市场，但必须显式重新加入才能交易；客户端需要展示 `needs_rejoin`，
不能自动重新发放资金。控制鉴权不得先强制要求资金成员。公开市场只读接口可以由已登录但
未加入的主体访问，本人金融接口仍要求成员。WS 只发布本人金融投影，同一 timeline 缓存
AccountId；timeline 切换先要求 resync，再原子解析新 baseline 和本人账户，不复用旧账户 ID。
每次事件、客户端命令和 heartbeat 检查 identity credential；logout 后停止私有流。

## 验证诚实边界

先添加真实 REST 测试，但 Root 集中 Rust 编译尚未执行时已继续实现，缺少真实红编译输出；
不能将静态上未存在模块冒充已执行失败测试。Root 统一编译与 10 秒进程外 case 验证，
独立完整 diff 复核尚待安排，本文不宣称功能验收完成。

2026-10-05 使用 Root 编译的 host19 fresh binaries 实际执行：先两个 binary 并行 `--list`，
确认身份 6 case、共享市场 REST 4 case；随后 10 个独立进程并发，每个 `--exact`、
`--test-threads=1`、`RAYON_NUM_THREADS=2`，进程外 `timeout --signal=KILL 10s`。
身份 6 case 全绿，REST reset 与经济档授权分离 2 case 绿；其余 2 case 红，因为测试 helper
错误地将严格 Query 拒绝产生的 Axum plain-text 400 当 JSON 解码。已修 helper 按 Content-Type
保留非 JSON body，不改变 400 断言；尚待新 binary 验证。原始日志保留于
`.tmp/checklist-wave4/identities-cases/`。新增第五个 REST case 锁定 archive rename/copy 的
generation 与独立控制授权；write RPC 现在都经过单个 Actor 命令原子检查，不直接绕过 Actor 写库。

独立复核发现 logout 后 push clock 仍可能交付缓存的 PublisherFrame。host21 Server lib fresh
binary 的清单确认新增 `cached_private_frames_are_not_serialized_after_credential_logout` 存在，
随后 10000ms 进程外 exact 测试实际 exit 101，日志 `.tmp/checklist-wave4/logout-cached-frame-red.log`。
根修在 PublisherFrame 序列化前及实际发送前检查 credential，覆盖 push clock、flush 和 GetFrame；
Baseline 每条消息发送前也检查 credential，失败显式 gateway error 后结束连接。尚待下一 fresh
binary 验证与非作者再次复核；已经交给网络栈的旧消息不能被此检查召回，不冒充传输撤回。

host22 fresh binaries 的清单再次确认身份 6 case、共享市场 REST 5 case 与上述缓存撤销 case。
身份与 REST 的 11 个独立进程使用 `RAYON_NUM_THREADS=2`、`--test-threads=1` 并发运行，
每个 exact case 受进程外 `timeout --signal=KILL 10s` 约束，最终批次 wall 为 1.78 秒，
11 case 全绿；缓存撤销 case 单独真实绿色 exit 0、0.00 秒。
原始日志位于 `.tmp/checklist-wave4/identities-host22/`。首次 shell pipeline 未正确等待
subshell 的后台任务，不能作为完整结果；随后修正为直接 job loop 并完整重跑全部 11 case，
只以最终已完成日志计入结果。非作者增量复核已请求；金融 events 跨主体泄露的另一 finding
由市场投影作者另行修复，本文不将上述 12 case 绿色当作该 finding 已关闭。
