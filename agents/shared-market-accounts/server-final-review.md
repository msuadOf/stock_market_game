# Server 身份与共享市场接口独立复核

- 范围：Server 身份 CRUD、REST 身份和主体鉴权、共享市场 REST、Actor 成员/控制路由、直接 intent ingress、WebSocket 基线/事件/命令/heartbeat、archive 元数据控制；依据根 `AGENTS.md`、`docs/principles.md`、ADR-0030、ADR-0033 与 `agents/shared-market-accounts/server-identities.md`。
- 方式：检查当前工作区相关完整源码与改动，不运行 Cargo、不改 production 或 Cargo 文件。
- 验证边界：身份/REST/WS case 与多客户端旅程本轮未执行；不报告测试通过，不宣称功能验收完成。历史工作记录中所述的先前个别 case 结果不替代本轮复核。

## 发现

### [P1 初审发现；已由当前投影修复，跨宿主端到端仍未验收] WS 事件包含其他成员的账户与订单信息

初审时 `EngineUpdate::for_member_account` 未投影 TickBatch events，导致 `Trade` maker/taker 及其他账户订单/拒单事件暴露。当前源码已使用不带参与者标识的 `PublicTrade` 保留公开量价、用 `PrivateEventOmitted` 保留私有事件 seq，并同步投影 EventFact canonical payload 与 key；完整 TickBatch、CivilUpdate 和日终 intraday 有对应 Server case。此原 P1 在 Server 投影源码及当前记录的短用例范围内已解决，但尚无真实多客户端 Server 跨宿主端到端验收。

定位：`apps/server/src/actor.rs` 的 `EngineUpdate::for_member_account`；事件字段定义在 `packages/engine/src/session.rs` 的 `Event`。

## 已复核边界

- 凭据使用 32-byte 系统随机值、仅存 SHA-256 digest；密码走 Argon2id 独立随机 salt；认证 DTO/错误路径不回显密码或 token。注册及 guest 的主体与 credential 由 NativeDatabase 一次事务创建，登录新增独立 credential，logout 只撤销当前 token。
- REST 的账户/intent 接口从认证主体解析成员账户；`enqueue_for` 在 ingress 读锁下同时检查 generation 和主体到账户映射，未加入/缺席主体不会因空账户而取得其他人的账户。市场控制通过独立 controllers 检查，不先要求经济成员；context/baseline 对缺席主体允许返回公开市场且不提供账户快照。
- reset/load 会推进 generation 并替换 ingress 映射；控制身份保持独立于经济档。控制写操作和 archive rename/copy/delete/select 在 Actor 内检查 generation；archive 查询为只读，并先鉴权控制身份。
- 直接 `/api/intent` 与 WS SubmitIntent 都使用认证主体调用 `enqueue_for`，请求不接收 AccountId。A 股 Money 分与股份股语义未见在该身份/路由改动中漂移；本次没有交易制度变更。

## 待复核 / 未验收

- 增量复核：`send_publisher_frame` 在序列化前及发送前都重新鉴权；push flush/push clock/pull `GetFrame` 均调用此发送函数。Baseline 每条消息发送前鉴权，初始握手、Resync 与 WS 事件/命令/heartbeat 另有鉴权。`cached_private_frames_are_not_serialized_after_credential_logout` 覆盖 logout 后缓存 frame 的授权序列化 helper。该修复静态复核通过；未发现缓存帧路径绕过鉴权。
- 检视了 `.tmp/checklist-wave4/identities-host22/` 中的 12 个独立 `--exact` 日志：各日志显示各自一个 case 通过，其中包括 6 个身份、5 个 REST 和 1 个缓存帧 logout case。另读到先前 `.tmp/checklist-wave4/logout-cached-frame-red.log` 的 1 个真实失败证据。此结果仅代表这些 exact case，不代表完整 Rust 编译或完整回归。
- 初始 WS 事件隔离 finding 后续由公开事件投影修复，见本记录“公开事件与 AccountId 增量复核”；完整多客户端旅程和跨宿主端到端仍未验收。
- host31 投影短组记录显示 6/6 exact case 通过（`.tmp/checklist-wave4/public-event-host31-list.log` 与 `public-event-host31-green.log`）；根统一构建记录含成功构建和 `public_event_projection` compiler-artifact（`.tmp/checklist-wave4/host31-build.stderr`、`host31-build.jsonl`）。这些是针对列明短用例/构建的历史证据，本审查未运行它们。

## 公开事件与 AccountId 增量复核

- 依据 `agents/shared-market-accounts/public-event-projection.md` 复核当前源码：Trade 投影为只含 seq/code/price/qty 的 PublicTrade；其他主体私有金融事件替换为 PrivateEventOmitted；本人事件按成员保留；事件、EventFact.event、canonical_payload、EventStableKey 一起替换。缺席 viewer 的省略 key 使用 Session/Sealed/seq，本人 key 保留账户实体，匿名 PublicTrade 保留 Stock/Sealed 和原 local index。
- Actor 投影覆盖 TickBatch frames、CivilUpdate.events、`refresh.intraday` 的所有 TickFrame 事件及 facts，并过滤 snapshot/runtime delta account 和 working-order upserts；没有把 `Trade` 改为 AccountId(0) 或丢弃成交量价。现有 `apps/server/tests/public_event_projection.rs` 短测覆盖本人、其他成员、缺席控制者和日终 intraday；本轮只读源，未执行。
- 全局 `AccountId` 目前通过 `canonical_u64_decimal` 序列化为严格十进制字符串并仅接受规范字符串，`ts_rs` 类型为 string；这也覆盖快照/存档 map keys、内部 Trade maker/taker 等直接使用 AccountId 的字段。金融 Event.account 与 `EntityTag::Account` 显式使用同一严格 serde 与 TS string 类型。未见 numeric JSON 兼容分支。
- owner-scoped removed 的 Engine 生产、Server 投影与 Server 双账户测试源码现已复核；host31 exact 6/6 日志中包含真实双账户撤单 case。host31 Engine test-list/build 记录和 Engine/Server 源显示 `OwnerScopedRemovedOrder { id, owner }` 新结构已进入成功构建产物。
- 大 A 与范围：此次只匿名化参与人归属与编码账户标识；公开成交仍保留原价格与股数，撮合/费用/T+1 无语义改动。完整多客户端旅程和这项 removed 隔离仍未验收。

## 身份 JSON 错误响应安全核验

- `register`/`login` 使用 `Result<Json<PasswordBody>, JsonRejection>`，拒绝分支均丢弃 extractor rejection，只返回 `invalid_json()` 的固定通用字段；没有调用 `body_text()` 或插入 request 内容。`guest`/`logout` 对 `EmptyBody` 采用同一拒绝方式。
- 身份路由代码未将 body、`JsonRejection` 或 password 记录到 tracing；现有 `strict_payload_and_errors_never_echo_password_or_tokens` 已含 password number 和 unknown-field 样例，但未覆盖 password object/missing，也未完整逐路径检查所有标记输入。
- 因上述 extractor 与 response 源码路径，不存在“Serde numeric password 错误被 Axum 自动响应回显”的当前缺陷，不将一般 Axum 行为推断成漏洞。建议在 register/login 的真实 router case 增补 number/object/missing/unknown-field 变体并断言响应正文无输入标记，防止将来改成裸 extractor；本轮只读，未执行测试或日志测试。

## owner-scoped removed 增量复核

- 当前 Server actor 已按 `OwnerScopedRemovedOrder.owner` 过滤 `working_orders.removed`，不再仅对缺席主体清空；Engine 生成删除记录时从上一状态订单读取 owner，id 仍受 JS safe integer 校验，owner 使用全局 AccountId canonical decimal string。
- Server 新真实多账户取消用例构造两账户各自委托并同时撤销，分别断言两个成员仅收到本人 `{id, owner}` 与缺席 viewer 收到空列表，batch 仍通过原 validate。Engine delta 校验遍历带 owner 的移除对象并维持 reset/ID 唯一校验。
- 当前 generated 类型已由 Engine export 更新：`PlayerOrderDelta.removed` 是 `Array<OwnerScopedRemovedOrder>`，新类型 `{id:number, owner:AccountId}`，`AccountId` 为 string。Web parser 严格检查对象的 id/owner 字段及 canonical u64 owner；reducer 只删除本人 snapshot 中已存在且 owner 匹配的委托。
- `remote-ui-wiring.md` 记录 Root31 的实际 `tsc -b` 在 7.49 秒通过；`.tmp/web-shared-market-final-short.log` 记录跨 Save/MAX/membership/WASM/NPC/removed/PublicTrade 等 75/75；读取到的 host31 Server public projection 日志为 6/6。记录给出的数值限定于这些检查，不扩展成完整宿主集成结论。原 owner-removal P1 在 Server/Engine/Web 当前结构、parser/reducer 和记录的短测范围内关闭。
- 尚未验证跨 Host 的真实 E2E 撤单同步；本轮只读、未执行测试。完整 owner-removal 端到端与完整多客户端旅程仍未验收。
