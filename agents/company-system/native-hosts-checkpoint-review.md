# Native Hosts Checkpoint 独立复核

## 范围与方法

- Reviewer：未参与实现的 checkpoint reviewer。对照 HEAD 与当前 worktree，审阅本记录末尾列出的 Native store、Server、Desktop Tauri、WASM host 和 Rust manifests 的完整 tracked diff，并通读新增 source/tests；另对照 ADR-0030、ADR-0033、工程原则、身份作者/Native storage 记录及相邻授权复核。
- 未修改 production、测试、Cargo manifest、Cargo.lock 或 index；未运行 Cargo。Root 提供的五个 library fresh compile 通过信息仅记作编译证据，不扩展为三平台运行验收。
- 交易语义核对以 A 股账户现金/股份与既有撮合代码未被这些宿主改动改变为限；本次不涉及交易制度规则，无需新增交易所依据。

## 结论

- **Must-fix：当前审查范围未发现。** 现有路径遵守共享市场账号认证与本人经济账户分离；market-control grant 独立于经济存档；REST/WS 从认证 subject 派生账户，不接受请求指定账户；公开事件投影隐藏其他参与者的账户、委托和私有拒绝/结算细节，保留匿名成交量价；AccountId 在 host 投影和相关用例中保持严格规范 `u64` 十进制字符串。
- 日终经济持久化只接受由成功 `CivilUpdate` key capture 且经 `ProtocolSession::restore` 验证的 `DayEndCandidate`。SQLite 初始化只接受精确当前结构，无 schema 迁移；journal mode 为 DELETE，未启用 WAL。日内内存状态不触发自动持久化，失败写入保留旧候选并报告失败。
- 启动档读取、明确加载、档案元数据操作均分开；无已选档时不会隐式回退旧档，已选档缺失/失效显式报错。选择其他存档恢复经济状态时，控制授权留在独立 `market_control_grants`，缺席主体需本人确认后才按当前 admission cash 加入。
- Simple public availability 在 Server、Desktop 和 WASM 均走 Engine 查询；Engine query DTO 拒绝未知字段，REST 额外校验 path/body `company_id` 一致。Host 直接返回查询结果，没有发现转成 simulation 或旧兼容路径的 fallback。
- 完整 Native runtime 仍有限制：Linux 源码/编译与锁边界短测不替代 Windows/macOS writer-lock runtime；不将此复核或 fresh compile 称作完整跨平台验收。签名不在本批交付范围，现状未实现。

## 复核点

1. 身份：密码仅以独立 salt 的 Argon2id PHC hash 保存；Bearer token 由系统随机源生成 256-bit，SQLite 仅保存 SHA-256 digest；匿名主体同样独立持有 token。logout 删除当前 credential，不删除主体、账户或市场控制授权。身份 extractor 失败走固定通用响应，不回显密码解析错误。
2. 授权：REST intent、snapshot、working orders、交易历史由 subject->member 映射解析账户；WS baseline 与事件在每个连接主体范围内投影，并在缓存帧序列化/发送前重新认证。失去 credential 后连接停止发送后续私有数据。控制操作独立检查 controller，允许缺席经济成员控制但不授予其资产。
3. 多人可见性：Trade 改为不带 maker/taker 的 PublicTrade；其他成员金融事件改为带 sequence 的 PrivateEventOmitted；相应 EventFact/key/canonical payload 同步投影。runtime snapshot/delta 账户和订单按 owner 过滤；本人视图不会因为采用 `accounts[0]` 泄露或误选他人账户。
4. 金额/身份 wire：账户标识维持完整 `u64`，相关 JSON DTO、Event/EntityTag 路径要求 canonical decimal strings；金额依 Money 分字符串契约。该复核未发现转成 JS number 或 numeric JSON 的兼容分支。
5. 日终仓库：NativeDatabase 使用物理文件身份上的 OS writer lock；`DayEndCandidate` 无外部构造/反序列化入口，保存前深校验。结构差异或旧 `user_version` 拒绝而非迁移；生产 journal mode 不允许 WAL。元数据 writer 受活动 generation/owner guard 约束，Desktop IPC 经 actor 入口执行。
6. 新局/查询：Server 默认只加载明确选择的槽；`--new-market` 与 `--archive-slot` 互斥。新市场共享性在 manager gate 内检查，已有局不会为新主体另建市场；重复加入返回既有 membership，不重复拨款。Simple report availability 按显式 CompanyId/period/kind/scope 查询并校验。

## 已知未覆盖

- 没有在本轮运行 Cargo、Rust 测试或真实多客户端端到端；以 Root 指定的五个 library fresh compile 通过作为编译证据，其真实性和范围由 Root 的构建记录承担。
- Windows/macOS runtime、签名及完整生产发布均未验收；Linux writer-lock 边界测试不是这些验收的替代品。
- 现存作者复核记录指出跨 Host 真实多客户端旅程尚未验收；本记录不把单测/静态投影核对升级成 E2E 通过。

## 完整文件清单

以下清单是 `git diff --name-only HEAD -- packages/native-store apps/server apps/desktop/src-tauri apps/web-wasm Cargo.toml Cargo.lock` 与相同路径下 untracked source 的并集；审查的是这些路径相对 HEAD 的完整变更范围，Rust manifests 也在内。

### Tracked changed files

- `Cargo.lock`、`Cargo.toml`
- `apps/desktop/src-tauri/Cargo.toml`
- `apps/desktop/src-tauri/src/actor.rs`
- `apps/desktop/src-tauri/src/actor/failure.rs`
- `apps/desktop/src-tauri/src/actor/fatal_tests.rs`
- `apps/desktop/src-tauri/src/actor/protocol_tests.rs`
- `apps/desktop/src-tauri/src/lib.rs`
- `apps/desktop/src-tauri/src/lib_tests.rs`
- `apps/server/Cargo.toml`
- `apps/server/src/actor.rs`
- `apps/server/src/actor/fatal_tests.rs`
- `apps/server/src/deployment.rs`
- `apps/server/src/lib.rs`
- `apps/server/src/main.rs`
- `apps/server/src/routes.rs`
- `apps/server/src/routes/auth_tests.rs`
- `apps/server/tests/actor.rs`
- `apps/server/tests/api_contract.rs`
- `apps/server/tests/deployment_options.rs`
- `apps/server/tests/deployment_routes.rs`
- `apps/server/tests/pause_security.rs`
- `apps/server/tests/ws.rs`
- `apps/web-wasm/Cargo.toml`
- `apps/web-wasm/src/lib.rs`
- `apps/web-wasm/src/protocol_tests.rs`

### Untracked source/test files

- `apps/desktop/src-tauri/src/actor/archive_tests.rs`
- `apps/server/src/identity.rs`
- `apps/server/src/identity_routes.rs`
- `apps/server/src/routes/archives.rs`
- `apps/server/tests/identities.rs`
- `apps/server/tests/identity_fixture.rs`
- `apps/server/tests/native_archives.rs`
- `apps/server/tests/public_event_projection.rs`
- `apps/server/tests/report_corrections.rs`
- `apps/server/tests/shared_market_rest.rs`
- `apps/server/tests/shared_market_restart.rs`
- `packages/native-store/Cargo.toml`
- `packages/native-store/src/identities.rs`
- `packages/native-store/src/lib.rs`
- `packages/native-store/src/writer_lock.rs`
- `packages/native-store/tests/archives.rs`
