# WASM App 日终保存与请求占用独立复核

结论：`SOURCE_PASS_WASM_APP_SAVE_CAPTURE_AND_SCOPE`。最终完整 patch SHA-256 `40e67df0fd7a372d62d4dee9f8eae61094cbb966fe6d405621bfd8937fb672bf`；基线正式 dev8e `8e058acae3ffa498714dc54d810c00ee5c315278`。审查者未实施产品或作者测试，已核完整 tracked diff、两个新增 capture 文件及冻结 patch。实质准票先送父与唯一 writer，文书不阻净接。

## 必要性与领域语义

真实默认 20007 NPC App 实跑中，日终 save 请求至 saved 回包约 15.202 秒，超过原 WorkerRequestScope 通用 10 秒；同 engine 后排 orders、五个 company query 与 speedMetrics 也超过普通请求期限。修复只处理后台日终候选取得与同 Worker 请求发送顺序，不更改 A 股交易制度、自然日/交易日、撮合、T+1、Money 分字符串、Civil 语义或存档内容。

依据项目 ADR-0025 的成功自然日日结候选、ADR-0033 的事务持久化与失效隔离约定，候选取得不是 DAL 提交，成功 notice 仍必须在原写入完成后显示。没有用扩大普通测试上限替代产品契约；60 秒只是独立 save 请求的有界产品期限，全部定向自动化测试仍以外/case 各 10 秒执行。

## 完整实现差异

1. `worker-host.save` 两种请求形态均显式 60000ms；普通请求默认 10000ms、ACK/线程入口不变。
2. 现有 WorkerRequestScope 只在 active save 期间保留后续请求 FIFO。未发请求不装 listener、不启动 timeout，实际 postMessage 才开始原请求期限。save 成功或明确 operationError 后放行，下一 save 重新成为边界；普通并发行为保留。
3. save timeout 使 scope 进入显式 poison，拒绝旧队列与后续业务，晚到 saved 不放行；保留 beginClosing 的显式 shutdown 许可，不把未知仍忙 engine 冒认为可继续。close/beginClosing 取消已发与未发资源，清理 listener/timer，无迟发。
4. baseline/restore 换代调用精确 `cancelGeneration(oldGeneration,error)`。同代 baseline 不取消；真实 baseline callback 同步创建的新代查询保留。取消放在消费者判定之后，原 consumerReject 主错误优先，旧代资源仍在同一同步 message 内取消。公司写命令 typed failure 解码原语义保留。
5. App 使用同一个已 validateDayEndCandidate 的 captured promise 交给 DayEndPersistence。候选 resolve 且 host identity、真实 ProtocolCoordinator ready/generation 仍匹配后才调用现有 orders refresh；reject 零 refresh，原保存失败 notice 保留。未修改 Civil reducer 的 `playerOrdersReady:false`、未伪造空订单权威基线、未延后到 DAL 全部 I/O 完成才刷新。

范围保持必要最小：只增加现 scope 内的请求资源状态和三参数候选 helper，不增加 UI busy 框架、依赖或新交易规则。`git diff --check` 通过。

## finding 与修复

独审真实 W1 host lifecycle fixture 在最初仅改 save60 的候选下发现 pending save 遇新 baseline 不立即拒绝，导致悬等并被外 10 秒 supervisor 终止。原日志 `independent-short.log` 保留。最终按代取消后，同一独审 fixture 已通过，dispose 与 generation 两路均即时拒绝，late saved 不恢复。

独审必要回归中，既有 `worker-request-scope.test.ts` 首 case 用 save 代表通用并发请求，并先等待后发 civilDate 逆序完成；新 save 边界使该 fixture 永等。原 `independent-final-short.log` 保存 51 pass 与该 suite deadline 终止，未抹去失败。fresh fixer 仅将首请求/响应改为普通 speedMetrics，标题明确普通并发查询；连续 ID、wrong generation 忽略、逆序完成、pending/listener 回收、duplicate 响应及结果断言全部保留。单独重跑 scope 3/3 通过，test SHA `dbcbc06ba7b4b08543fb69a7e54c34324998c72ef24b1da2b3e1805aefb7acdb`。这项修复保持资源隔离测试目的，未删除或弱化断言；save FIFO/timeout 另有专用短测。

无未解决 finding。

## 验证

独立必要合并集已成功的 51 项：worker-host 42、原公司写错误 suite 5、候选 capture 3、真实 W1 host lifecycle 1。该批因旧 scope fixture 悬等而整体 exit 1，故不冒称整条命令通过。

fresh fixer 后，只重跑必要 scope suite：3/3 通过，170.701515ms，自然 exit 0，日志 `independent-scope-contract-short.log`；未重跑已过 51 项。因此累计 54 项定向用例在最终实现上通过，分属两次核验，不宣称单次 54 全绿。两次均 Node24、仓库进程树 supervisor deadline 10000ms、case timeout 10000ms、file concurrency 4。

覆盖 save 15.202 秒假时钟成功、60 秒拒绝、普通 10 秒、未发请求不计时、operationError/FIFO/下一 save、同步 postMessage 失败、poison 关闭、dispose/beginClosing、same/new generation、baseline/restore callback 新代 query、消费者拒绝主错误、typed 公司 failure，以及 candidate pending/resolve/reject/失效。作者另有 App startup wiring 8/8 真实短测，路径误写失败原件保留；本独审未重复该 suite。

源码 SHA 与最终 patch 在 `independent-checks.json`。未运行 Browser、Cargo、build 或 target；没有重读旧大 raw。SOURCE PASS 只准统一净接和后续产物验证，不证明新保存链的 Browser 性能、DAL 提交、ACK 或 App/T6 完整 actual 已通过。
