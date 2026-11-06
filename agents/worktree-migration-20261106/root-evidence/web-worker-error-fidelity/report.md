# Worker 错误保真修复证据

- 独占 worktree：`.worktree/worker-error-fidelity`，基线 `5bbcab37`，仅修改 `wasm-worker.ts` 和 `wasm-worker-failure.test.ts`。未修改 ownership fixture、集成源码或 lock；未提交、push、运行 Rust/full。
- 开工已读 ROLE、AGENTS、principles、error-handling、testing、architecture、open-questions、Git 规则，以及 ADR-0010 / ADR-0027。修复只在 Web 错误传输边界，不改变 A 股账户、交易规则、价格、时间或单位，无需引入交易制度依据或新依赖。
- 旧实现 classifyWasmFailure 能读标准 Error 的 code/message，但 Object.entries 丢非枚举字段；真实 postFailure 回归抛 HostFailure.message/code 非空校验错误。新实现显式采用 classified.code/message，仅提取自有白名单 optional；原 parseHostFailure 严格校验不变。标准 Error.cause 同样保留。
- 新增四个 case：标准 Error code/message；非枚举 code 和原生 cause 加完整有效 optional；非法 recoverable/recoveryActions；optional getter 和 Proxy descriptor 访问失败。既有 revoked Proxy、code getter、不可序列化错误、真实 message listener 回归原断言保留。
- failure fixture 原有 WorkerTaskScheduler 真实 MessageChannel 导致所有 TAP 输出后仍不退出。仅本 fixture 捕获真实 channels，在 after 关闭两端；全局构造器与 self 在 finally 恢复，未伪造 scheduler 或强制退出。
- 初次 red.stdout 已得到预期失败，但 ports 泄漏令外部 supervisor 在执行预算 9 秒杀树：red.json 记录 timeout=true、wall=9.137s、剩余活动进程=[]；它不是自然退出红。
- 修复 fixture 后的有效红：red-clean.json，exit=1，wall=0.572s；17 case 中 3 fail（非枚举 message、code 与被 message 错误遮住的非法 optional），14 pass，自然退出，剩余活动进程=[]。
- 生产修复后绿：green.json，exit=0，wall=0.559s；17 case 全绿，自然退出，剩余活动进程=[]。红绿绑定相同测试 SHA（d293a264...bab10），生产 SHA 从 f1e27c06...9f5a8 到 a89f619c...5828a。
- 普通测试实际命令在各 json 中完整保留：Node 25.8.2、--test-timeout=10000、--test-concurrency=4，真实文件级隔离并发；外部 Python supervisor 总预算10000ms（9000ms执行，1000ms终止/清理），记录两个真实 child 的 CPU/thread 样本与进程组终态。未使用强制成功退出。
- lint.json：oxlint --threads=4 --deny-warnings，两改动文件 exit=0；git diff --check 通过。未运行完整 Web 测试、全量类型编译、Rust 或 full regression，此批只声明定向短测。
- 准备阶段两次命令失败也如实保留说明：首次误选不存在 wasm-failure.test.ts，supervisor在启动测试前 FileNotFoundError；一次误用 oxlint 包路径得到 MODULE_NOT_FOUND。已改用存在的 protocol-failure.test.ts 和真实 apps/web/node_modules/.bin/oxlint，不计失败命令为验证通过。
- 依赖通过独占树的 node_modules symlink 明确只读复用集成树同 lock 安装；路径及 lock SHA 见 freeze.json，无安装/改 lock。

独立 reviewer：/root/sol61_proc_exit_review。完整 diff 是 change.patch；冻结源码和依赖身份见 freeze.json；日志原件留在独占树，本目录同步副本供只读审查。完成声明须等待未实施者独立审查。
