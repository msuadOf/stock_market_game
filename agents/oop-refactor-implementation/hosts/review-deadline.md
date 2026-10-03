# deadline 独立复核

日期：2026-10-03。复核者未参与源码实施；基线为
`b89afb3346743a4b4fccf26c9ac9ff108595f696`。

## 复核范围与结论

已阅读 `AGENTS.md`、`docs/principles.md`、`docs/testing.md`、
`docs/error-handling.md`、`docs/architecture.md`、`docs/open-questions.md`、
`docs/trading-rules.md`、ADR-0023、ADR-0027，以及本批 `deadline/actions.json`、
`deadline/status.md`。审查四个改动文件的完整 diff 与源文件全文：

- `scripts/run-with-deadline.mjs`
- `scripts/run-with-deadline.test.mjs`
- `scripts/run-full-regression.mjs`
- `scripts/run-full-regression.test.mjs`

另读 `run-long-validation.mjs`、`run-web-tests.mjs` 全文，确认它们相对基线无 diff。

初轮结论：`hosts-N08` 通过；`hosts-R2-N19` 存在一项 P2 acceptance 回归，须修复并再次复核。
修复后结论：R1 已修复且独立复核通过，两项动作的源码审查均通过；修复后全部短测证据
由父代理协调，不能沿用修复前 54/54 冒充最新四套脚本全通过。
没有修改源码、真实 Cargo/build、完整回归或长验收，也未委派其他 agent。

## 三门核对

### 1. A 股语义与依据

本批只移动本地工具的资源生命周期和库存不变量，未改变 engine、撮合、交易时段、
价格时间优先、T+1、费用、股/分单位、存档/API 或 UI 规则。`fullRegressionSteps`、
Rust 执行策略、必跑长用例、源码指纹输入以及 Web shard 调用均保持原样。
与 ADR-0023 的虚拟前史和实际撮合边界、ADR-0027 的同一 engine 与独立构建目标一致。
没有新增交易制度判断，不需要为本批重新制定或重新核验交易所规则。
所读 `trading-rules.md` 中官方依据日期和中国结算来源复核缺口均为现有登记，
本复核不将其描述为 2026-10-03 已重新访问通过。

### 2. 必要性、范围与对象成熟度

`BoundedCommandRun` 拥有一次 command 的 child、双流、callback error、abort、timeout、
两个 timer、signal listener 和完成态，生产 facade 每次调用创建一个对象。
没有继承层级、全局 registry 或新增 caller API。`ArtifactInventory` 拥有 build/execute
之间的封存 record 及 seal 校验；filesystem、realpath/lstat/read/hash、原子写入仍在 adapter。
两项都有真实资源或不变量与生产 caller，符合动作范围。没有引入依赖或混入交易功能。
唯一需要修正的范围漂移是以下 R1：新增复制机制缩小了旧 extra JSON 接受范围。

### 3. 边界、漂移与复杂度

`BoundedCommandRun` 的 spawn/options、stream 注册、execution timer、hard timer、
abort listener、error/close 注册顺序保持原样。`#settleOnce` 先标记 settled，再清两类 timer、
移除 listener，最后 resolve/reject；重复 close/error 被忽略。close 仍依次采用 callback error、
abort、timeout、非零 exit 优先级。pre-aborted signal 仍在 spawn 前拒绝；运行中 abort
仍终止 tree 后等待 close 或 hard cutoff。本批新增短测覆盖 pre-abort、运行中 abort、
重复完成事件、error 后 close、callback 与 abort 优先级、原 chunk 与双流累积。

Windows `taskkill` 仍 fire-and-forget/unref，POSIX kill 仍仅忽略 ESRCH；没有暗增
“确认整个 tree 已关闭”的保证。abort 恰在 injected spawn 同步内部发生的竞态、
output listener 在 settlement 后保留等行为均源于基线，本批没有声称解决。

Inventory 在输入时先比对原 decoded record 去掉 `identity_digest` 后按原 JSON.stringify
顺序计算的摘要，未重新封存输入来掩盖篡改。metadata/digest 与私有 record 中对应 index
的 seal 比较。字段顺序、extra、duplicate 没有规范化；测试保留 duplicate ordinary
执行两次及 required long target 歧义失败的既有行为。descriptor 逐条校验与 filesystem
交替进行，保留前一条 filesystem 错误先于后一条 descriptor 错误。冻结的 JSON plain
record 与复制输出阻止 alias 修改，但深层 JSON 输入存在 R1。

## 有效发现

### R1 / P2：复制机制新增拒绝原可接受的深层 extra JSON

位置：`scripts/run-full-regression.mjs:178-179`；另需核对 `:207-208` 的输出复制和
`:167-170` 的 recursive freeze。

`fromDecodedInventory()` 的所有原 identity/digest/descriptor 条件均可满足，
但 constructor 对 decoded record 进行 `structuredClone()` 时，2000 层的合法 extra
JSON 会抛 `RangeError: Maximum call stack size exceeded`。旧实现只 JSON.parse 并
JSON.stringify 计算摘要，在同一 Node 环境可处理该输入，没有 extra 深度拒绝政策。
这不是旧 digest 本身不支持的极深 JSON：探针在 2000/3000 层均确认原 digest 匹配。
仅约 20 KB 的输入因此被新增 clone 限制拒绝，违反 `hosts-R2-N19` 保留既有 extra
acceptance 的明确要求。

复现输入：`extra` 初值为 null，循环 2000 次 `extra = { child: extra }`；其余 required
字段和 artifact descriptor 使用合法值。按原函数算法计算 identity digest，再经
JSON.stringify/parse 得到真正 decoded record，传入 `fromDecodedInventory()`。
一个带 `timeout -k 1 3` 的纯值对象探针耗时约 139ms，无文件、child fixture 或真实验收。

| extra 深度 | 原 digest 匹配 | 新对象创建 |
|---|---|---|
| 500 | true | 成功 |
| 1000 | true | 成功 |
| 2000 | true | RangeError |
| 3000 | true | RangeError |

建议：对 JSON record 使用保持旧 JSON 接受范围与 key order 的复制机制，freeze 改为
不额外消耗递归调用栈的遍历；constructor 和 `toJson()` 一并修正。补一个 depth2000
短 fixture，验证创建/输出成功、摘要与字段顺序保持、caller 深层修改不改变封存值。
不要通过新增深度配额或忽略 extra 字段解决。本发现已发送给父代理；实施者已接收。

## 验证证据与限制

读取父代理提供的 `validation/deadline-short-test-result.json`：相同四套脚本短测
54/54 pass，0 fail，exit 0，Node 2741.378114ms；concurrency 4，case timeout10000，
runner10000，并有进程外 `timeout -k 1 9`。该证据来自 root 权限重跑，本复核未重跑。
历史 sandbox 8 项失败及其基线复现记录保留；未要求因 sandbox pipe/EPERM 限制改源码。
本复核另执行 `git diff --check --` 四个改动文件，通过。

现有 54/54 不能覆盖 R1，因为测试只使用浅层 extra。修复后需要新的短测证据与独立
复核；本记录不声称通过真实 build、Cargo、E2E、完整回归或 Windows taskkill 验收。

## 修复复核

同日收到修复后重新审查相对基线的完整 inventory 源码/测试 diff。constructor
（现 `run-full-regression.mjs:182-184`）和 `toJson()`（`:212-213`）均使用
`JSON.parse(JSON.stringify(...))`，与持久化 JSON 的接受边界一致；原 record key order、
extra 和 duplicate 在 wire JSON 中保留。freeze（`:167-173`）改为显式 pending stack，
每个 object/array 及其 children 均冻结，无新增递归栈深度限制；pending 与 record 只包含
JSON.parse 生成的 acyclic plain 数据，不需要 cycle 检测或更多框架。

`fromDecodedInventory()` 仍先校验输入 digest，之后才复制，未重新封存或覆盖输入摘要。
filesystem descriptor 校验顺序、build 返回 plain inventory 与三个 caller 契约无变化。
没有引入深度配额、丢弃扩展字段或忽略异常。

新增 `run-full-regression.test.mjs:490` 的 depth2000 fixture 每层包含 null，覆盖
fromDecoded 创建、toJson 完整 wire JSON、输入和输出的深层叶子修改隔离，以及 injected
执行四个 command 均到达的 acceptance。读取 `deadline/deep-json-red.log`：1 case 在
structuredClone 处正确红灯；读取 `deadline/deep-json-green.log`：相关 4 case 全通过，
0 fail，138.790248ms。实施记录的命令为 Node case10000、runner10000、concurrency4。
本复核未运行这批测试；再次执行四个文件的 `git diff --check` 通过。

R1 核销。复核后未发现其他有效问题，A 股语义、必要性/范围、遗漏边界/跨层语义三门
通过。该结论是源码与定向证据复核；最新四套脚本共 55 项短测的完整证据仍由父代理
负责，既有 54/54 仅对应修复前快照。
