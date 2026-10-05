# Remote 身份、共享市场与 UI 接线记录

## 已接线范围

- Web／Desktop 使用同一启动页选择本地或填写 Server URL；本地不要求登录。Remote 分支不加载 Worker／WASM，不读取浏览器游戏 IndexedDB 档。
- 独立 credential IndexedDB 按 Server 基地址保存 Bearer；不保存密码。remembered credential 刷新后查询 `/api/auth/me`，失效明确报错，不偷偷调用 guest。匿名身份丢失 credential／退出不可恢复。
- 注册、登录、guest 成功后仅列公开市场；创建、加入与缺席后的重新加入都是本人明确操作，无邀请码。没有资金 member 的 Controller 可直接进入公共市场和控制界面。
- RemoteHost 接受身份 token 和已选择的 Server context，不自动 `/api/new`，不用 legacy/session token 或 `local-player` fallback。实际 setup、seed、resumed 来自 Server；换 generation 时先查询 context 再交付 baseline。
- member 的规范 u64 `account_id` 决定本人资产、持仓、活动委托和事实刷新；不存在 member 时不冒充账户 0。`can_control` 与资金账户独立，控制界面按能力禁用，REST／WS 仍须 Server 权威鉴权。
- 登录身份、市场控制授权与日终资金档独立。load／reset 全市场影响需明确确认；缺席后不自动发资金，只有本人明确重新加入才申请一次入场资金。
- start 仅订阅 Remote 市场；离开、隐藏页面和 dispose 不暂停、不删除共享市场。显式暂停／继续、倍速、加载与重置绑定 generation；同一 host 的旧 generation 控制成功或失败不能回写新 timeline UI。
- Remote 日终档继续由 Server 保存；客户端游戏 IndexedDB 不承接 Remote 档。明确 JSON 文件操作与自动存储分开。数据库选槽沿共享 ArchiveStore 的恢复后 select 两阶段契约。

## 实际验证与边界

- 身份／context 与独立 credential 存储、真实 React SSR 交互：16 个定向 case，通过；其中失效 credential 不调用 guest、旧异步结果失效、Controller 无 member 公共入口与不可恢复警告均有测试。
- 既有 Remote 契约迁移：69 个定向 case，通过；另入场资金设置新增真实红→绿，3 个 token／控制 case 通过。mock fetch／WebSocket 只验证客户端协议，不是多客户端 Server 端到端验收。
- 本人 selectors／owner／移动控制 gating：44 个定向 case，通过；协议 owner 接线追加 16 个定向 case，通过。规范 u64 字符串不转换为 Number。
- Remote reset 先有失败测试，再实现；保存命令 12 个定向 case 通过。后续 ArchiveStore 选择契约由存储作者继续维护，不用这些早期数字替代最终结果。
- 47 个启动／生命周期／交易命令定向 case 曾通过；追加 Remote 生命周期不覆盖全市场设置、同 generation 资金账户等待 baseline、同 host 跨代控制结果隔离后，33 个相关 case 与 3 个控制 case 再次通过。
- 所有普通验证均显式 Node case timeout 10000ms、进程外命令 deadline 10000ms，按 4／8 个 Node 测试进程并行；未运行完整回归、Cargo 或真实多客户端端到端。
- Web `tsc -b` 实际通过，最新命令约 8.7 秒；定向 oxlint 实际通过。未手工修改 generated。
- Auth 子模块已由未实施者独立复核，两轮有效发现均修复并再次短测；整批 UI 与 Server／Engine 的完整 diff 独立复核由 Root 统一安排，未完成之前不宣称总体完成。

本批不改变 A 股 Money 分、股份股、T+1、费用、价格时间优先等撮合规则；只依据 ADR-0030／0033 接入身份、成员与市场控制，未将 mock PASS 冒充真实多人撮合或持久化验收。

## 公开投影与完整账户范围后续接线

- Root 后续批准当前协议新增 `PublicTrade {seq,code,price,qty}` 和 `PrivateEventOmitted {seq}`；严格 parser、canonical/key、覆盖 seq、effects 与逐笔量价类型已接线，不伪造 AccountId(0)。CivilUpdate 的 refresh.intraday 同样可消费匿名量价且不重放历史 effects。
- Root 在真实 Rust MAX 账户链红测后批准全 AccountId 当前 JSON 统一规范 u64 十进制字符串；Web Snapshot／RuntimeDelta map key、Trade maker／taker、金融事件 account／entity key、Save 字段、NPC query 与 Inspector 都不经 Number。order ID／seq／tick 不改变。
- 新协议真实 Node 红 2／2 后实施，后续 PublicTrade／MAX／CivilUpdate 4／4 通过；相关两批旧协议与移动／UI短测试分别 41／41 和 80／80 通过。
- Save／NPC wire 作者真实红 3／3 后实施，最终代表性定向 101／101 通过；扩展 159 个 case 中 33 个陈旧 current JSON／公司 schema case 失败，已交 Root 正规重产，不手改 generated／历史 JSON，不将失败隐去。
- Root 后续新增 owner-scoped removed 契约：`PlayerOrderDelta.removed` 必须为 `{id:number,owner:AccountId字符串}` 数组。真实 Node 红 1／1 后严格对象 parser 和 apply 已接线，旧数字数组、非本人 owner、非规范 owner 明确拒绝，只删除匹配本人订单，不清空全部订单。相关四文件 16／16 通过，约 0.35 秒，8 测试进程，case／外部命令 10000ms。
- OwnerScopedRemovedOrder 当前源码 lint 通过；其余 PublicTrade／完整 AccountId 正规 binding 已到位后，最新 tsc 实际仅剩三项旧 generated removed:number[] 不匹配，等待 Root 正规 export 更新再复验，未手写生成类型。完整独立 diff 复核仍由 Root 统一组织。

## Root31 当前 binding 收口

- Root31 由真实 Engine ts-rs export 更新 `OwnerScopedRemovedOrder` 与 `PlayerOrderDelta` 后，实际 Web `tsc -b` 通过，进程外 10000ms deadline，耗时约 7.49 秒。
- Save／MAX／memberships／WASM／NPC／removed／PublicTrade／CivilUpdate／公司协调器／协议共 13 文件，75／75 通过，8 测试进程、case／进程树共享命令 deadline 10000ms，耗时约 0.61 秒。日志为 `.tmp/web-shared-market-final-short.log`。
- Remote 8 个定向测试文件复验 70／70 通过，8 测试进程、case／外部 10000ms，耗时约 1.32 秒；日志 `.tmp/web-shared-market-remote-final.log`。仍是客户端 mock fetch／socket 协议测试，不是多客户端真实 Server 验收。
- 公司协调器另有完整事件 seq matcher；新增公开变体真实红测暴露未知事件错误后，已补两种明确 variant，17／17 绿色。只推进 coverage／metadata，不将占位事件当成公司披露或金融通知。
- market_memberships 与 WASM stringMap 续批代表性 116／116 通过；合法空格／原型同名 subject 不丢失，AccountId MAX 无损，PlanId numeric Map 不改变。全批独立 reviewer 已启动，必须完成有效项修复与复核后才报告完成。
- 独立 reviewer 发现并确认 P2：Server logout 成功后，本地 IndexedDB 清理失败曾阻止断开 RemoteHost。不记住登录的 IndexedDB 不可用环境也会触发。新增 logout orchestration 真正红测后修复，2／2 绿色；撤销后仍保证尝试断开，本地清理错误在返回启动页后显式显示，双清理失败保留 AggregateError。复核者完整复读 helper／test／App 接线，并独立短测 9 文件 27／27 绿色，约 0.51 秒；最终 tsc／lint 再次实际通过，约 7.62 秒。
