# hosts server/WASM 实施状态

2026-10-03。本 worker 未执行 Cargo、完整回归、E2E、性能测试，也未执行任何 Git 写操作。已读 AGENTS、principles、testing、architecture、open-questions 与 ADR-0010/0017/0027；待父 agent 统一编译、代表性短测及独立完整 diff 复核。状态为“已实施待验证”，不是已验收完成。

## 动作状态

| 动作 | 状态 | 文件 | owner / 方法 | caller 与测试 |
| --- | --- | --- | --- | --- |
| hosts-03-A01 | 已实施待验证 | `apps/web-wasm/src/lib.rs`、`apps/web-wasm/src/protocol_tests.rs` | `SessionRegistry` 组合私有 `sessions`；`register/create/restore/with_session/step_update/remove`；thread-local RefCell 与 NEXT AtomicU32 保留 | `create_session`、`restore`、`restore_json`、`step_update`、全部有句柄的 WASM 导出经 Registry；现有两条直接 insert 的协议测试改用 create；新增独立句柄、构造/恢复失败不注册、未知 remove/access/step、恢复后 CivilUpdate barrier 测试 |
| hosts-N02 | 已实施待验证 | `apps/server/src/web_ui.rs` | `WebState` 组合 `StaticAssetRoot`；`validate_package/accepts_resource_path/resolve_resource`；`ResourceError` 保留 missing 与 I/O 区分 | `static_router`、`serve_file` 真实 caller 已迁；`deployment_router` 继续间接使用；现有 `deployment_routes` 启动、MIME、隔离头、权限、traversal/symlink 断言保留 |
| hosts-N06 | 已实施待验证 | `apps/server/tests/deployment_cli.rs` | 测试私有 `RejectedCommandFixture` 持有 `Option<Child>` 和 deadline；`spawn/wait_rejected/kill_and_wait` | 五类 CLI 拒绝测试仍经薄 `rejected` bridge；真实 CARGO_BIN_EXE_server、500ms deadline、5ms poll、原有断言保持；不新增 Drop 清理行为 |
| hosts-R2-N14 | 已实施待验证 | `apps/server/src/routes.rs` | 私有 `WsPublisherConnection` 持有 delivery/baseline cursor/generation/ClientFrameBuffer/barrier/failure latch；`ingest/buffer_error/failure_delivered/require_resync/prepare_resync/install_baseline_cursor/baseline_delivered/request_frame/take_push_frame/take_flush_frame/accept_after_flush`；纯 `PublisherAction/FrameRequest` | 每次 `run_ws` 创建 owner，真实 select loop 委托纯决策并继续 socket I/O；订阅改用 actor worker 新增 `handles.subscribe_events()`；新增五项纯转换短测；现有 baseline wire test 保留；恢复 WS 测试补充 barrier 期间 SubmitIntent→CommandQueued 断言 |
| hosts-R2-N15 | 已实施待验证 | `apps/server/tests/api_contract.rs` | 测试私有 `ApiTestSession` 拥有同一 manager/router/id/token/handles；`new_settled/shutdown`；closed_day_save 现在显式返回 Result 供构造失败诊断 | 四个旧 new_settled_session caller 与 pending-player-queue 测试均迁移；不再 tuple 解包后重复 lookup；五个测试正常结束显式 shutdown；原 generation、cash、pending queue、坏 JSON/坏存档断言保留；新增 setup 创建失败与未完成市场日日终 fixture 失败的诊断测试 |
| hosts-R2-N16 | 已实施待验证 | `apps/server/tests/ws.rs` | 测试私有 `ServerFixture` 拥有 base_url/manager/listener JoinHandle/session 清单；`start/session/session_with_setup/ws_url/shutdown/run` | 六个 WS 测试与手工性能 probe 的 measure 全部迁移；run 的 test task panic 时外层先显式 shutdown 再恢复 panic；新增 listener/session 清理与真实 run panic 清理短测 |

## 保持的协议与 A 股边界

- 引擎仍是 ProtocolSession、A 股账户/单位/委托及交易规则的权威；本批没有修改撮合、T+1、费用、集合竞价或存档格式，不引入新的交易制度，依据 ADR-0010/0017 与现有 trading-rules。
- Registry 保留全部 wasm_bindgen 导出、输入解码及 JsValue 错误形状。serde-wasm-bindgen/JSON 仍各自解码后统一 restore。NEXT 耗尽行为保持，未顺带修复 wrap-around。step_frame→tick_batch 仅保持顺序，不宣称组合原子性。
- 静态服务顺序保持“namespace→path 字面校验→GET/HEAD→canonicalize→containment/hidden components→metadata→ServeFile→404 再开文件诊断”，请求时不缓存 canonical 文件，不改变现有 symlink/TOCTOU 行为。
- WS `ingest` 严格 failure→generation→awaiting_resync→covered→push；新 failure 仅在成功发送后 latch，duplicate failure 保留 barrier；generation 变化先 clear/barrier，再显式 ResyncRequired。
- MetadataTransition 一帧 flush 与 Push capacity 全 backlog flush 都返回未接受的 incoming update；真实 run_ws 发送成功后才 accept_after_flush。Pull 满载仍 clear/barrier/resync，不压缩或静默覆盖。
- Resync 保留 subscribe→clear→public_baseline 顺序；baseline 请求失败仅 clear 已发生，cursor/generation/latch/barrier 保留旧值；成功 snapshot 先写 cursor，成功发送 baseline 后才更新 failure/barrier。发帧失败仍关闭连接。SubmitIntent 不受 resync barrier 阻止。
- WS fixture 是测试资源 owner，无产品 caller。关闭顺序显式为 listener abort+await，再 manager.remove+actor.shutdown；ActorGone 只在已停止 actor 清理中显式接受。panic 收束不依赖异步 Drop。

## 已做静态验证与待办

- 已执行目标文件 `rustfmt --edition 2021 --config skip_children=true`，解析成功；仅格式化自身拥有文件。
- 已执行目标文件 `git diff --check`，无 whitespace 错误。
- 新增 Registry、WS connection 及 fixture 测试先写后实现；因父 agent 明确要求统一 Cargo，本 worker 未运行 Red 或 Green，不声称有运行中的失败/通过证据。
- 必须由父 agent 统一编译并修复编译/短测问题；必须独立 subagent 复核完整 diff 后才能报告完成。
- `buffer_error` 的纯测试直接输入类型化错误，固定 MetadataTransition/满载的连接决策及发送前不接受输入；不是 socket send-failure 或真实 65,536 units 容量压力验收。
- API fixture 的无注册 session 分支有上下文错误，但正常构造器保证 manager/router 配对，未为了模拟该不可达错误增加注入架构。恢复失败返回显式错误并关闭 fixture，新增未完成市场日测试覆盖 domain fixture 失败；未 mock actor 来制造 restore-only failure。
- WS `run` 用 Tokio test task 收束断言 panic；取消整个外层 test future（例如进程外 deadline kill）仍由 runner 管理，不宣称 fixture 提供跨进程清理或抗任意取消保证。
- 新增依赖：无。

## 建议统一验证的精确短测 filter

先多核定向编译，不把编译计入普通测试的 10 秒门禁；随后对预构建 test binary 使用进程外 10000ms deadline、显式 `--test-threads` 与 Rayon budget。每个下面的 filter 单独或按资源预算并发执行，勿运行整个 WS E2E 或 ignored probe。

- `web-wasm` lib：`protocol_tests::registry_`（四项 Registry 协议/生命周期测试）；`error_layout_tests::step_update_error_keeps_fatal_payload_indirect`。
- `server` lib：`routes::publisher_connection_tests::`（五项纯转换）；`routes::baseline_failure_tests::latched_failure_is_sent_after_an_unchanged_baseline`。
- `server --test deployment_cli`：五个既有 CLI 拒绝测试可作为一个小 binary 批次（每项 child 上限500ms）；不额外跑其他 server 二进制。
- `server --test api_contract`：`settled_api_fixture_reports_`（两项新增失败诊断）；`save_generation_rejects_same_date_seq_edited_assets_after_restore`；`load_rejects_corrupt_body_before_actor_replacement`。其余既有迁移用例待父 agent判断短测预算，尤其不把 history_len=10_001 当必跑短 fixture。
- `server --test ws`：`server_fixture_`（两项新增短生命周期测试，真实临时 listener，但无需 WS 长交互）；六个原有 WS E2E 与 ignored `publisher_modes_report_actual_speed` 仅迁移，不在本轮短测执行范围。
- `server --features web-ui --test deployment_routes`：`startup_requires_valid_root_index_and_nonempty_assets`、`head_has_headers_without_body_and_static_post_is_rejected`、`traversal_and_symlinks_cannot_expose_host_files`；其他静态路由用例仍保留。

## 独立复核发现修复（2026-10-03）

- review_server 发现新增 `flush_actions_leave_incoming_update_unaccepted_until_send_succeeds` 未断言 `all`，一帧 fixture 无法区分 MetadataTransition 与 Push capacity 的 flush 策略。
- 已仅修改该新增用例：按 `(error, expected_all)` 明确 MetadataTransition=false、Push capacity=true，在取帧前 assert_eq 并删除 let_=all。无产品代码或原有断言调整。
- 定向 rustfmt 和 git diff --check 通过；未运行 Cargo 或测试。已请求原 reviewer 增量复审，结论待其报告。
- 六项结构化记录见同目录 `status.json`，其 review 指向 `../review-server.md`；Rust root 编译/短测仍待验。

## Root 最终代表性验证（2026-10-03）

本组最终状态以 status.json 的 final_validation 为准。root build07 与 all-targets check08 均通过；root 选定精确 Rust case 的逐条结果已按 source 映射，WS 两个 bind sandbox EPERM 保留初始失败记录并由沙箱外原断言 2/2 通过核销。Writer 5/5 与桌面 CLI 5/5 通过。未选中的旧测试、完整 suite、API doctest（actors）及真实 matrix/E2E/性能未据此声称执行。历史“worker 未运行/待 root”段落保留为过程证据，当前没有未关闭实施或独立复核发现。
