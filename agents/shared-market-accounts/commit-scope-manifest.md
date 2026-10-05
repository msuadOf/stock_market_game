# 多人市场／AccountWire 提交归属与依赖清单

本清单仅依据当前 HEAD 和工作区 diff 作静态归属；没有运行 Cargo、操作 index、创建提交或修改产品源码。
文件路径相对仓库根目录。以下“整份 diff”不是整仓 `git add` 授权；共享文件必须依据符号定位 hunk。

## 1. Engine 本批最小生产集合

| 文件 | 本批必需 hunk／符号 | 同文件其他任务内容不能误归本批 |
|---|---|---|
| `packages/engine/src/orderbook.rs` | `AccountId` 的 TS string／规范 u64 字符串 serde；`canonical_u64_decimal` 原严格实现从 Session 下移复用 | **`canonical_u128_decimal` 属于 Q08 累计成交额扩容**，与本批 u64 codec 同一 diff hunk，不能按完整 hunk 一股脑认作 AccountWire |
| `packages/engine/src/session/memberships.rs` | 整个新文件：主体、经济成员、Admission、加入幂等、只新成员资金、成员校验、主体到账户派生／查询、设置现金纯逻辑 | 不含市场控制授权；不得把控制权移回 SaveSlot |
| `packages/engine/src/session.rs` | `mod memberships`／exports／tests；PublicTrade／PrivateEventOmitted 和 seq 分支；四金融事件 account 的当前字符串属性；`SaveSlot.market_memberships`；state.memberships；constructor、save_projection、restore 新玩家账户构造、clone_for_shadow／commit_from 的成员真实转移；Session 旧 canonical_u64 codec替换为低层 import | exchange_calendar、retained_market_history、PersonalTradeConfirmation、report_correction／report_frequency、u128 turnover 是 Q13／Q08／Q23 共享改动 |
| `packages/engine/src/session/persistence.rs` | `validate_save_slot` 中账户集合由旧 NPC+单个Player／连续编号校验改为 `save.market_memberships.validate(save)`；pending_player owner 由固定0改为成员账户集合 | stock-local calendar／history、税 Owner、频率、更正队列 guard不属本批 |
| `packages/engine/src/session/hash.rs` | exhaustive state destructure中的 memberships；`hash.field(&self.state.memberships)` | retained history、personal confirmations、更正事实的 destructure／hash属于其他批次 |
| `packages/engine/src/session/shared_ingress.rs` | `SharedSessionIngress::register_player` 动态注册 Player 的完整方法 | CalendarPublication、publish_calendar、calendar checkpoint、verification gate 模块及旧 Q22 并发测试是 Q13／Q22；方法与这些新增内容同 hunk，应按符号拆分 |
| `packages/engine/src/session/snapshot.rs` | `snapshot_inner` 将可见账户从 id0改为全部 AccountKind::Player；NPC仍不进入普通运行投影 | 其他快照／金融资源计算不改 |
| `packages/engine/src/session/protocol/player_orders.rs` | `PlayerWorkingOrder.owner`；可信本地 player_working_orders delegate；新增 `account_working_orders(account)` 和真实 owner过滤 | 全份当前 diff属于本批 |
| `packages/engine/src/session/protocol/delta.rs` | `OwnerScopedRemovedOrder`、removed当前对象数组；金融账户校验不再只认0但保所有资金／持仓 guard；same_order比较 owner；全部 Player 工作单投影；从上一权威工作单 owner派生移除记录；删除旧 number数组 codec；严格测试模块 | 全份当前 diff属于本批 |
| `packages/engine/src/session/protocol.rs` | re-export `OwnerScopedRemovedOrder` | 单行 export diff属于本批 |
| `packages/engine/src/session/protocol/civil/session.rs` | 仅 `bind_market_creator`／`join_market`／`set_admission_cash` 三个 ProtocolSession wrapper | 同一插入区的 market history／trade history、更正 wrapper、publication transaction／calendar、存档职责是 Q08／Q13／Q23 |
| `packages/engine/src/session/protocol/commit.rs` | 新公开事件两种 variant在 timeseries匹配中明确不生成账户 effects | 全份当前 diff属于本批 |
| `packages/engine/src/session/pipeline/event_key.rs` | EntityTag.Account规范字符串；PublicTrade stock身份；PrivateEventOmitted session身份 | 全份当前 diff属于本批 |
| `packages/engine/src/session/pipeline/event_collection.rs` | 内部执行明确拒绝外壳投影事件；完整 seq assignment匹配新 variants | 全份当前 diff属于本批 |
| `packages/engine/src/diagnostics.rs` | `PublicProjectionInInternalDiagnostics` 及两 variant明确拒绝 | u128累计成交额、ExchangeClosed及相关量价统计归 Q08／Q13，不属于本批 |
| `packages/engine/src/verification_evidence.rs` | 新variant identity／name／entity匹配；内部 evidence拒绝匿名投影冒充原执行事实 | ExchangeClosed rejection分支属于 Q13；旧 sealed witness未修改 |

## 2. Server 本批集合与共同所有的 hunk

`apps/server/src/actor.rs` 为多人、SQLite、Q08／Q13／Q23共用的大文件，不能声称整份仅为多人。
本批精确符号归属：

- `MarketIngress` 三元组（generation／SharedSessionIngress／派生 subject→account map）；map不持久化、不作为独立授权真源。
- `EngineUpdate::for_account`／`for_member_account`、`project_viewer_events`，包括 TickBatch、CivilUpdate.events/facts 与 refresh.intraday递归匿名化及 owned removed过滤。
- `PublicBaselineSnapshot::from` 不再硬筛0；`PublicBaseline::for_account`；MemberRequest／MemberResponse／MarketContext。
- `SessionCommand::Member`；Restore中的 subject／generation guard与 scoped响应过滤。`archive_slot_id`、metadata命令属于 Native存档；不能遗漏后者而暂存整份现有 enum。
- SessionHandles 的 `member_request`、context／baseline／join／resolve／snapshot／enqueue／工作单／history／confirmations主体方法，以及 scoped speed／running／pause／cash／restore／reset／shutdown。
- SessionManager的 shared_market_gate／market_ids／new_shared_session、同步创建主体绑定；register_game处同代派生map和controllers读取／初始化与 Native作者共同实现，须与SQLite grant API一起提交。
- SessionActor的 controllers与独立控制检查、handle_member_request、同代替换 source／map、真实控制权限／generation执行 guard。
- `replace_timeline` 的 generation／源关闭／派生map切换属于多人生命周期；ArchiveWriter.renew_after、槽位选择和写代更新属于Native，但目前合成唯一真实切换路径，不应拆出半个版本。
- `run_protocol_batch`／prepare_civil_updates 的 publication transaction、civil rollback 属 Q13；日终自动SQLite写入属Native；Q23 typed error与更正 handler、Q08个人历史／均价命令不归本人独占。

真实认证生产接线（由身份作者实施）须与上述 Actor一起提交：

- `apps/server/src/identity.rs`、`apps/server/src/identity_routes.rs` 两个新文件。
- `apps/server/src/routes.rs` 的 authenticated subject／authorized_market／authorized_session／独立 controller检查，所有 owned/control REST，public list／join／context／reset和WS授权、viewer投影、注销缓存拒绝。
- `apps/server/src/lib.rs` module／router装配；`apps/server/Cargo.toml` 的身份依赖与 Native依赖。
- `apps/server/src/routes/archives.rs` 属Native作者的存档接口，依赖同一主体与独立控制检查，不能用legacy sessionToken替代。
- `apps/server/src/main.rs`／deployment及SQLite bootstrap 属Native部署作者，依赖新数据库／身份服务，但不应算作纯Engine会员逻辑。

## 3. 直接测试集合

| 文件 | 当前本批内容 |
|---|---|
| `packages/engine/src/session/memberships_tests.rs` | 整个新文件11case，包括真实 MAX账户执行／Snapshot／SaveSlot恢复 |
| `packages/engine/src/session/protocol/delta.rs` | multiplayer_validation_tests 两case（与生产同文件） |
| `packages/engine/src/session/pipeline/event_collection_tests.rs` | 仅 `internal_execution_rejects_outer_public_projection_events` 新case |
| `apps/server/tests/public_event_projection.rs` | 整个新文件6case（3 viewer、Civil历史、MAX金融事实、owner移除） |
| `apps/server/tests/shared_market_restart.rs` | 整个新文件，真实同SQLite重建Manager且经济档不授撤控制 |
| `apps/server/tests/actor.rs` | 新 `shared_market_members_receive_scoped_accounts_and_actor_enforces_control`；其余已有fixture变化请按实际Wire迁移归属 |
| `apps/server/src/actor/fatal_tests.rs` | 新 busyHTTP真实NPC gate、scoped subject-cache切换、控制／经济加载分离三case；现有fixture ingress三元组／controllers字段补齐属于本批，archive字段是Native配套，其他fatal／publication修复属Q13／Native |
| `apps/server/tests/shared_market_rest.rs`、`apps/server/tests/identities.rs` | 身份作者新增的真正公网身份／共享市场API短测 |
| `apps/server/src/routes/auth_tests.rs`、既有API／WS／pause测试 | 身份作者当前真实token／accountstring契约迁移；需其当前file manifest逐项确认，不能删除旧安全断言 |

## 4. 跨Web契约最小闭环（共同作者，不能仅提交Rust）

- 生成类型必须来自真实ts-rs：`AccountId.ts`、`EntityTag.ts`、`Event.ts`、`PlayerWorkingOrder.ts`、`PlayerOrderDelta.ts`、新 `OwnerScopedRemovedOrder.ts`、`OpaqueSubjectId.ts`、`AdmissionFunding.ts`、`MarketMembership.ts`、`MarketMembershipState.ts`；`SaveSlot.ts` 是多人／Q08／Q23共用必填契约，`SessionSetup.ts` 含Q23必填频率，不属于单独的AccountWire生成物。
- 当前协议：`apps/web/src/host/protocol/{parse,canonical,normalize,validate,effects,types,guards,wire-values,runtime-delta}.ts`；`host/player-working-orders.ts`、`host/serde-normalize.ts`；对应 source contract／runtime delta／public events／owner removals／serde AccountId短测。
- 成员存档：新 `save/schema/market-memberships.ts`、`save/market-memberships-schema.test.ts`；共享 `save/schema/root.ts` 的必填market_memberships及账户集合校验；`primitives.ts`／`save-snapshot.ts`／`orders.ts`／`personal/common.ts`／`runtime-state.ts` 的全AccountId规范字符串边界；`save/account-id-contract.test.ts`。
- 当前Auth与本人UI：新 `auth/`、`host/remote-auth.ts`／`remote-market-context.ts`、remote-host／EngineHost能力、startup/login/logout、`store/remote-membership.ts`、store owner selector及portfolio／下单／工作单消费者；由UI作者独立清单归属。不能保留“远程仍读accounts[0]”然后宣称多人闭环。
- 全AccountId当前fixture迁移横跨旧Rust/Web tests；只迁字段／key契约，不按文件把Q08量价／Q13日历／Q23税断言一起称为Wire。未提交的真实 SaveSlot producer／current-schema-save／current-company-slice 是完整新职责结构，须与所有必填字段同批提交，不允许删字段、默认补旧档或手补sealed。

## 5. 对 HEAD 的真实依赖阻断

1. **Q08**：HEAD没有 PersonalTradeConfirmation。新多人Actor的confirmations／个人history请求、新成员MAX成交恢复测试，依赖 Session中的真实 confirmations状态、receipt结算生产／SavedRuntimeState捕获恢复、查询类型及相关engine exports；新增 retained_market_history／active_minute_history 是当前Save必填。因此仅暂存多人Actor或新SaveSlot生成类型会产生未定义类型／方法或不完整存档。
2. **Q13／Q22**：完整Actor生产循环现调用 `with_publication_transaction`、calendar publication checkpoint／rollback／stock calendar等；这些不在HEAD。SharedSessionIngress的新register_player可从独立纯方法逻辑拆出，但完整当前Actor／ProtocolSession文件不能缺其真实calendar生命周期配套。不能为独立编译删publication guard或保旧副路。
3. **Q23**：当前 SessionSetup.report_frequency、report_correction_operations／epoch、共享税Owner及更正结构不在HEAD。当前fixture、ProtocolSession exhaustive state clone／hash／SaveSlot／Web root parser已使用这些必填字段，直接暂存整份session.rs／SaveSlot.ts却缺Q23 modules会编译失败或违背当前契约。
4. **SQLite／IndexedDB**：当前Server控制授权注册／身份服务／register_game／resume_session依赖 `native-store` workspace、Cargo依赖／lock、SQLite身份与独立grant API；Web启动与日终候选使用IndexedDB repository。只提交Actor成员helpers没有真实生产身份、授权或存档入口。
5. **Q08 u128扩容**：orderbook.rs同hunk里的canonical_u128_decimal被DailyTradeStats／retained history／均价／diagnostics调用。AccountWire u64 codec可静态按符号区分，完整现文件若整份提交必须携带这些调用方及当前生成类型，不能删u128量价数据假装AccountId基础独立。
6. **默认100亿元**：已静态核对当前HEAD的 GameConfig defaults与注释已是1_000_000_000_000分。本次不要再次把已提交默认值当作待提交生产 hunk；当前成员测试仍需要该HEAD基础。

### 安全提交建议

现有完成产物覆盖的是上述共同当前源码，不是删去依赖后的临时单任务子集。优先以“共享市场／日终数据库／当前Wire与完整Save职责”为cohesive基础提交，包含必要Q08／Q13／Q23接线和current生成物／fixture；后续 ShareRegistry等消费者再以该基础为父提交。
若根坚持拆更细提交，必须按此清单的符号逐hunk暂存并证明每个中间提交真实可编译／当前parser一致；本清单不授权通过删除强guard、旧副路或格式兼容实现中间绿。没有建议把Dividend研究／未完成新产品、旧sealed见证、工具格式噪声混入本基础。

## 6. 本主题文档／复核记录

本作者记录：`agents/shared-market-accounts/engine-memberships.md`、`public-event-projection.md`、本清单。
共同作者与复核：`account-id-web-wire.md`、`server-identities.md`、`server-test-contracts.md`、`remote-ui-wiring.md`、`control-separation-review.md`、`default-admission-cash-review.md`、`market-grant-lifecycle-review.md`、`server-final-review.md`、`remote-ui-final-review.md`、`web-batch-independent-review.md`、`real-browser-wasm-validation.md`、`real-browser-wasm-independent-review.md`；浏览器真实验证脚本为同目录 `real-browser-wasm.test.mjs`。
正式 docs 的architecture／ADR0033／trading-rules／总账是共同结论；ADR0031当前diff为Q08累计u128定义而非纯AccountWire，须由根按实际已决内容归组。
