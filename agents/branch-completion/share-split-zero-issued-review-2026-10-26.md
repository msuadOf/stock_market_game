# 拆缩股零发行量早拒：独立业务复核（2026-10-26）

复核者：`sol61_review_share_basis`。未实施本批业务、测试或正式文档；曾独立审查前置只读研究及作者勘误，本次另审实际实现。受审 worktree 为 `.worktrees/share-split-zero-issued`，基线 `a29d9be40a7bf59f5c8bdb9563ed70e8e3bed143`。

## 1. 范围与裁决

完整检查 `share_split.rs`、`share_split_tests.rs`、`session.rs`、`session/persistence.rs`、`docs/trading-rules.md` 与作者工作台账。未修改这些文件，只新增本复核记录；未构建、未运行 full、未操作 main 或远端。

**本批限定通过，无未关闭的有效发现。** 零发行量约束位于唯一分配函数，冻结前拒绝不可执行结果；Book 与直接内存存档恢复复用相同约束。公式、碎股奖励、面值/资本、方案类型和合法账户归零均保持原义。正式文档关于错误上下文的表述已按第 4 节收窄并复核；最终冻结对象见第 6 节。

## 2. 门禁三问

1. **大 A 语义与依据：通过。** 本批维护 `ShareRegistry::validate_holdings` 和 `CompanySpec::validate` 已有的正发行股本不变量，不把旧游戏 `floor(total/ratio)` 冒称现行 A 股统一规则。前置研究已核当前中登沪 2026-05、深 2025-12 原件的送转碎股排序、不同精度单位，以及特殊缩股公告的有限适用范围；这些原件没有授权自动补 1 股、现金替代或证券结束。本批也未新增这些效果。100 股申报单位没有用于推导登记总量下限。
2. **必要性及最小范围：通过。** `allocate_share_split` 只在原算法计算出的 `issued_shares_after == 0` 时返回结构化错误。既有 `register` 先分配后写入，失败不会进入 `Registered`；既有 `validate` 重放同函数，严格 Book JSON 无须新解析器。`validate_save_slot` 在原有所有关卡之后逐本调用 `validate`，堵住直接内存 `SaveSlot` 绕过 serde 的路径，并保留原首错顺序。没有新增依赖、兼容 fallback、参数或第二套取整算法。
3. **边界、跨层语义及复杂度：通过当前修复范围。** 新用例分别覆盖总量小于/等于/大于 `ratio`、普通账户为 0 而发行人仍为正、失败 book 不变、旧自洽零回执的 Book/JSON 拒绝、真实完整存档的内存/JSON 拒绝、原会话不变且不 poison，以及非零 Registered 完整往返。`SessionError::ShareSplit` 保留 `Error::source`；用例对 source 作 downcast 并比较事件、证券、方向、比例、before/after，避免只断言泛化错误。新增恢复校验不借机引入未来日期或全部公司域重构。

## 3. 实际行为与 TDD 证据

| 输入或入口 | 本批结果 | 复核关注点 |
|---|---|---|
| `A=1、B=1、N=3` | 分配和 register 均失败；before=2、after=0；book 保持 Announced。 | 维护正发行股本，未把结果改成 1 股。 |
| `A=1、B=2、N=3` | `A=0、B=1`，总量为 1。 | 不能误拒单账户归零。 |
| `A=1、B=3、N=3` | `A=0、B=1`，总量为 1。 | 保持旧 floor 模型。 |
| `A=1、B=5、N=3` | `A=0、B=2`，总量为 2。 | 碎股按原余数获配。 |
| 真实 Simple 完整存档，当前总股本 `100000`，未执行 Registered 回执 `N=100001` | 直接内存 restore 与完整 JSON 反序列化均拒绝；当前 registry、存档输入和原会话均不改变。 | 证明“正 registry + 零未来回执”原本确可受理；未假称当前 registry 已为 0。 |
| 同一完整 fixture，`N=2` | 非零 Registered JSON/restore 往返保留。 | 未关闭全部拆缩股恢复。 |

作者 `red4-tests.log` 为 8 通过、3 失败、3.72 秒。其中分配/register 的实测拒绝状态为 `(false,false)`；完整内存恢复/JSON 同样为 `(false,false)`，清楚显示旧实现接受了本次要拒绝的事实。最初 fixture 日期错误的两轮不计目标缺陷红证；错误 filter 所得 0 tests 不计通过，作者台账已如实区分。

已核作者 green 日志：share_split 11/11（3.52 秒）、registry 30/30（0.01 秒）、Simple 拆缩股账务 4/4（0.18 秒）、save validation context 2/2（0.20 秒）。普通命令均注明外部 10000ms 进程树 deadline 与 `--test-threads=8`；不以这些短测代替全链验收。

本复核者独立复用冻结 binary，执行：

```sh
node scripts/run-with-deadline.mjs 10000 -- .tmp/share-split-zero-issued-target/debug/deps/engine-3eb2aa70a85e274e company::share_split::tests --test-threads=8
```

实际 11/11 通过，0 失败，3.50 秒，退出码 0。没有冷构建或改写 cache。binary SHA-256 `d54704d5cc63a3e5056f87a9a49c49f5472cc125fe9b80a49087edba4357fe9e` 与作者冻结值一致；作者 Cargo JSON 产物指向同一 worktree/可执行文件且 `fresh=true`。4 个 Rust 源文件 SHA-256 也与 `freeze-source-before.json` 一致。

## 4. 唯一返回发现：错误上下文的文档适用范围

正式新增段落把领域、JSON、直接内存恢复连写后，笼统声明“错误保留事件、证券、方向、比例与换算前后数量”，容易被读成所有入口都保留完整 cause。但现有 `decode_save_slot` 仍将 serde 错误映射为 `InvalidSave(2080,[0])`；server、desktop、wasm 的 `failure_description` 也仍将陌生领域错误脱敏。

应仅说明领域错误及直接内存恢复的 typed source 保留这些上下文，严格 JSON 同样拒绝不可执行回执但沿用现有解码错误边界。该发现只需要收窄文案，不要求本批扩大修改 JSON decoder 或宿主 wire。

作者已修正，复核最终 `docs/trading-rules.md` 明确区分领域/直接内存 source 与通用 JSON 解码/宿主脱敏，内容与代码一致。发现关闭；Rust 源文件与 binary 未变，无须因这项文案修正重复业务测试。

已核三宿主的错误转换均有既有默认分支，且 `SessionError` 本身没有 serde/TS 导出；新增 source wrapper 不直接新增 wire 字段或把 event 字符串未经处理公开。直接内存路径的完整 source 已有实际测试；跨宿主 UI 展示本批未执行，不冒称完整验证。

## 5. 通过范围的明确限制

本复核只覆盖零发行量提前拒绝及其恢复契约。旧账簿当前可读但不可执行的零回执将被显式拒绝，接受范围变化应如实保留在记录中；没有兼容补 1、补现金或改写存档。不会因本修复认定零发行股本的证券结束/清算功能已实现。

R3a/R4 真实结算、原单位/新单位日内相位、税务 basis、混合交易日历及三宿主完整验收仍由其各自批次负责。本批通过不能作为这些事项完成的证据。

## 6. 最终冻结及交付对象

| 文件 | 最终 SHA-256 |
|---|---|
| `packages/engine/src/company/share_split.rs` | `478267f68e63c5052c08f3d8a1d58d0c363785fa10fcd6a3cf298b31091c1691` |
| `packages/engine/src/company/share_split_tests.rs` | `3d5b0e5a0851cab2e2fd06270c639e387f84447b93fefd1d79943ddbe858e175` |
| `packages/engine/src/session.rs` | `6e8c6cb73e0a306334881135067d251a897417c122a982a7e41419a92a2e715a` |
| `packages/engine/src/session/persistence.rs` | `d62990a28e04227737c81acd8122b23e3f5f56ec18efc120eb0b37243d63f7b8` |
| `docs/trading-rules.md` | `906d0b17556e9c08acb02f20cf968dd8e74bb634b93838b621c46d4127394f01` |
| `agents/branch-completion/share-split-zero-issued-2026-10-26.md` | `328e56ff0fc0fb8540564540669b37180afc591291ffafb5a3373189136f1987` |

随批归档 [red-tests.txt](share-split-zero-issued/red-tests.txt)、[green-tests.txt](share-split-zero-issued/green-tests.txt)、[manifest.txt](share-split-zero-issued/manifest.txt) 已读取；红绿文本与原始 `red4-tests.log`、`freeze-tests.log` 字节相同。manifest 绑定 4 个 Rust 源文件及实际可执行物，另列文案修正后的 docs SHA。原先错误日期 fixture 和 0 tests filter 不包装为目标红绿证据。源码/docs 的 `git diff --check` 通过，台账/复核记录无行尾空白；作者暂存全部原件后，`green-tests.txt:16` 的原始 stdout 尾空行产生 `new blank line at EOF` 提示，保留原始字节，不误报整批空白检查全过。复核者未执行构建或提交，构建日志和并发配置为作者证据，不能混称复核者独立运行过构建。

构建资源取证局限：作者说明在 green 构建期间确实用 `ps` 观察过 `rustc`：PID `2956957`、PPID `2956644`、CPU `97.7%`、NLWP `4`；red 为 CPU `81.6%`、NLWP `4`。这些是作者工具输出，未独立归档采样文件；后续逐线程采样时构建已结束。实际 `-j 32` 参数明确，但不能从单次前端阶段样本证明后端曾有效多核占用，也不能由它推断整段构建均为单核。本复核不签署完整构建资源验收，不为补取样重跑已通过构建；此局限已向 root 报告，后续长构建应在运行期保留有效进程/线程采样。
