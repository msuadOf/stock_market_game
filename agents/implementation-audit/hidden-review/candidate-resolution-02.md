# 候选裁定 02

## 范围与依据

静态复核隐藏扫描 `batch-048.md`、`batch-078.md`、`batch-080.md` 所提候选，核对基线 `43b1aa5` 的调用代码、现行 `docs/testing.md`、相关 ADR、总账及 `exhaustive-review/resolution.md`。未运行测试或构造利用场景；不涉及 A 股交易规则。

## 裁定

| 候选 | 裁定 | 理由 |
|---|---|---|
| `run-with-deadline.mjs` 超时消息声称 process tree 已终止 | **并入既有 G62，补充边界** | `scripts/run-with-deadline.mjs:10-18` 的 POSIX 分支只发送进程组 `SIGKILL`；Windows 分支异步启动 `taskkill` 后 `unref()`，不检查其退出结果。`:80-86` 在 hard deadline 可于直接 child `close` 前拒绝，文案仍称树“was terminated”；`:118-127` 则只依据直接 child 的 `close` 事件描述树已终止。`docs/testing.md` §运行时限要求长任务 deadline 覆盖进程树终止与清理，不能用信号请求或直接 child close 证明后代已退出。总账 G62 已登记嵌套 detached POSIX 进程组逃逸，并在 `sweep64.md` 留下 Windows taskkill 未确认的同类边界；因此不新建重复 G，建议将错误状态表述也列入 G62 的验收边界。未确认每次 timeout 都有遗留进程，也未复现现场失败。G39 是 K7 输出相等契约，与本候选无关。
| `validateArtifacts` 在 containment 前 realpath/readFile，且允许内部符号链接 | **确定新 G** | `scripts/simulation/run-escrow-verification-matrix.mjs:367-418` 对声明路径先做词法检查；`:397-399` 随后 `realpath` 并读取目标；`:402-404` 才检查 canonical containment。因而越出 output 的符号链接目标内容在拒绝前已读取。目标落在 output 内时，canonical containment 本身不会拒绝 symlink。该验证器由新建结果及 PASS 复用路径共同调用（`:660`、`:727`），并在后续以 hash/长度决定 artifact 是否可信。它不是单纯 OOP 候选是否获批的问题：这是现存 validator 的文件读取顺序及路径约束不一致，且输入 artifact 由外部 harness 写入；需作为独立工具边界登记。`docs/testing.md` 和核对的 ADR 未找到授权跟随符号链接或先读后判 containment 的约定。不开扩为竞态防护、任意文件读取攻击链或未来 OOP 范围；只确认静态可见的 containment-before-read 缺口。与 G39、G61、G62 的受理轨迹、异常清理及进程组监督语义不同，不应重复并入这些项。

## 去重与限制

- `batch-048` 和 `batch-080` 对 `validateArtifacts` 的发现为同一缺口，合并计一次；matrix runner 的 `EscrowVerificationRun` 已存在不改变该读取顺序结论。
- `batch-078` 的文案发现与 `batch-080` 提及的 symlink 风险相互独立；前者归 G62，后者新建工具 G。
- `exhaustive-review/resolution.md` 中 G39、G61、G62 均没有覆盖 matrix artifact 的 canonical containment-before-read。G62 及 `sweep64.md` 已覆盖超时树未完全收敛和 Windows 未确认边界，故文案问题应补入 G62 而不重复编号。
- 只完成代码和规范静态核对。未验证 timeout 后实际进程存活情况，也未运行 matrix、测试或安全复现；结论不构成运行结果。
