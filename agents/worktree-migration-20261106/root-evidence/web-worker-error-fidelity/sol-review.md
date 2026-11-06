# Worker 错误保真独立复核

2026-10-25；未实施审查者：`/root/sol61_proc_exit_review`。

**APPROVED：仅批准 `wasm-worker.ts` 与 `wasm-worker-failure.test.ts` 两文件精确补丁。**

- change.patch（历史路径：`change.patch`） SHA：`d259965bf5e14fa70dcbd446601ce035ee21d94c05b9ac75c34aca45a8cfd3a5`。
- 批准生产源 SHA：`a89f619c673ed6291a5a925aaaa3d2a99adb4678a07ae57311a3ad96c9f5828a`。
- 批准测试 SHA：`d293a264b8635d8d71b3efb82f871efaa6f7fb68c3c0b12f4c73bb87ed7bab10`。

**领域语义不变。** 仅修Web错误传输边界，不改A股价格/数量/时间单位、账户、撮合、费用、交易规则、Rust或存档，没有新增制度判断、依赖或协议字段。

**必要且最小。** 已亲核 classifier、原projection和严格parser：真实带code的标准Error被判为structured，但 `Object.entries` 丢非枚举message/code，导致错误处理再次抛ProtocolError、掩盖原错。生产修复仅使用已分类code/message，并按hasOwn提取既有白名单optional，保原生非枚举cause。`parseHostFailure`不变，不补默认字段、不吞访问或校验错误。optional的枚举性不再决定诊断是否丢失；没有拓展字段集合。

**边界与TDD有效。** 四个新增case覆盖标准Error、非枚举code/原生cause与完整optional、非法recoverable/recoveryActions、getter/Proxy访问失败。坏optional仍按原parser明确抛错；访问失败仍抛原异常，未制造成功failure消息或默认值。原code getter、revoked Proxy、不可打印异常及真实message listener断言保留。fixture仅捕获并关闭真实MessageChannel，finally恢复构造器/self，未mock调度器。

已亲读有效red-clean与green完整TAP：相同测试SHA，旧生产源自然exit1、0.572s，14过3败；失败准确为非枚举message/code及其遮蔽的optional校验。新生产源自然exit0、0.559s，**17/17**，无skip、timeout或残留进程组成员。最早ports泄漏外杀的red保留，不当自然退出红。两个文件真实进程隔离并发，case10000ms，独立Python执行9000ms后启杀、总10000ms含清理；lint显式4线程退出0。

完整diff、实际源/测试/lockSHA与相同锁依赖复用身份已核，独立apply-check退出0。证据指纹见 sol-review-binding.json（历史路径：`sol-review-binding.json`），详见 report.md（历史路径：`report.md`）。审查者未实施、编译或运行测试。本包没有运行完整Web/类型编译/Rust/full，不据定向绿宣称整树通过；合并后的新before与正式验收由根另行安排。
