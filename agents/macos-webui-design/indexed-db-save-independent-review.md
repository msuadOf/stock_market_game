# IndexedDB 快速槽独立复核

2026-10-05。读取完整当前 tracked diff 及新增 IndexedDB adapter/测试，检查 App 接线、CompressedSaveRepository、DayEndPersistence 与浏览器验收变化。未参与实现，未修改产品代码；主 agent 运行 E2E，未启动 CPU 验证。

## 初审结论

大 A 语义没有新增制度假设：存档仍经过原有 strict schema 与 gzip codec，不改变日终候选、tick、资金或持仓单位。仅替换快速槽存储容量层，符合 engine/Web 分层。旧 LocalStorage 档只在 IndexedDB 确实缺槽时读取，损坏或打开失败不回退掩盖问题；不在启动复制写盘，范围克制。

写入在压缩后、开库后、put success 时检查 generation；失效事务 abort 保留原槽。成功 Promise 等待 transaction.oncomplete，配合 DayEndPersistence 已有等待 writer 的替换契约，未发现新旧局穿透。commit() 是请求提交而非已经持久化完成，当前返回值正确等待最终完成。现有 quota/commit 失败、旧档只读、失效时点短测覆盖主要路径，新增浏览器真实 IndexedDB 保存/刷新方向正确。

## 需修复/补齐

1. P2：数据库意外关闭后 connection 缓存不会失效。当前只处理 onversionchange；若浏览器异常关闭数据库，缓存的 fulfilled Promise 仍返回已关闭 IDBDatabase，后续每次 transaction 都会 InvalidStateError，用户无法通过正常重试恢复。建议监听 database.onclose 并清理该连接缓存，补异常关闭后下一操作重新开库成功测试。
2. P2：正式架构描述仍使用旧持久层。docs/architecture.md 的 Stage 1 表格与可替换实现段落仍写 LocalStorage；需同步为 IndexedDB 快速槽及旧 LocalStorage 同格式只读兼容，避免产品代码与跨层契约漂移。

开库失败/阻塞/升级错误会经 request 错误显式拒绝，transaction 同步异常也由 Promise executor 拒绝。建议针对这些生命周期路径补适量 adapter 测试；不需要扩大到新增依赖或存档格式迁移。暂未发现新的交易语义或正常路径原子性缺陷。

本批尚未通过最终门禁：上述有效发现待修复复核，实际默认 20007 NPC 保存/刷新仍在验收，不能先称默认局存储问题已解决。

## 再次完整复核：代码门禁通过

2026-10-05。两个初审 P2 已关闭：database.onclose 清除匹配的缓存 connection，异常关闭后重开测试保留旧档并验证后续新写入；onversionchange 同样比较 connection 身份，避免旧连接事件清掉新连接。正式 architecture 已同步 IndexedDB 快速槽、旧 LocalStorage 同格式只读和错误不可回退契约。

最终 App 延迟到旧槽读取时才访问 window.localStorage，因此 IndexedDB 已有档时不受旧存储 getter 限制影响。transaction.onerror 优先使用 request.error，保留请求真实原因；其余 reject/abort 不吞错。put 回调的 isCurrent/commit/abort 已在 try/catch 内，最终成功仍等待 transaction.oncomplete。新常量模块仅共用存储键，没有新增依赖或存档格式。再次完整 diff 未发现新的有效 finding。

Reviewer 独立执行 indexed-db-save-repository、save-repository、day-end-persistence、session-replacement 四个测试文件：23/23 通过，238ms；显式 test-concurrency=3、case timeout=10000ms，外部 run-with-deadline 10000ms 进程树保护。没有构建或运行 E2E。git diff --check 通过。主 agent 报告最终真实 WASM trading-workflows 为 6/6 通过、23.5 秒。

三项结论：本批不改变 A 股规则、日终边界或元/股单位，复用 strict schema 和 gzip 保证存储适配层语义一致；替换快速槽容量层及保留只读旧槽是必要的最小范围；generation、错误保护、关闭恢复及新旧档边界已有针对性覆盖，没有剩余独立审查阻断。代码门禁通过。

验收边界：主 agent 已报告默认 20007 NPC 实际日终写入成功并刷新启动正常，但仍继续核实读取后的实际局状态。不能把启动正常本身等同精确恢复完成；该用户路径最终证据应由主 agent 完成并写入工作记录。本结论不表示整个终端任务完成。
