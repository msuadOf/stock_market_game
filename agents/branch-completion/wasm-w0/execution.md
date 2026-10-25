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
