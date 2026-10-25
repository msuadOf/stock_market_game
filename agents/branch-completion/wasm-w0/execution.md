# W0 救回实施台账

- 基线：`f5820291`；旧增量：`1554b89a..0ac6bcc6`。独占分支 `feat/wasm-w0-recovery`。
- 46 文件清单见 `old-increment-files.txt`；排除全部旧 agents 正文与 asm comparator。现行 main 中 #117/#121 仅用 patch 叠加 clock/thread_facade 调用点。
- 设计依据：仓库根 `.tmp/wasm-thread-abstraction-design-input.md` 与 `.tmp/wasm-gate-r19-review-stdout.txt`；本次仅恢复 W0 基座，wasm runtime 保持 false，生产 Worker 接线留 W1。
- 红证：先移入旧测试，scripts 在缺少 `verifySharedMemoryArtifact`、combo 入口、nightly 参数及 CI 门时失败；5 个 Web 测试因目标模块不存在失败。见 `red-scripts.log` / `red-web.log`。Rust 门面探针因 clock.rs 缺失编译失败，见 `red-rust.log`。这些为移植必要性红证，不冒充旧历史提交的 TDD。
- 验证计划：Node 受外置10000ms deadline及case10000ms约束，显式8文件并发；Rust 先用 rustc 独立测试纯门面/slot模块，多线程执行、外置10000ms；实际engine编译与wasm产物构建按不可替代长构建分类，独占根 `.tmp/wasm-w0-recovery/target`，jobs=16，共享300000ms外置supervisor。禁止完整all回归。
- 内存裁定待落实现：设计 §6.3 的4GiB为工具链解阻塞参数；最终预算按 §7.5/§8.5/§17.1 为2GiB。先新增4GiB拒绝红测，再收紧配置和binary/glue门。initial64MiB生产装配留W1。
- 本批不改交易制度；native门面保持std re-export，业务规则、账户单位、撮合顺序与存档契约不变。最终须由未实施者独立审查完整diff。


## 有效协议修复与设计细化

- 三风险新增短 fixture 在旧基座上分别失败：锁持有检查点反复追加、同 pair 重复退出使 FREE_LEN=2、DEAD已发布时诊断code仍为0。2GiB binary/glue负控同样失败，见 `red-protocol-budget.log`；这些是真实协议/预算红，与先前“模块缺失”红证分开。
- 延迟回收先drain有限快照，同pair延迟登记去重；H1内检查是否已在free list；未知/身份错配退出零槽写且不摘除其他worker。
- D5从单CAS发布细化为“CAS WAITING→DeadWriting领写权→写code→CAS到DEAD发布”。这是修正旧实现发布竞态的两阶段细化，不是声称原设计已有该中间态。保持3bit与同代严格递增：Waiting0/DeadWriting1/Done2/DeadHardtrap3/DeadPanic4/Consumed5，Rust/TS镜像同步。join等待DeadWriting；重复D5幂等；输给DONE/CONSUMED/换代零载荷写。trampoline输给误报D5时释放本人结果恰一次并发workerExit，不panic。
- Rust wasm JoinHandle.join保留io::Result错误契约；world失败收集处仅wasm目标将io::Error装箱进入既有payload列表，native代码不变，避免两目标类型不一致且验证导出丢失错误详情。
- I6已覆盖H4/退休、OK/FAIL、FAIL_ACKED、超时/显式CANCEL、DONE/DEAD、两种CONSUMED的wake计数。A1/DeadWriting属于领写权中间态，不单独唤醒，最终发布负责唤醒；H1只在有登记等待者时notify。测试H1改局部RAII，断言panic不漏锁。
- 新夹具曾因Arc平台未转发新增D5缝导致5s失败；保留原失败日志后补齐转发，同断言48个Rust短测全绿。普通短测仅证明纯协议模块，不能替代engine/wasm实际编译。
- 首次长构建native/wasm共target产生cargo锁；发现后终止整个PGID1843774并核对进程归零，保留`*-interrupted.log`，不算通过。后续拆native/wasm target并等待源码冻结后重跑。


## 未实施者预审修复

- F1：新增H1前旧事件与engine新代REQ复用的确定性交错；TS以及Rust立即/pending回收均先红（正常复用被旧回收误判tombstone），后改为H1内重验完整代、配对join消费及manifest门链，旧代零槽写。见`red-recheck-*.log`。
- F2：TS encodeW/encodeJoin补位宽1..29、gen域及各自合法state集合，拒绝静默回绕；最大合法代正控保留。见`red-encoding-combo.log`。
- F3：组合CLI由独立子进程supervisor覆盖矩阵、构建、产物与清理，300000ms共享总时限、保留1000ms清理，显式jobs并记录。短fixture仅验证调度合同，不冒充browser实现。
- F4：废除code section原始0xFE扫描。使用Node内置V8 inspector反汇编，只匹配真实指令token；本次同步编译唯一scriptId、getScriptSource.bytecode逐字节身份核对、完整chunks及finally断开Session。const126合法LEB含FE00负控在旧门假绿，新门拒绝；函数名atomic.fence伪装、同进程先前/反序/并发模块均不误判，无新增依赖。
- F5：恢复main中独立npc_decision_diagnostics测试与fixture；新增产品门拒绝verification_facade_*，保特性导出隔离。
- F6：移除lib.rs已被历史撤回的CGU/GOTPCREL实测归因，只说明门控隔离codegen输入，不把声明当逐指令实证。
- Node兼容边界：当前机Node25.8.2短测绿；精确Node24.18.0二进制/官方SHASUM/registry下载均因TLS EOF或deadline失败（node24-*原件），已有Docker镜像无node24；不把Node25冒充最低版本实证。Node24.18实跑仍缺，CI运行前须显式保留该缺口。

- Node24缺口已关闭：环境员工从官方Docker `node:24.18.0-bookworm-slim` linux/amd64提取绝对路径 `.tmp/node24-env/node`；镜像digest `sha256:6f7b03f7c2c8e2e784dcf9295400527b9b1270fd37b7e9a7285cf83b6951452d`，node二进制SHA256 `41a74efb34cbde5c7632cdac0cf8bd1a14d0b8d73dc1e82755014d9a9ce70f5c`。实际版本v24.18.0/V8 13.6.233.17-node.50，PATH仅当前子进程前置；9文件全套相关Node短测在该精确版本全绿，含inspector伪opcode/伪名称/缓存并发与组合入口合同，见`node24-green.log`。此前网络失败仍保留，不回填为成功。


## 最终实编译与产物门收据

- 实际构建源码冻结 `7a557b6bf537439bb065b2ef0ec06b33bb3f8f7f`；`source-before.txt`与`source-after.txt`完全一致，2221个tracked非agents文件聚合SHA256 `198fe53ea6e3ca77134a7311fe465a519fb8b7976c383ede56a86c76ea1f8b41`；LICENSE源SHA256 `e57d1f88b21c0b41859a0c884768d168a16bb9924c4681b3eddc6b83b32ad3c8`不变。工作树只后补agents证据，不修改该批源码。
- 短测最终：Rust纯std 49/49（16测试线程、外10000ms）；精确Node24.18.0相关9文件69/69（8文件并发、case/外均10000ms），app/node TS类型检查及定向oxlint通过。reviewer独立同9文件曾因10s整命令deadline失败，随后单checker19/19独立绿；与本人69绿分别保留，不篡改独立负结果，API兼容有单独实证。
- 首个定向长batch外300000ms：native `cargo check -p engine -p web-wasm --lib -j16` 33.64s完成；正式release product生成；后续release harness因剩余总预算不足终止，整体失败（`long-build-timeout.log`）。第二次独立release harness同样300000ms超时，没有编译错误结论，原件`wasm-harness-build.log`；不将两段拼成单批成功。
- root与可信reviewer按W0原范围确认验证feature产物未规定release，故只追加一次代表性dev harness编译：相同nightly/build-std/atomics/panic=abort/2GiB链，`wasm-pack --dev --features verification-harness`、jobs16、`CARGO_PROFILE_DEV_DEBUG=0`、独立harness-dev/pkg，外300000ms，2m22完成。本产物只证明ABI与feature导出隔离，不充作release/performance/browser验收。正式release实际workspace参数为thin LTO/16CGU，不能沿用过时fat注释归因。
- dev harness每2s真实PGID进程树/CPU/线程与全机IO PSI见`harness-dev-cpu.log`，包含rustc并行与wasm-bindgen129线程/多核峰值；首批实际观察及两次超时采样局限见`build-cpu-snapshot.txt`。未把全机IO压力当实现性能证明；所有长任务结束后相关进程归零，无在途全回归。
- 正式release WASM 14,285,916 bytes，SHA256 `0df571042e83b5c5dbec6ae5f51f712ffd128f28451917af18b89d7a694d4371`；JS SHA256 `e01c87788cddf47270dc821e12224c166fec0c099e0deb1e50edf966b72e8208`。精确Node24实际产品门：唯一clock import、V8真实atomic、私有/verification导出零暴露、shared imported memory、binary/glue32768页(2GiB)全部通过（`product-gates.log`）。
- dev harness WASM 116,778,175 bytes，SHA256 `0b00b4504e38012caca16f5f20327d9771ad1f2da605c84b899cd9122bae670f`；JS SHA256 `4a710057d45beedf4a662f6294c3ba514cb3fc8f49e9c6eee54bb8b2492777c6`。5个facade/verification ABI导出、clock import、shared/2GiB通过；正式门正确拒绝verification包（`harness-gates.log`）。实际pkg均位于根`.tmp/wasm-w0-recovery/artifacts/`的product/pkg和harness-dev/pkg，不入Git。
- 实际组合CLI再次在已有正式production缓存上执行（不是重跑冷构建）：`CARGO_TARGET_DIR=根.tmp/wasm-w0-recovery/wasm-target node24 scripts/verify-wasm-thread-combo.mjs --jobs16`。入口外supervisor单批300000ms，构建7.30s，lock/build/product/shared均绿，browser-combo显式skipped，整体`green=false`；原件`combo-actual.log`。只满足W0常驻必绿门，不能称R4a完整组合全绿。
- 边界仍保持：wasm runtime=false；生产supervisor/子Worker/bootstrap、watchdog wasm收件替代、budget ledger、真实浏览器R5与runtime翻转未完成。没有full all、merge或push。正式完成以未实施者最终复核为门。
