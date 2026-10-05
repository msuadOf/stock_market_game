# 真实 Browser／WASM 工具脚本独立复核

## 范围与状态

- 复核者 review_full_web 未实施工具脚本；完整读取 real-browser-wasm.test.mjs 初版、作者修复后全文，以及旧探索失败日志。
- 对照读取 check-web-release-wasm.mjs、IndexedDbSaveRepository、validateDayEndArchive、WASM restore_json／prepare_public_baseline／snapshot／save、Engine Snapshot 玩家投影及外部 deadline supervisor。
- 本轮仅静态复核并执行 node --check，实际成功。未启动浏览器验收；当前尚无有效 fixture／current normal pkg 的本轮绿色证据。

## 两项 P2 及再次复核

1. 初版仅按 wasm-pkg 路径称 normal，没有核验实际编译制品，diagnostics 误放该目录也可能产生 normal 证据。作者修复后 before 第 26 行复用 verifyWebReleaseWasm 验证真实 WASM 无私有 NPC diagnostics 导出；Worker 第 64 行再次要求 host_capabilities().npcDecisionDiagnostics 为 false。WASM SHA 与 fixture SHA 均在前后核对并记录，修复有效。
2. 初版 after 顺序清理 browser／server，前者失败会跳过后者及 fixture 稳定性检查；newPage 在 try 之外，创建页面失败也会漏掉 context.close。作者修复后第 38–45 行 Promise.allSettled 独立尝试全部资源与双 SHA，显式聚合失败；第 51–52 行把 newPage 纳入 context finally 保护。修复有效。

当前全文再次复核未发现新的有效 P1／P2。仅本工作记录发生修改，不改实现或 fixture。

## 真实路径与证据边界

- 原始 fixture 直接 fetch／JSON.parse→当前 strict Save＋DayEnd 校验，未补字段、删 pending、改 snapshot 或注入 fallback；无 main-thread WASM restore 或 object Map 恢复兼容分支。
- 真正 module Worker 初始化当前 normal WASM 与 Rayon 2；restore_json→prepare_public_baseline→snapshot/save；save 输出再次经过当前 strict DayEnd→真实 IndexedDB.save/list/copy/select/load→restore_json roundtrip，资源 finally 释放 Worker／Blob URL／browser context。
- 两个 BrowserContext 与独立 DB 名隔离，外层 concurrency 2；两个同链重复分片已诚实改名“隔离分片一／二”，不能称两种不同恢复路径。
- 脚本设置普通 case timeout 10000ms，但正式命令仍必须由 Root 使用 run-with-deadline.mjs 10000 -- node --test --test-timeout=10000 执行；单独 node --test 不构成命令进程树 deadline 验收。browser 启动、WASM 初始化、并行执行及清理均计入共享外部 10 秒，不得放宽掩盖运行时间。
- 前后 SHA 可确认运行中的固定文件稳定性，但源码与 pkg 编译对应关系仍需 Root 的正规生成／冻结证据；脚本不自动重编译，也不根据路径猜测源码新鲜度。
- 实际断言覆盖 public Snapshot seq／tick／成员账号、restore→DB→restore Snapshot 等值、两槽及已结算日期；不声称全部 Save 私有状态逐字节等价，不声称生产 App／WorkerHost 生命周期、真实 tick 撮合、Remote 多人、Tauri 或 Server SQLite E2E 通过。
- .tmp/real-browser-wasm-game-candidate-rejected.log 的两项失败明确保留：object 恢复 i128 类型错误与 main-thread Atomics.wait 错误。它们不是当前修正版 Worker 路径绿色证据，不能消除或改名通过；pending 日终候选的真实 producer 红仍待 Root 修正并正式重产。

## A 股语义与最小范围

这是验证工具，不新增 A 股规则、资金、账户或存档兼容；直接复用生产 strict schema、真实候选接口与真实存储。两项修复仅加强制品身份与失败清理，不改变被测交易或经济事实。未跑绿时必须保持“静态复核通过、运行验收待完成”的区分。

## 正规日终 fixture 的新真实红结果复核

完整读取 `.tmp/checklist-wave4/real-browser-wasm-day-end-current.log` 与 `real-browser-wasm-validation.md`，并只读核对当前文件 SHA：fixture 为 `f81dc44c86912666e9c78fbe2af541c7c36448d60c33ce2b82b9686442db43fb`，normal WASM 为 `b319e641bacd2193be5173f6f05f92b731a1796854fc09095e8a634a1ef6aad4`，与记录一致。没有修改源码／fixture，没有自行重跑。

- 两个真实并行分片均在首次 Worker `restore_json(raw)` 失败：NPC 7 保存 attention probability 为 `0.03979332740595576`，重建 deterministic profile 为 `0.03979332740595565`。这是当前 Native→WASM 恢复一致性拒绝，日志本身不足以单独确认产生浮点差异的具体算术／编译根因，须 Root 跨平台根修。
- Node 输出 tests 3、pass 0、fail 3，是两项失败 child 加失败父测试，不是三个独立恢复场景；记录称“两分片均失败”准确。Node duration 6445ms 与记录的外部整命令约 6.58 秒不矛盾，外部 exit 1 确认失败。
- 日终 strict parser 已执行，restore 拒绝后尚未到 prepare_public_baseline／snapshot／WASM save，更未构造或调用 IndexedDbSaveRepository.save；不能称真实 IndexedDB 成功路径通过，也不能称下一日生产或全部宿主通过。
- 日志没有 after hook 的双 SHA／cleanup 失败；脚本的成功前后核对与当前只读 SHA 一致。此项仅确认制品稳定性，不把运行失败计为 PASS。
- 新红不同于已保留的旧 main-thread Atomics.wait／对象 serde i128 探索失败；当前 Worker 路径未用那两项旧错误作根因，更未手补存档绕过新 profile 校验。

复核结论：当前结果记录诚实，工具路径与失败阶段解释正确；运行门禁保持失败，待 Root 正规根修、重建并绑定新哈希后另行验收。

## probability 根修后的真实绿色结果复核

Root 授权本批唯一正式运行后，本复核者完整读取当前工具脚本、`.tmp/checklist-wave4/real-browser-wasm-probability-current.log`、`wasm-probability-normal-package-check.log` 和 `wasm-probability-normal-exports.log`，并只读执行 sha256sum 核对当前制品；没有自行启动测试或修改源码／fixture。

- fixture SHA：`5457be5b68eba83576a0a87f4f54b8123d119a252543b7e49721924977a5fcb1`。
- normal WASM SHA：`36210175fa7a3152dea84ff407f6be3f53813aef2cf2653b8c359105aef9a58f`；exports 记录 bytes 12197383、forbidden 为空，正式 package checker 输出 `release WASM verified: web_wasm_bg.wasm`，与当前只读 SHA 一致。
- 两个隔离 BrowserContext 分片真实通过，每个 Worker 初始化 Rayon 2；两条真实 evidence 均为 committed／selected／sameSnapshot true，tick／expectedTick 120，seq／expectedSeq 1473，accountIds／expectedAccountIds 都为 ["0"]，settledDate 2030-01-08，slots 2。
- Node tests 3、pass 3、fail 0：两项成功 child 加成功父测试，不冒充三个独立恢复 case。分片耗时约 5.72／5.68 秒，Node duration 6.76 秒；Root 登记共享外部整命令约 6.93 秒。case 和命令树 10000ms 硬门禁未放宽。
- 全文确认 evidence 只能在 restore_json→prepare_public_baseline→snapshot／WASM save→strict DayEnd candidate→真实 IndexedDB save／list／copy／select／load→第二次 Worker restore 的全部调用成功后输出；因此当前不再只是静态脚本或 mocked DB 的绿色，是真实 normal WASM／Browser IndexedDB 恢复后候选存储往返绿色。
- 现有 after 双 SHA 检查与独立资源清理没有失败，当前只读 hash 与运行 evidence 一致；制品对应正规 source 根修和运行冻结由 Root 负责，本复核不另作 math 根修实现审查。

此绿色仅关闭上述真实恢复后候选→IndexedDB→恢复 Snapshot 门禁。之前 profile 校验红及更早探索红保留原始证据；新绿色不覆盖新自然日自动 DayEnd 生产、生产 App／WorkerHost 生命周期、全量 Save 私有状态逐字节等价、全部宿主、Remote 多人 E2E、Server SQLite 或 Tauri。
