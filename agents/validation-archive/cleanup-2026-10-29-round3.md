# 2026-10-29 第三批 /data1 精确旧 ELF 清理

状态：精确4件已独审并实际删除，sync及command均exit0；前两批实际删除的command exit1保留，不回写。第二批后`sync -f /data1`使df增量约等于删除的1.223GB，表明延迟记账可解释该次降幅；不能断言已查明任意外部writer。sync后available 20,614,029,312B，距离原20GiB硬门仍差860,807,168B。本批只删约1.224GB提供小预算，不追21GiB。

| 精确路径 | allocated bytes | 原日志真实结果 |
|---|---:|---|
| `/data1/baiyifan/workplace/stock_market_game/agents/branch-completion/b4e-commands/correction-contract-green-v3` | 304095232 | test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s |
| `/data1/baiyifan/workplace/stock_market_game/agents/branch-completion/b4e-commands/correction-nested-red` | 304066560 | test result: FAILED. 4 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s |
| `/data1/baiyifan/workplace/stock_market_game/agents/branch-completion/b4e-commands/correction-status-red` | 304484352 | test result: FAILED. 5 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s |
| `/data1/baiyifan/workplace/stock_market_game/agents/branch-completion/b4e-commands/player-cash-red` | 311558144 | test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 14 filtered out; finished in 0.01s |

合计allocated `1224204288`B。仅上表4 ELF；correctiongreen-v4、playercashgreen-v2、cashapprovalgreen-v2以及所有小source/log/binding/freeze/review仍保留。红失败分别为嵌套凭证未知字段被吞、completed重复key/完成时间未知字段被吞、T+1锁定正持仓被NoHolding误拒；green-v3为后续v4已替代的5 pass旧快照。

## 原始来源与限制

correctiongreen-v3原base Git SHA `f53e3c9e0e18178f2fd93a25eccf39b576d85f91`，normal rlib source `6ac167e1d98d1a56b1307822f311d0277d624003`，原binary SHA `80c868de480474488afca1b36212925e403a05bec375c6ac4961f4470bb1781c`，见保留correction-contract-freeze-v3.json；playercash-red source SHA `531022b2967bc19e1cc5039c7cb289c61f0919780df233eebc3b9ba7d194f0ae`、fixture SHA `635e54b1cbd8789b20b34da52c4ba33802e30ed16b28c0b245cdf3a12e34e3a8`、原binary SHA `368b14e273c1c73840dbbb542dffe1c67f6b633ec1c3ff72a3fb4b3929a25bbf`见player-cash-red-binding.json。两件correction中间red未在独立binding取得既有binary SHA，不新大hash、不编造commit，保留其-source.rs/.log与correction-contract-2026-10-27.md/review.md小原件。

原完整build/run argv仍见上述freeze/binding/review；build采用 `node scripts/run-with-deadline.mjs 10000 -- rustc --edition=2024 --test <fixture> -C codegen-units=8 -C debuginfo=0 --extern ... -o <ELF>`，run为外层10000ms deadline、test线程8，playercash-red原日志明确仅1目标case失败、14 filtered，不冒称全套失败。

用户授权小Markdown替代不用raw；实际删除日期和退出将追加，删除后旧SHA只是asof收件、无法再对原raw字节复核/直接执行。本轮不重新压缩或迁移大件。4worker删除、共享300秒外层deadline、普通前后check和sync命令各10秒。可读/proc核无引用，权限限制按真实scope记录；不运行测试/Cargo或改产品/源码/规则/单位/API，不改变A股交易语义。

## 第三批实际执行（2026-10-29）

独立gpt-6.1-sol high审过后4worker真实删除4件，sync exit0，session2044亲WAIT chunkbc8a5f **exit0**，wall约1.236秒。原件于2026-10-29已删除。current8 stat、HEAD/index与lastfront完整SHA原样；tmp真实quota available13,662,718,976B，data1 df available21,739,253,760B，分别超过12GiB/20GiB原门。实际receipt为cleanup-2026-10-29-round3-receipt.json；已即时交restore自行preflight并运行下一native19。前两批exit1不回改。
