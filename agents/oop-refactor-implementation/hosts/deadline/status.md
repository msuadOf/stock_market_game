# deadline 实施状态

本目录对应 `actions.json` 的两个动作。源码实施与自审已完成，独立复核三门通过，R1 修正复审已关闭。
结构化逐动作台账见 [`status.json`](status.json)；独立审查见
[`review-deadline.md`](../review-deadline.md)。
本轮只改本地脚本与短测试，不改变 engine、交易制度、A 股概念、单位或存档/API 契约。
已核对 `AGENTS.md`、`docs/principles.md`、`docs/testing.md`、`docs/architecture.md`、
`docs/open-questions.md`、`docs/error-handling.md` 和 ADR-0023、ADR-0027。

## hosts-N08

- 状态：实施完成，修正前全部 54 项短测通过，独立复核通过；原 sandbox 限制已由权限重跑核销。
- 改动文件：`scripts/run-with-deadline.mjs`、`scripts/run-with-deadline.test.mjs`。
- 对象：私有 `BoundedCommandRun`，聚合 options、child、stdout/stderr、output callback error、
  timeout/abort/settled、execution/hard timer、signal listener 和 Promise completion。
- 方法：`start()`、`#onOutput()`、`#abort()`、`#onError()`、`#onClose()`、
  `#settleOnce()`、`#cleanup()`。
- caller：`runBoundedCommand()` 继续作为 public facade；`runWithDeadline()`、
  `run-long-validation.mjs`、`run-web-tests.mjs`、`run-full-regression.mjs` 继续使用相同
  options、Promise 结果和 injected run。后三个 caller 无需修改 API 接线。
- 行为边界：保留 10000ms/300000ms 上限和 cleanup reserve；cleanup 仍先清 timer、
  再移除 signal listener，再 resolve/reject。保留 callback error → abort → timeout →
  非零 exit 的 close 错误优先级；重复 close/error 不再次完成。
- 新增测试：pre-aborted signal 不 spawn、运行中 abort 等待 close、重复 close/error/abort
  只清理一次、spawn error 后重复 close/abort、callback error 优先于 sibling abort，以及
  injected child 的双流累积与原 chunk callback。
- 未完成：无未关闭复核发现。
  Windows taskkill 的 fire-and-forget 行为保持原样，本批未修复或声称解决。

## hosts-R2-N19

- 状态：实施完成，定向测试通过，独立复核及 R1 修正复审通过。
- 改动文件：`scripts/run-full-regression.mjs`、`scripts/run-full-regression.test.mjs`。
- 对象：`ArtifactInventory` 只拥有唯一 deep-cloned/deep-frozen 原 record。保留 JSON
  字段插入顺序、extra fields 和 duplicate artifacts，没有排序、删字段、去重或规范化副本。
- 方法：`fromBuild()`（仅构建新 record 时计算 identity digest）、`fromDecodedInventory()`
  （先对原 decoded record 去掉 identity_digest 后实算摘要并比对）、`toJson()`、
  `artifactDescriptors()`、`validateArtifactDescriptor()`、`validateArtifactMetadata()`、
  `validateArtifactDigest()`。实测 metadata/digest 与对象内部按 index 取得的 seal 比较。
- caller：`buildFullRegressionArtifacts()` 创建值对象，再以 `toJson()` 交给原子 JSON adapter，
  public 返回 inventory 继续为 plain record；`readAndValidateInventory()` 读取原 JSON 后重建
  值对象，由 `executeFullRegression()` 使用验证后的 executable 描述启动测试。
- I/O 边界：路径 containment、lstat/realpath、read/hash、原子写入和临时文件清理继续由
  filesystem adapter 执行。descriptor 仍逐条验证，保留前一条 filesystem 失败的优先级。
- 新增测试：原 decoded 与 toJson 输出被 caller 修改不影响封存值；descriptor 本身不能修改；
  reorder 保留旧 digest 被拒绝，重新实算 digest 后接受；extra/duplicate 被原样保存；
  duplicate 在执行阶段保持两个 ordinary binary 调用及 required long target 歧义拒绝；
  缺 schema/digest/artifacts、source/workspace/target/temp/fingerprint 漂移、invalid descriptor、
  byte/hash seal 修改、未 reseal extra/order 均在启动任何 test 前拒绝。
- 未完成：无未关闭复核发现；不宣称消除文件替换竞态，不宣称 Release manifest 可信或可移植。

## 验证证据

所有测试命令使用 Node case `--test-timeout=10000` 和进程树 supervisor 10000ms；
显式 `--test-concurrency=4`。fixture 中 Cargo、Rust binary、Web batch 均用 injected run，
真实 child 仅为短 Node `-e` fixture。没有启动真实 Cargo、build、E2E、完整回归或长验收。

- 红灯：新 `ArtifactInventory` 所有权测试先因 `typeof ArtifactInventory === undefined`
  的断言失败；abort/完成态新增测试在原行为上通过，作为 refactor preservation 覆盖。
- 首次绿灯：5/5 定向测试通过。
- 最终定向：34/34 通过，Node 224.23186ms，见 `targeted-tests.log`。该子集不能代替以下全部短测。
- 全部四脚本短测：54 项，46 pass / 8 fail，Node 3360.009272ms；exit 1。
  完整错误栈见 `all-short-tests.log`，未删除或弱化既有断言。
- 父代理安排 root 在 sandbox 外重跑相同四文件和断言：54/54 pass，0 fail，2741.378114ms，
  exit 0；沿用 concurrency 4、Node case 10000ms、runner 10000ms，并增加
  `timeout -k 1 9` 进程外门禁。证据：
  [`deadline-short-test-result.json`](../../validation/deadline-short-test-result.json)。
  原 sandbox 的 8 项失败已核销，历史失败日志保留。
- `git diff --check --` 四个源码/测试文件通过，仅使用 Git 只读检查，未 Git 写入/提交。

完整短测复现命令：

```sh
node scripts/run-with-deadline.mjs 10000 -- node --test --test-timeout=10000 --test-isolation=none --test-concurrency=4 scripts/run-with-deadline.test.mjs scripts/run-long-validation.test.mjs scripts/run-web-tests.test.mjs scripts/run-full-regression.test.mjs
```

8 项失败是：CI cache directory probe（`spawnSync .../node EPERM`）；Cargo progress、rendered
error、incomplete JSON 三项（pipe 输出未送达）；stdout/stderr capture、超时前输出、失败诊断、
streaming callback 四项（pipe 输出未送达）。没有把这些失败记为通过。

对照证据：`baseline-run-with-deadline.mjs` 是 `git show HEAD:scripts/run-with-deadline.mjs`
取得的只读基线快照。`baseline-capture.log` 返回 `{ stdout: '', stderr: '' }`，
`native-capture.log` 仅返回 `close [ 0, null ]`；两者都没有收到期望的 `data`。
这证明 output 缺失在基线及独立原生 spawn 路径也存在；root 已通过 sandbox 外相同断言
54/54 通过，确认原失败是本地 sandbox 执行限制，无需修改实现或断言。

最小基线复现：

```sh
node --input-type=module -e 'import {runBoundedCommand} from "./agents/oop-refactor-implementation/hosts/deadline/baseline-run-with-deadline.mjs"; console.log(await runBoundedCommand({command:process.execPath,args:["-e","process.stdout.write(\"data\")"],timeoutMs:1000,captureOutput:true}));'
```

最小原生复现：

```sh
node --input-type=module -e 'import {spawn} from "node:child_process"; const c=spawn(process.execPath,["-e","process.stdout.write(\"data\")"],{stdio:["ignore","pipe","pipe"],detached:true}); c.stdout.on("data",x=>console.log("out",x.toString())); c.stderr.on("data",x=>console.log("err",x.toString())); c.on("close",(...v)=>console.log("close",v)); c.on("error",x=>console.log("error",x));'
```

本子代理没有申请或执行 sandbox escalation；root 已完成权限重跑，父代理继续安排独立复核。
源码文件已冻结供独立 review。

## 独立复核修正：深层 extra JSON

review_deadline 发现初版 `structuredClone()` 对 depth 2000 合法 JSON 的栈限制比原
`JSON.parse()`/`JSON.stringify()` 更严，构造封存值时新增 `RangeError`；这是有效接受边界回归。
新增短 fixture 先复现红灯，证据 `deep-json-red.log`（在 `ArtifactInventory` constructor 的
`structuredClone` 抛出 `Maximum call stack size exceeded`）。

修正 constructor 与 `toJson()`：改用原 wire format 的 `JSON.parse(JSON.stringify(record))`
复制，仍在 clone 前对原 decoded record 校验传入 identity digest，不重算替换输入摘要；
deep freeze 改为 iterative stack，避免 recursive freeze 再次引入深层栈限制。
depth 2000 fixture 含每层 null 字段，检查 fromDecoded、toJson、修改输入与输出的深层叶子
均不能改变 owner，以及原 inventory 文件经 injected 执行 adapter 接受。

只重跑 4 项针对修正的短测（深层 extra、原 immutable/order/extra/duplicate、duplicate 执行、
identity/seal 拒绝），4/4 通过，见 `deep-json-green.log`；仍使用 10000ms supervisor、
case 10000ms、concurrency 4。初版 54/54 权限重跑证据为修正前记录，本次没有重跑完整四文件。
修正后 `git diff --check` 通过。review_deadline 对修正后的 diff 三门复审通过，R1 已关闭。
当前四文件共 55 case 的整套未重跑；初版 54/54 与修正后相关 4/4 分别报告，不合并冒称完整结果。

## Root 最终代表性验证（2026-10-03）

本组最终状态以 status.json 的 final_validation 为准。root build07 与 all-targets check08 均通过；root 选定精确 Rust case 的逐条结果已按 source 映射，WS 两个 bind sandbox EPERM 保留初始失败记录并由沙箱外原断言 2/2 通过核销。Writer 5/5 与桌面 CLI 5/5 通过。未选中的旧测试、完整 suite、API doctest（actors）及真实 matrix/E2E/性能未据此声称执行。历史“worker 未运行/待 root”段落保留为过程证据，当前没有未关闭实施或独立复核发现。
