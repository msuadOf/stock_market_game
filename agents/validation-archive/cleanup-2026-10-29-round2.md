# 2026-10-29 第二批 /data1 旧 ELF 清理摘要

状态：精确4件已独审并实际删除；末尾data1门检查exit1，原失败保留。第一批已删但/data1并发占用导致后验门失败的事实仍保留，不回写为成功。本批只恢复/data1原20GiB准入及合理小预算，不再回收已满足12GiB的/tmp。

所有下列文件均为旧独立 rustc test ELF；保留对应-source.rs、小.log、binding/freeze.json/review.md和当前最新green。删除日期将以真实执行追加；删除后不能再对原raw做字节复核或直接执行，既有SHA只是asof记载；本轮不重新hash大件。

| 精确路径 | allocated bytes | 实际原日志 |
|---|---:|---|
| `/data1/baiyifan/workplace/stock_market_game/agents/branch-completion/b4e-commands/cash-source-chain-red` | 310996992 | test result: FAILED. 8 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s |
| `/data1/baiyifan/workplace/stock_market_game/agents/branch-completion/b4e-commands/correction-contract-red` | 304062464 | test result: FAILED. 3 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s |
| `/data1/baiyifan/workplace/stock_market_game/agents/branch-completion/b4e-commands/correction-contract-green` | 304062464 | test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s |
| `/data1/baiyifan/workplace/stock_market_game/agents/branch-completion/b4e-commands/correction-contract-green-v2` | 304095232 | test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s |

总allocated bytes `1223217152`。全部为同UID普通文件、nlink1，stat冻结在round2-plan.json。

本次所删两件red的原失败为标准source链丢Plan类型和DTO吞epoch；另外两件为当时已通过的旧green版本，后续green-v4继续修复嵌套/完成时间及重复key边界，保留最新版本。仅重述原日志，不改产品或规则。

本批起草期间df回升至21,335,351,296B，距原20GiB只差139,485,184B，因此将原10个候选缩为上表4件。未列出的6候选全部保留，不追大额回收。

## 源与命令、既有SHA

源码与完整build argv保留在 `agents/branch-completion/b4e-commands/` 对应 binding/freeze JSON，小review/contract文档保留其边界。均为 `node scripts/run-with-deadline.mjs 10000 -- rustc --edition=2024 --test <fixture> -C codegen-units=8 -C debuginfo=0 --extern ... -o <ELF>`；test线程8，run argv以原freeze/review和.log为准。原失败保留筛选用例计数，不冒称全suite red。

下面逐个列出已有小凭据；未记录独立Git SHA的旧快照只能按保留source hash绑定，不编造历史commit。

- `correction-contract-freeze-v2.json`：`{"base": "448af08608e65bed8d997be9df3f33a87f226e67", "normal_rlib_source": "6ac167e1d98d1a56b1307822f311d0277d624003", "binary_sha256": "84a71db533cdb2302c4712c8719c909e6dce9cff1bad8c6b37ee6e65657d4d0b", "fixture_sha256": "6cca12fc0b73a662fda369754001a7430f3731e3a4bdaf7665f135303a9edb9a"}`。
- `correction-contract-freeze.json`：`{"base": "448af08608e65bed8d997be9df3f33a87f226e67", "normal_rlib_source": "6ac167e1d98d1a56b1307822f311d0277d624003", "binary_sha256": "262921b36734b2a093f031ef78a13e5bf9b13fe2e35aefec3a6dc14ba4d6bfea", "fixture_sha256": "6cca12fc0b73a662fda369754001a7430f3731e3a4bdaf7665f135303a9edb9a"}`。
- `correction-contract-red-binding.json`：`{"source_sha256": "99c5bdbab3a703b23ef021cf7e722480f50ba52697e30c5b5039beedd4c5661c", "binary_sha256": "a31b588973deb667e62957afd1f65c11c80e4ef39bf9d2e274fbe51783cdb79b"}`。

cash-source-chain-red没有独立binding中的binary SHA，不再补算大hash；其原首红由保留cash-source-chain-red.log、cash-source-chain-red-source.rs及cash-approval-review.md记录，不把缺失历史SHA误写成已核。其余3件binary SHA见上述freeze/binding原件。

## 保护与执行约束

latest correction-contract-green-v4、player-cash-green-v2、cash-approval-green-v2保留；restore current4/4actual seal/sharedtarget及所有源码/node/toolchain/小receipt/三map不在集合。只读核/proc可读exe/cwd/maps/fd无命中；权限拒绝如实记录，不绕过。4个独立删除worker，共享外层300秒deadline；普通只读命令10秒。没有测试、Cargo或Git变更。A股语义、单位、契约和断言不变。

## 第二批实际执行（2026-10-29）

独立Sol审过后4worker真实删除上表4件，总allocated 1,223,217,152B，current8/HEAD/index/front保护通过。正式command末尾实际exit1：data1同时期又受外部占用，实测available 19,391,975,424B仍低原20GiB；tmpquota 13,662,718,976B已过12GiB。原raw现已不存在，不能再复核原字节。真实收件round2-receipt.json保留失败，不冒称交权。

后续只读核查：执行`sync -f /data1`后available增至20,614,029,312B，差值约等于本批释放1.223GB，说明延迟空间记账可解释该次变动。此前“外部占用”是未经证实的推测，不作为确定归因；原command exit1保持不变。该新值仍距原20GiB差860,807,168B。
