# sweep51：2247f4f 完整验收新增历史记录复核

只读取审计 worktree 内的两份已入库记录；没有引用主工作区中其他贡献者未提交的 summary 更新。只新增本记录，不修改产品/Git、不重跑验收、不查询 GitHub/Release 状态。

## 全文与基线

| 文档 | 全文行数 | 连续阅读范围 |
|---|---:|---|
| `agents/main-release-validation/summary.md` | 43 | 1–43 |
| `agents/main-release-validation/scripts-isolated-final-results.md` | 40 | 1–40 |

合计 **83 行**。两文档由 `b76ece3` 新纳入，所述产品冻结提交是 `2247f4f`。`git diff --stat 2247f4f b76ece3 -- packages apps scripts` 无输出，确认本轮源码与该历史产品一致。当前worktree该目录只有这两份Markdown，没有引用的 `.log`、`.json`、trace或临时runner；不能声称已检查原始运行日志与hash receipts。报告里的PASS、耗时、fingerprint属于该记录作者的历史验收结果，本轮仅核源码/工具与表述边界。

## summary 逐章分类

| 原文行号/主张 | 源码事实与反证 | 分类 |
|---|---|---|
| :3，7198348起始、六修正提交、2247f4f冻结、独立复核 | 现总账已经核对 `7198348..2247f4f`九个产品/测试文件；本轮产品diff为空 | 新历史来源，不新增功能完成项，不重复把用例数量累计到其他批次 |
| :9，默认2199Rust/608Web/74binary/build67.801s/execute72.634s | `scripts/run-full-regression.mjs:328`预构建Rust binary并发、:568必跑长验证、:588doctest、:593Web阶段、:596源码后验 | 历史运行PASS；不能仅因runner存在确认该次case数/耗时，本轮无原日志 |
| :10，all-feature2248普通case+5doctest/78binary/106.131s、ignored分开 | 可选feature产品入口与修正测试存在；server `tests/actor.rs:132`、:139明确按simulation-diagnostics区分结果 | 历史额外验收；不把default/all-feature重复相加，不把ignored跳过说成已覆盖 |
| :11，scripts32文件/372case/16.904s | 当前递归 `scripts/**/*.test.mjs`确为32个文件；逐文件表映射下节。根 `package.json:16` test仍仅run-full-regression，runner未将全部scripts套件变成正式入口 | 新运行证据；既有Q05入口决策仍保留 |
| :12，Chromium12/12/43s/2worker/0retry | 是特定浏览器历史验收，报告明确后续测试修正未改Web生产行为 | 不等于三宿主长期跨日、GUI安装器或全部移动/Wayland体验通过 |
| :13–15，格式/契约/build/Clippy/Web lint | 生产构建与静态检查不同于行为；lint保留三warning的表述诚实 | 历史验证；不称零warning，不以build代替游戏验收 |
| :17，fingerprint与publish-full日志 | runner `run-full-regression.mjs:547`/549先核当前源码与inventory、:596–597执行后再核fingerprint；构建:501也核前后digest | 机制实现；日志/fingerprint具体值未在本worktree独立重核 |
| :19，128CPU/8binary×12harness+4Rayon/跨年127Rayon/Web8分片/300000ms | runner :346显式harness、:347 Rayon，:390共享deadline，:641阶段外监督；docs/testing.md:107–117与之对应 | 多核与长阶段deadline机制已实现；实际CPU/内存采样数值只承接历史报告，不能当本轮测量 |
| :23，九ignored/20k与50k全日/100k并行248.15s、不缩人口300tick | `tests/session.rs:1574`规模helper；:1580严格账户数；:1634逐tick对账；:1705两独立实例并行；:1744/:1752/:1760三档人口ignored定义 | fixture与并行事实可核；“通过/秒数”仅历史报告，不自动推出宿主100k保存加载能力 |
| :25，经历/跨季度/多计划/十年archive/WS probe | 对应源码存在，server WS实测历史环境限制修正未描述为产品修复 | 保存历史验收范围；archive484.7MB不与整个SaveSlot体积混淆 |
| :27，100k档591344527bytes超536870912 | 规模helper :1683–1687采用typed serde_json+GameSession restore；:1710–1715输出decode_limit_fit，不强制该档适合有界入口；`session/persistence.rs:944`512MiB，:963–969显式拒绝；server `routes.rs:838`调用有界decode | **需补总账容量证据限制**；压力typed恢复不能宣称远程宿主可加载。边界已公开承认，不擅自扩大解码限额 |
| :31，去错误跨自由调度事件/PlanId/整档相等，保留seed/立即精确恢复+实际资产/负控 | `tests/session.rs:1603`开局seed字节相等、:1622立即恢复相等、:1628独立实际受理对账、:1635双边Trade/receipt、:1689各实例全字段末尾restore比较 | 与现行ADR0017/0018一致；已有G39 K7旧artifact比较入口没因测试修正自动核销 |
| :33，diagnostic feature/同时间线Growth/causal fixture/1/2/8worker/20日5seed | server actor编译feature分支明确；相关测试文件已在总账九文件diff审查 | 测试契约修正，不新增生产诊断/行情功能完成结论；性能统计全域债不自动消失 |
| :35，cleanup reserve/zombie窗口/debug checkpoint fixture/Clippy等价模式 | 属测试/等价表达修正；普通代表fixture不改变门禁，不用“Zombie不消失”旧环境报告推断现产品失败 | 历史失败及修正记录，不重复登记生产G |
| :37，源码瞬变失败轮/依赖查询超时后原样通过、根因未确证 | runner fingerprint不相等确实fail；测试查询5秒child和10秒整命令未放宽 | 历史失败保留；不把随后PASS叫环境偶发问题根治 |
| :39–41，Release尚未启动/后续merge标签Actions等 | 文档所述时点尚未发布，不是当前线上状态；本轮按任务禁止查询GitHub | 历史待执行；不得引用其他worktree更新扩张结论 |
| :43，K7/真实UI性能/三平台GUI/签名公证未验收；普通Rust case无独立watchdog | 前半为明确排除的未验收范围；后半可由runner具体args确认（见下节） | 长期/GUI债仍保留；**需补普通Rust case十秒监督工程缺口** |

## scripts-isolated-final-results 逐章/逐行覆盖

- :1标题明确批次为长验收；:3规定Node25.8.2、最多4文件worker、每文件进程外10000ms、每case10000ms、文件内concurrency1/isolationnone；:5明确32/32与16904ms。整个32文件批次大于十秒已分类长验收，不能据16.904s说每文件十秒规则被放宽。
- :9–21为build/cache/docs/Web-release/CI/desktop/distribution/frontend/full-regression-web-worker/package/static/Pages，共13文件；:22–30为performance-report/prune/publish/release-policy/full-regression/long-validation/Web-shard/Web-tests/deadline，共9文件；:31–40为diagnostic-divergence/baseline/performance-harness/source-manifest/contracts/prepare/matrix/verify-artifacts/smoke/wasm-dependencies，共10文件。全部32行逐行读取，所列真实脚本测试路径当前均在32文件集合内。
- 所列每文件耗时最高为 `wasm-build-dependencies.test.mjs` :40 的8662ms，仍低于10000ms；报告把真实Cargo查询独立执行避免外部资源抢占，不能解释成取消并行规则。其余31文件四worker的原runner/日志本worktree不在，未独立重建该调度。
- 每行日志链接只证明作者列出了本地文件名；这些ignored日志当前不在审计worktree，不假称已读取、下载或计算其hash。372case总数也只按summary历史陈述引用，逐文件表未列case分布，不能从32行自行推出372。
- 这次历史全脚本运行不能替代正式持续入口；`package.json:16`和`run-full-regression.mjs`没有递归执行这32文件，继续归 **Q05**，不将它们重新加进用户批准的build-only发布链。

## 需补总账的两项边界

### N51-T01：普通Rust case没有十秒独立硬门禁

summary:43已承认事实。`scripts/run-full-regression.mjs:341`为每个Rust binary取长批次剩余deadline，:346仅传 `--test-threads`，:349把该剩余期限交外部runner，没有按case十秒停止机制。一个普通case运行20秒仍可能在不到300秒的binary/全批中通过。`docs/testing.md:91`和根AGENTS明确单case十秒硬上限，:120还明确普通测试不能借长验收放宽。

因此这不是“已经发生某case超时”的实测结论，而是**确定缺少强制case门禁的测试工程实现**。独立监督需要另行设计实现；本轮不授权修改runner，不以完整回归PASS核销。整批外部deadline已在，不应报告完全没有测试超时控制。建议总账单列工具缺口或明确保留工程门禁边界，不与Q05或G39重复。

### N51-B01：100k typed恢复PASS不是远程有界恢复PASS

591344527较512MiB上限多 **54473615 bytes**；数字来自summary:27历史测量，本轮未复现。源码确保超限调用 `decode_save_slot`会返回ResourceLimit；server加载实际使用该入口。压力测试 :1685绕过有界decode，以typed serde正常恢复只证明引擎业务结构可恢复，:1712专门打印fit结果且没放宽限额。

其他宿主不可静默统称同一字节门禁：WASM `apps/web-wasm/src/lib.rs:510`直接serde_json::from_str，:502用serde_wasm_bindgen::from_value；Desktop `apps/desktop/src-tauri/src/lib.rs:230`接typed SaveSlot、:234参数后交actor。此次三宿主没有实际加载591MB大档证据。所需总账补充是“已验证100k引擎typed恢复，未验证大档三宿主可加载，远程默认会被512MiB限制拒绝”，不是擅自要求无限存档或把三宿主都说成经同一bounded decoder。

## 总账来源与完整性

总账首段和 `coverage-index.md:13`旧写“未跟踪agents/main-release-validation不属于2247f4f审计结论”，对2247f4f当时文件状态成立；但两文档现在由b76ece3纳入，应在覆盖集合补 **2个新增路径/83行** 与“仅历史验收来源”分类。原运行日志仍本地ignored、未纳入全文Markdown范围。

本批没有新交易语义代码缺失结论。推荐补充N51-T01工程门禁缺口及N51-B01容量证据限制，保留Q05/G39/K7/UI性能/GUI/签名公证等原分类；不因新增完整回归历史PASS声称当前38个G项已修复。
