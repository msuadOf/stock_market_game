# 工作树关联 branch 批量核销

冻结基线为 `6fe53f65265e186b62dfcb1d510a993f91b07e11`。下表各 branch 的基线外非 merge 提交经 `git cherry` 均为补丁等价，不表示原提交是基线祖先。独立复核确认各工作树干净、未发现匹配路径的活动进程。执行前复核工作树状态、HEAD、补丁等价与进程 cwd；将 worktree 切为原提交 detached，删除 branch 引用，保留 worktree 及其提交。进程检查只能反映当时状态。

| 原 branch | 原 HEAD | worktree |
| --- | --- | --- |
| codex/branch-cleanup-record-20261101 | 5c82fc9f | stock_market_game-branch-cleanup |
| codex/regression-parallel-20261101 | 24fcb1ca | .worktree/regression-parallel-20261101 |
| codex/candle-fixture-split-r40 | 1a4869bb | /tmp/stock-candle-fixture-r40 |
| codex/convergence-plan-20261102 | 99ba145f | /tmp/stock-convergence-plan-20261102 |
| codex/current-handoff-20261101 | 9d0a85de | /tmp/stock-current-handoff |
| codex/facade-exit-order-r42 | d91ab905 | /tmp/stock-facade-exit-order-r42 |
| codex/branch-convergence-handoff-2026-11-01 | bc308174 | /tmp/stock-handoff-20261101 |
| codex/isolate-report-case-r39 | 097d8e43 | /tmp/stock-isolate-report-case-r39 |
| codex/performance-final-record | d8f1539b | /tmp/stock-performance-final-record |
| codex/r55-web-timeout | 3fc410a5 | /tmp/stock-r55-web-timeout |
| codex/r57-regression-fixture | 11e9031c | /tmp/stock-r57-regression-fixture |
| codex/regression-throughput-r37 | 9fba8046 | /tmp/stock-regression-throughput-r37 |
| codex/supervisor-schedule-r50 | 0c58fcd8 | /tmp/stock-supervisor-schedule-r50 |
| codex/web-regression-r38 | fb8158bb | /tmp/stock-web-regression-r38 |
| codex/runtime-convergence-20261101 | 04831a1e | /tmp/stock-runtime-convergence |

相对路径位于仓库根目录；`stock_market_game-branch-cleanup` 位于仓库同级目录。共删除 15 个已等价吸收的 branch。内容涉及测试调度、测试夹具、性能记录和交接记录，未修改 A 股交易语义；未运行产品测试。

第二批冻结基线同上，以下 6 条关联 worktree 均干净，先以原 tip 创建 `archive/20261102-<短 SHA>` tag，再将 worktree 切为同一提交的 detached HEAD 并删除 branch。切换前核对 HEAD、状态和进程 cwd；独立复核确认内容依据。保留 tag 以保存可能有用的原始历史。

| 原 branch | 原 HEAD | 依据 |
| --- | --- | --- |
| fix/plan-identity-runtime | b41c8fb2 | 前五项补丁等价，末项增删行与基线祖先 14f5c703 按文件忽略空白后相同。 |
| codex/wasm-runtime-override-20261029 | b57398a0 | 增删行与基线祖先 8a5ca157 按文件忽略空白后相同，初始化行为与测试保留。 |
| codex/clippy-idioms-runtime-20261030 | 2043d5fc | 与基线祖先 ce4b20ba 的改动一致，仅 tick 行缩进不同。 |
| codex/company-archive-record-20261101 | ad550ec5 | 公司归档记录补丁等价。 |
| codex/company-convergence-20261101 | b2f0d353 | 配股除权与简税结算两项补丁等价。 |
| feat/company-disclosures-candidate | 316ca881 | 基线保留更正候选推进、具体错误原因链及相关测试；提交本身不属于补丁等价。 |

前三项的对应提交 patch-id 不同，增删行对照不能单独证明全部上下文语义相同，因此保留原 tip tag 和 worktree。六条 worktree 均保持原提交；没有运行产品测试。

第三批同样使用冻结基线 `6fe53f65`。以下四条均保留原 tip tag `archive/20261102-<短 SHA>` 和原工作树 detached HEAD，删除 branch 前复核 HEAD、工作区及进程 cwd。独立审查确认如下内容依据：

| 原 branch | 原 HEAD | 依据 |
| --- | --- | --- |
| feat/b4c-share-transform-r3a | d239ab47 | 六项补丁等价，余项与基线祖先 46f7692c 按文件忽略空白的非空增删行相同。 |
| feat/b4d-information-prepare | 51ca08c8 | 三项补丁等价，余两项分别与基线祖先 1e54f18b、9d74e819 按文件忽略空白的非空增删行相同。 |
| fix/share-split-zero-issued | 2c331b5c | 与基线祖先 a568584f 按文件忽略空白的非空增删行相同。 |
| codex/regression36-inline-repair | c000d475 | 三组 session 回归夹具的断言保留于基线，两个提交并非补丁等价。 |

增删行对照并非 patch-id 相等或最终上下文的完整语义证明，因此保留原 tip tag。前两处相关 worktree 中，share-transform 的 ignored `bindings/` 及 share-split 的 ignored `.tmp/` 也随完整 worktree 保留；未清除 WIP 或运行产品测试。

第四批将 `integrate/source-a-latest-validation-2026-10-27` 的 `bd1f1dfb` 归档为 `archive/20261102-bd1f1dfb`，保留原工作树的同提交 detached HEAD 并删除 branch。独立对照确认公司精确股数、root 与 retail 正每股估值等新增断言在基线保留；基线调整了公开前史和财务 fixture，因此不称补丁等价。操作前工作树干净并检查进程 cwd；未运行产品测试。

第五批将 `feat/registry-physical-day-phase` 的 `23759efa` 转为 `archive/20261102-23759efa`，worktree 保持原提交的 detached HEAD。该分支唯一提交的新增与删除内容在基线祖先 `6578cff5` 中均有对应，五处旧 `corporate_actions` 访问已适配为 `company_state.actions()`；基线祖先含其他改动，不能称完整 diff 相同。独立复核了存档来源校验和测试适配，操作前确认工作树干净并检查进程 cwd。未运行产品测试。

第六批将 `feat/admin-funds-cost-p1` 的 `547d86df` 转为 `archive/20261102-547d86df`，并保留原工作树同提交 detached HEAD。独立核查确认其 11 个提交只涉及三个资金成本和回滚测试文件；这三个文件在候选 tip 与基线祖先 `18f38220` 的 blob 完全一致。该比较证明当时集成的文件一致，不证明之后没有演进。操作前确认工作树干净并检查进程 cwd；未运行产品测试。

第七批对未绑定 worktree 的两条公司来源 branch 保留原 tip tag 后删除引用：`feat/company-b4e-commands` `9c671946` → `archive/20261102-9c671946`；`feat/b4c-rights-offering-r3b` `429e9182` → `archive/20261102-429e9182`。这是历史保全，不代表其生产与测试内容均已吸收。前者缺原测试/fixture，须继续核对送转批准边界；后者保留回购 binding Web 存档验证与独审证据，须继续核销演进后的解析行为。相关 stash 原样保留，稳定 OID 分别为 `0d33313de6cccef6f0789983b02824c3050d125a`、`6614e27629bae13cc62ca98de5866c0fa6c54359` 和 `6e342ccfb3e428c15481087abc4cfbc7cd0b01ef`。未运行产品测试。

第八批将 `feat/wasm-w1-combined-build-monitor` `d192e0d6` 和 `feat/wasm-w1-spawn-ui` `c4ce50c3` 分别转为同短 SHA 的 `archive/20261102-...` tag，原工作树均保留同提交 detached HEAD。检查时工作树干净、未发现进程 cwd 使用；独立复核提示早期 supervisor、bootstrap 和 handshake 仍须逐行为核销，不能宣称全部已交付。基线已有主要宿主和 spawn UI 路径，monitor 特有的历史构建工具与红绿证据保留在 tag；还需核对该工具是否仍承担现行门禁。未运行产品测试。

第九批将 `feat/wasm-w2-primitives-2026-10-28` `a1069614` 转为 `archive/20261102-a1069614`，保留原工作树同提交 detached HEAD。代表性的九项等待机制 smoke 以 `9799e336` 集成到更新后的收敛线（不是本记录原冻结基线 `6fe53f65`），未声称原 23 项测试全部迁移。此 branch 尚含旧宿主、握手、测试及证据的其他差异，仍需逐项核销；归档保留完整原始历史。操作前工作树干净并检查进程 cwd；原 smoke 直接 rustc 运行 9/9 通过，Cargo 冷编译未在 9 秒内完成。

第十批将未绑定 worktree 的 `integrate/product-union-2026-10-28` `87576b7a` 转为 `archive/20261102-87576b7a`。独立逐提交核查其七项变动涉及的 122 路径：候选 tip 与基线祖先 `bdb0412c`、`11cb97fa` 相比 120 个 blob 一致，另两个测试文件仅追加测试及模块声明；候选自身已替换的旧测试不能称为基线遗漏。这证明历史集成点覆盖，不能推断此后所有文件未演进或验收已通过。全部 stash 保留；未运行产品测试。

第十一批将未绑定 worktree 的 `test/company-b4e-actor-recovery-2026-10-28` `94d1f0ad` 转为 `archive/20261102-94d1f0ad`。该提交本身不是基线祖先；独立对照基线祖先 `0b368b1a`，双宿主测试模块注册及原 assert 行均保留，server 有一行 receiver 流程调整，另增加溢出、顺序及错误边界断言。此为历史内容覆盖，不证明当前测试通过；全部 stash 保留，未运行产品测试。

第十二批将 `feat/company-b4e-correction-lifecycle-2026-10-28` `d444f715` 和 `feat/company-b4e-repurchase-route-2026-10-28` `bea4ec29` 分别转为对应短 SHA 的 `archive/20261102-...` tag，保留原 worktree 同提交 detached HEAD。独立核查历史集成祖先 `bdb0412c`：更正队列、恢复、类型及文档一致，宿主失败处理和原测试行为保留；三个 save fatal 断言改为匹配 `SessionError::Step(actual)` 并比较原 fatal。回购候选、共同批准路由和路由测试 blob 一致，其他共享测试随错误包装演进。此为历史集成点的静态内容核对，不代表当前测试通过；操作前工作树干净并检查进程 cwd，未运行产品测试。

第十三批将 `feat/company-b4e-repurchase-common-2026-10-28` `878ea155` 和 `feat/approval-c-common-93-2026-10-28` `cac6a9e4` 转为对应短 SHA 的 `archive/20261102-...` tag，保留原 worktree 同提交 detached HEAD。共同来源 `506d55bd` 是未完成 green 验证的候选；相关批准、行政镜像与自然日测试在基线有后续实现，但核心测试尚需逐断言审计。该操作只保全历史和未完成任务，不声称全功能吸收或验收通过；全部 stash 原样保留，操作前工作树干净并检查进程 cwd。本批未运行测试。

第十四批保全四条 Clippy 来源：`codex/clippy-cfg-args-leaf-rework-20261030` `b5ce43c3`、`codex/clippy-cfg-npc-tests-20261030` `292b5eaf`、`codex/clippy-cfg-settlement-rework-20261030` `b21296d1`、`codex/clippy-idioms-sessionextra-20261030` `9985c6dd`，均转为对应 `archive/20261102-<短 SHA>` tag 并保留原 worktree 同提交 detached HEAD。四项含未逐项核销的生产重构；其中参数叶来源的 `trading_day` 说明在现行文件未见，须确认是否恢复；NPC 旧类型迁移和 30 文件 session 清理仍需核对。Settlement 同源读写上下文在基线存在。记录明确原候选未完成独审或 Cargo 证据的限制；本次不宣称全部吸收或 green。操作前工作树干净并检查进程 cwd，未运行产品测试。

第十五批将 `feat/tax-admin-projection` `5e123180`、`fix/finish-120-groups2` `5ff2673c` 转为对应短 SHA 的 `archive/20261102-...` tag，保留原 worktree 同提交 detached HEAD。前者保留官方原件、合同及未执行草稿；现行规则已有受限 native 税务接线，旧草稿不能代表当前尚未接线或全面支持。后者保留原始红绿与验收记录，其历史窄门不能替代最终集成验收；四个 diagnostics 场景曾出现 Filled 缺链，须以现行专项证据判断是否关闭。独立复核确认保留上述限制；操作前工作树干净并检查进程 cwd，本次未运行产品测试。

对第十五批历史 Filled 缺链的后续静态核对：基线祖先 `81547a33`、`db666393` 已接入成交因果来源，现行真实成交及 retained 重试测试包含一笔 Execution 对应两腿 Filled 的断言；四个历史场景仍保留真实 NPC、三交易日与双腿数量断言。尚未找到这四例在后续基线通过的执行收据，故状态为修复已接入、四场景复验缺证；不能依据旧失败推断当前缺陷仍存在，也不能称四例已绿。

第十六批将 `feat/web-correction-input-parser` `21f5d390` 和 `feat/web-proposal-input-parser` `3d01278a` 转为对应短 SHA 的归档 tag，原 worktree 保留同提交 detached HEAD。普通 JSON 的字段、枚举与数值解析在基线有对应实现；不声称完全等价：旧版拒绝部分非常规原型、symbol、不可枚举键，correction 还拒绝 getter 和数组额外属性；基线对部分此类非 JSON 对象可能接受或执行 getter。proposal 旧版返回新对象、基线返回原引用，基线还额外拒绝 u32 负零。按本轮非阻断范围记录这些差异，原 tag 保存真实 serde fixture 与历史证据；correction 原先红 stdout 已被覆盖，不能称其历史先红可审计。操作前 worktree 干净并检查进程 cwd，本次未运行测试。

第十七批将 `feat/reintegration-b4c-dayend-chain` `97ee0b8d` 转为 `archive/20261102-97ee0b8d`，保留原 worktree、ignored `.tmp/` 和 `b4c-probe-temp` stash。旧 12 步链是过渡实现，后续已有受限 Admin 事务及公司行为，不宜直接移植旧链；本操作不声称旧实现或全部测试等价。历史 60k 名册差额的原规模逐笔来源及当前复现/关闭均未证实，g=2、g=16 事件差异与性能净机窗仍应按现行代码核销；旧 runner 不应直接作为现行 deadline 工具。独立复核确认此证据边界，未运行产品测试。

第十八批将 `codex/wasm-allocation-observer-20261029` `dd267ded`、`codex/wasm-production-stop-20261029` `809dbc4d`、`feat/wasm-restore-topology-2026-10-28` `629b034d`、`feat/wasm-w1-combined-cargo-inputs` `1522e935` 转为对应短 SHA 的归档 tag，四个 worktree 保留同提交 detached HEAD。基线已有 Memory 观察、shutdown 回收、完整存档恢复容量及构建输入的主要后继路径和测试；容量策略由旧 2 GiB 测量方案演进为 3 GiB 和显式上界。前置 shutdown、完整调用链及演进测试未逐断言核完，不能称全部等价或 green；原始浏览器和红绿证据保留在 tag。操作前 worktree 干净并检查进程 cwd，本次未运行测试。

第十九批将 `test/company-b4e-actor-e2-base-2026-10-28` `9ec434ed` 转为 `archive/20261102-9ec434ed`，保留原 worktree 同提交 detached HEAD。该来源含双宿主恢复、回购、行政镜像及个人信息候选与大量历史工作证据；后继集成点保留双宿主恢复原测试内容，但其他路径仍需逐项核对与运行，不能宣称整体 green。关联 E2 和 B4e stash 均保留；操作前 worktree 干净并检查进程 cwd，本次未运行产品测试。
