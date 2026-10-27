# 拆缩股零发行量早拒修复（2026-10-26）

## 范围与依据

基线为真实 `main`：`a29d9be40a7bf59f5c8bdb9563ed70e8e3bed143`。本批只维护既有 `ShareRegistry`、`CompanySpec` 的正发行股本不变量，不改变整数 `Split`／`Consolidate`、汇总取整、碎股余数排序、面值／资本模型。

依据为根工作区的独立研究复核 `agents/branch-completion/share-fraction-basis-review-2026-10-26.md` 第 3 节；该复核将游戏支持边界与现行 A 股统一规则明确区分。总发行股本归零的结果无法进入既有名册；拒绝该结果不是保底补 1 股政策，不自动产生现金替代。普通账户可以归零，只要发行人换算后的总发行股数仍为正。100 股买入申报单位不用于推导发行总量下限。

## TDD 设计

- `allocate_share_split`：总量小于／等于／大于 `ratio`；合法普通账户归零、发行总量为 1 或 2；零发行量拒绝。
- `ShareSplitBook::register`：失败保持原 `Announced`、无冻结快照／成功回执。
- 手工构造旧实现接受的自洽 `Registered` 零回执：`validate` 与严格 JSON 同步拒绝。
- 真实 `GameSession::save()` 产出正发行股本完整 `SaveSlot` 后挂接尚未执行的零回执：直接内存 restore 与完整 JSON 严格反序列化拒绝，输入及原会话不变、不 poison。
- 合法非零 `Registered` 回执完整 JSON／restore 往返保持。

## 验证状态

测试已先于实现落盘。独占 COW 构建目录通过 `cp -a --reflink=always` 继承冻结 R1 cache，目标文件 inode 与源不同且 link count 均为 1；没有 hardlink、full-copy fallback 或对原 cache 写入。

先前两轮完整存档 fixture 因注册日未完成而失败，原日志 `red-tests.log`、`red2-tests.log` 保留，不计作目标缺陷红证。修正为先真实运行一日，再通过 `ShareRegistry::close_day` 显式补齐 main 尚未安装的无成交 R 日候选后，`red3-tests.log`、`red4-tests.log` 确认：旧实现接受零发行量 `allocate`、`register`、`Book::validate` 与直接内存完整存档／完整 JSON；合法非零完整存档已通过。`red4` 为 8 过 3 败，3.72 秒。

实现的单一权威约束位于 `allocate_share_split` 返回成功回执前；`register` 写入前及 `validate` 重放自动复用。直接内存 `SaveSlot` 在既有所有存档校验之后逐本 `validate`，以 `SessionError::ShareSplit` 保留结构化领域错误和 `Error::source`。该 Rust 枚举没有 serde／TS 导出，既有宿主 source 链继续脱敏；未新增 wire 字段或借用无关 fault／poison。

构建使用独占 COW 目录、`cargo test -p engine --lib --no-run --offline -j 32`、`CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0`，每条构建均由进程外 `run-long-validation.mjs 300000` 监督。green 构建 61 秒；普通测试各命令外部进程树 deadline 10000ms、`--test-threads=8`：

| 范围 | 实际结果 |
|---|---|
| `company::share_split::tests` | 11/11，3.52 秒 |
| `company::share_registry` | 30/30，0.01 秒 |
| `company::simple::finance::share_split::tests` | 4/4，0.18 秒 |
| `session::persistence::save_validation_context` | 2/2，0.20 秒 |

首次使用错误 filter 的 `finance-tests.log`、`persistence-tests.log` 实际为 0 tests，不计通过；修正 filter 后的 `finance2-tests.log`、`persistence2-tests.log` 才计入上表。原件均在根工作区 `.tmp/share-split-zero-issued/`，未运行 full 或跨宿主完整验收。完整 diff 已交未实施者 `sol61_review_share_basis` 独立复核，等待终裁。

最终冻结：先记录五个源文件 SHA-256，再运行 Cargo freshness 校验并执行相同 11 例，实际 11/11、3.47 秒，随后核对源码字节未变。Cargo 返回 `fresh=true`，实际二进制 SHA-256 为 `d54704d5cc63a3e5056f87a9a49c49f5472cc125fe9b80a49087edba4357fe9e`。关键原件随本批归档：[red-tests.txt](share-split-zero-issued/red-tests.txt)、[green-tests.txt](share-split-zero-issued/green-tests.txt)、[manifest.txt](share-split-zero-issued/manifest.txt)。更早构建及 fixture 调整失败日志仍保留在上述 `.tmp` 目录，未重写为成功。

独立复核发现正式规则文案可能让读者误以为 JSON 解码也保留完整 cause，已按实际边界修正：结构化领域错误与直接内存恢复保留 source；`decode_save_slot` 仍以既有通用 `2080/[0]` 拒绝 JSON 解码失败，宿主仍脱敏。未为文案扩大解码器改动。
