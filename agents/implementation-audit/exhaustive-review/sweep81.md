# sweep81：最新main发布结果与当前调用链复核

日期：2026-10-03。产品基线 `08e4fc7`（产品树同 `b76ece3` / `2247f4f`），审计工作树 HEAD 为 merge `a7c7ce3`；`git diff --stat 08e4fc7..HEAD -- packages apps scripts` 无输出，确认产品/工具树与该基线一致。连续全文阅读 `agents/main-release-validation/summary.md` 共47行、`agents/main-release-validation/scripts-isolated-final-results.md` 共40行，合计87行；也连续全文读完旧对照 `agents/implementation-audit/exhaustive-review/sweep51.md` 共62行。遵循根 `AGENTS.md` 与 `docs/principles.md`。本记录只新增审计文件；没有产品修改、Git写操作、测试/完整回归、联网或发布状态查询。

## 最新发布记录适用范围

| 原文位置 | 当前代码/工具证据 | 判断 |
|---|---|---|
| summary:1、43、45：从7198348验收，产品冻结2247f4f，随后发布提交b76ece3；当前08e4fc7产品树相同 | 当前HEAD相对08e4fc7的`packages apps scripts`差异为空。两篇文档分别47、40行，均已全文读取至EOF；旧sweep51记载当时43行的summary，现有版本新增发布信息，不能继续引用旧行数或旧“发布尚未启动”结论 | 验收和发布是文档作者记录的历史事实，产品代码仍对应本基线。这里仅核对原文和当前源码，不重新运行，也不验证GitHub远端状态。 |
| summary:9–15：2199 Rust case、608 Web case；2248 all-feature case；372 scripts case；12 E2E；build/Clippy/lint | 当前源码与工具存在对应入口，但代码不能证明所记的当次退出码、计数、fingerprint或耗时 | 这些是记录中的历史结果，不能改写成当前轮运行结果、彼此累加成唯一case总数，或推论完整游戏已验收。lint仍明确有三个warning。 |
| summary:17–19、scripts结果:3–40：fingerprint、多核配置、300000ms共享期限及32脚本分项结果 | `scripts/run-full-regression.mjs`现行执行器与`docs/testing.md`提供对应阶段、线程预算和长阶段监督；脚本历史表逐项报32文件，总批次16904ms | runner能力与作者记录的那次运行分开表述。日志链接指向本地忽略文件；本轮未读取/重算日志与receipt，不把配置当成CPU实测。 |
| summary:21–27：九项ignored、规模档、10万存档591344527 bytes超过536870912 bytes | 规模验证测试分别走typed JSON/restore或`decode_save_slot`，取决于测试fixture；Server远程restore caller显式先做深度预检，再以`MAX_SAVE_DECODE_BYTES`构造decode限制 | 历史大档结论明确只证明该typed路径恢复；不能宣称Server有界入口能读该591MB档案。Server默认限制超出即ResourceLimit，远程宿主成功加载该大档没有证据。不同fixture的10万账户测试不能混为一项。 |
| summary:39–45：test Release、Actions、35资产核对、Pages；K7与真实性能仍未纳入 | 文档描述具体发布提交、tag、run及其作者下载核对，但本审计未联网；当前发布和验收工具的存在不等同于本轮实测 | 按作者记录引用，范围止于该Release及其声明的制品。Pages只代表静态制品部署；不推出公网完整游戏、三平台GUI、签名公证或真实UI性能通过。普通Rust case deadline缺口仍需按当前runner判断。 |

## scripts结果全文覆盖

`scripts-isolated-final-results.md:1–40`已连续读完：第3行声明Node版本、最多4个文件worker、文件和case各10000ms；第5行记32/32、总耗时16904ms；第9–40行逐项列出32个文件、结果、耗时和本地日志路径。其总批次超过10秒，原文定位为长验收，单文件/单case仍受10秒门禁。表格最慢项为wasm-build-dependencies 8662ms。上述均为原报告结果；本轮未重跑、未读取忽略日志，也不把所有脚本视为root默认或发布自动入口。

## 残余项追至当前caller

### N81-1：普通Rust case仍没有独立10秒硬门禁

当前`executeRustTestBinaries` (`scripts/run-full-regression.mjs:324–352`)为每个预构建binary从完整回归共享期限计算`timeoutMs`，启动时只传`--test-threads=<预算>`与Rayon线程环境变量；没有按普通case设定/执行10000ms watchdog。`runFullRegressionPhase` (`:630–643`)对execute阶段提供300000ms外层child门禁。因此一个普通case即使超过10秒，只要binary和全阶段未耗尽300000ms也可能通过。`docs/testing.md:91,99,120`及根AGENTS的普通测试规则明确要求case不得超过10秒；完整验收共享期限不能替代单case期限。

这是确定的测试工具门禁缺失，不是本轮发现了某个实际超时case；默认/all-feature PASS记录不能核销。长ignored场景另有长测分类，不应与普通case混称。该项与sweep51/sweep78记录的同一缺口去重，不另推成交易实现缺陷。

### N81-2：100k typed恢复结果未证明Server有界解码可加载

当前Server `/restore` caller在`apps/server/src/routes.rs:838`附近对JSON做深度预检，随后构造`SaveDecodeLimits { max_total_bytes: engine::MAX_SAVE_DECODE_BYTES }`并调用`engine::decode_save_slot`；引擎上限在`packages/engine/src/session/persistence.rs:944`定义为512MiB，解码函数在`:963–969`先拒绝超限载荷，再执行serde解码。故summary:27的591344527 bytes历史档案比536870912多54473615 bytes，不能经该Server入口按默认限制成功载入。

规模restore历史用例若直接`serde_json`解成typed `SaveSlot`后恢复，只证明该结构路径可恢复，不证明bounded decoder成功。当前`packages/engine/tests/company_scale.rs:263–275`另有一个100k high-attention、10 tick测试，实际调用`decode_save_slot(...Default::default())`；它是不同fixture，源码显示预期会走有界入口，但本轮未执行、不能替它报告结果。也不据一份历史大档推断此测试fixture的具体字节数。保留当前资源上限，清晰区分已记录的typed恢复与远程加载能力；Web WASM/Desktop各自typed caller也没有这份591MB档案的实测。

### N81-3：K7与性能报告仍需区分工具存在和验收完成

K7现行入口` scripts/simulation/baseline-run.mjs:1379–1392`要求after使用fresh setup与完整seed列表，完成矩阵、finalize并在deadline内发布manifest；`:1397`起的sensitivity也由该runner走共享批次期限、分seed child期限及原子checkpoint。`docs/testing.md:118–120`说明before语料已封存、CLI拒绝重跑，after/sensitivity需fresh fixture、独立构建与各自300000ms deadline。最新summary:43明确额外K7 after/sensitivity矩阵未在该次发布验收范围内；其runner、validator及脚本单测通过不等于本基线已产出/验证fresh矩阵。K7旧artifact比较与跨worker确定性边界仍按既有G39台账追踪，不由这次发布记录核销。

真实UI性能入口当前由`package.json`性能脚本调用`scripts/performance/market-ui-report.mjs`；其脚本测试验证解析、报告和stub行为，不是Chrome运行报告。summary:43也明确真实UI性能报告未纳入验收。已有sweep48的C48-1（工具未驱动现行启动确认）和C48-2（正式入口未接进程外总deadline/清理收敛）是相应的当前caller审查，应按原项去重；不能把32 scripts历史通过或market UI工具单测写成真实浏览器性能通过。

## 结论与范围

最新文档补齐了发布结果，但不会扩大其历史测试与制品的适用范围。当前确认三项需保留的调用链边界：普通Rust case缺单case十秒硬门禁；10万大档typed恢复不等同于Server受限restore；K7及真实UI性能工具未由这次Release验收记录证明fresh矩阵/真实浏览器结果。与sweep51、sweep48、G39及性能审计现有条目合并去重，不另造产品缺陷。没有新的A股交易规则或交易语义实现结论；不宣称产品整体已完成。
