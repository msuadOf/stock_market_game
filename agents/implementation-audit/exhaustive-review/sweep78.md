# sweep78：发布前验证与发布流程工作记录复核

源码为`b76ece3`（worktree审计merge的产品树相同）。三篇连续全文读取：`agents/oop-release-validation/release-process-checklist.md`1–50（50行）、`review.md`1–153（153行）、`scripts-final-results.md`1–40（40行），共243行。review因首轮输出截断又分段补读至全部153行；32条脚本测试表格逐项读取。仅新增本审计文件，无产品修改、Git写操作、发布操作或测试执行。

## 发布流程检查清单逐条映射

| 原文锚点 | 当前代码/最新决定 | 状态 |
| --- | --- | --- |
| checklist:7：密封Rust build/execute各300000ms、二进制指纹、ignored跨年、doctest/Web | `scripts/run-full-regression.mjs:63`build、`:328`并行binary、`:368`必跑ignored、`:587`doctest、`:592`Web十秒；源码/inventory指纹逻辑仍存在 | 手动完整验收主体已实现；不是发布前自动运行要求。 |
| :9：root不跑scripts/lint/生产build/E2E，ignored不全执行 | root`package.json`test仍该runner；CI只白名单脚本测试；`run-full-regression.mjs:19`仅指定一个ignored长例 | Q05正式持续脚本入口仍待收口；其余ignored需独立验收，不伪称全部通过。 |
| :13–15：固定工具/环境、共享生产build、CPU参数记录 | 根packageManager pnpm11.19.0、`.nvmrc`、`rust-toolchain.toml`、`.github/workflows/ci.yml:25`附近工具setup；`scripts/frontend-build.mjs`独立build；runner`:300`CPU预算 | 工具版本/环境失败不是产品缺函数；本轮未启动安装或build。 |
| :17–30：fmt/build/execute/types/clippy/lint/E2E分别验证、128核换算、外部deadline | 正式手动CI仍有独立长supervisor；`:328`最多多binary并发，`:346`harness预算、`:347`Rayon预算；生产Release不调用这些验收 | command清单是本地等价验收，不是当前发布自动要求。Rust单case门禁另列S78-02，整批五分钟不能证明case十秒。 |
| :32：scripts32文件四worker、每文件/case十秒、聚合外部五分钟 | `agents/oop-release-validation/run-scripts-tests.mjs:15`递归discovery、`:50`四worker、`:54`输出长验收说明；文件末尾runBoundedCommand外部监督 | R3后已补共享监督，旧scripts-final标题“普通测试”及14234ms是历史结果，不重开为当前脚本聚合缺口。 |
| :36：test标签合法、freeze提交、gh、发版确认、版本不自动改 | `scripts/release-policy.mjs`parseReleaseTag；`release.yml:26`实际调用，`:61–62`固定tag/SHA；发布器统一invoke gh | 已实现标签/源码契约；本轮没有外部操作授权也未发布。 |
| :37：旧tag→双平台CI→分发→draft验证→prerelease→Pages→缓存 | 最新ADR-0028:15–18明确build-only、不CI/测试/lint/smoke；当前`release.yml:32–42`validate→distributions→publish，`:65`Pages | **旧双平台CI段已被覆盖**；普通commit/PR不自动、手动产品不跑回归均获批，不能作代码遗漏。 |
| :38–39：当前run十组manifest、35资产/包布局/哈希 | `publish-release.mjs:16`十组、`:34`身份去重、`:37`实际文件与manifest对照、`:42`大小/哈希；`release.yml:53`download当前run；打包器`package-distributions.mjs:196`预期native格式 | 正常打包链有完整格式校验，collector独立门禁仍只验证非空清单（S78-01）；本轮未下载或验线上资产。 |
| :40：test prerelease非draft，失败留draft，不覆盖旧asset/move tag | `publish-release.mjs:86`create draft、`:88`prerelease、`:89–106`draft身份/远端asset，`:113`最终公开 | 已实现，不以历史本地签认可发布成功。 |
| :41：Pages同run site/full SHA、安全隔离、本地WASM、短fixture限制 | `release.yml:66`needs publish、`:72–74`reuse-site；`build-web.yml:16/26`reuse路径；正式ADR-0028:44–46限定SW同源隔离 | 已有构建/隔离生产入口；默认两万NPC性能、GUI安装、签名、公网/WSS不是短fixture证明。 |
| :42：cache≤10000000000、成功tag退休、失败进度/活动构建保留、API错误报告 | `scripts/prune-actions-cache.mjs:7`预算、`:12`active statuses、`:89`成功本run tag门禁、`:132`循环重读/重验；`release.yml:94–103`按publish成功选择retire-tag | 当前生产已接清理安全路径；发布workflow直接清理不预跑契约测试是最新决定，不新增“清理前应测”项。 |
| :46：scripts未进root/CI全量 | runner存在但root/手动CI未整体消费 | 既有Q05，不重复G项；后续发布决定进一步明确产品build-only。 |
| :47：G27公开前第二SHA核对已修 | `publish-release.mjs:109`第二api→`:110`比较→`:113`edit；tag错误保留draft，API错误直接传播 | 已核销，**不重开G27**；两请求不是原子tag锁，原文已诚实限定极短竞态窗口。 |
| :48：collector只要求每组files非空 | `publish-release.mjs:33`files.length>0，`:38`expected由manifest自己推导；`publish-release.test.mjs:33`fixture接受21资产 | 当前残余候选S78-01；不是声称正常Release已经缺包，upstream打包器格式guard构成必须保留的反证。 |
| :49：直接lint/E2E例子无外部deadline、reuse preview风险 | 手动CI E2E`ci.yml:227`已有长supervisor；真实fresh最终日志在review:122；本地Playwright仍可按配置reuse | 直接例子/平台验收执行纪律，不能按历史复用preview判最终fresh仍失败。 |
| :50：Rust普通case未独立十秒 | 当前runner`:341`timeout取长阶段剩余，`:346`只test-threads，未按case监督 | S78-02残余当前代码，区分长ignored case与普通case。 |

## Review全部章节及有效发现反查

| 原文 | 当前代码/证据范围 | 判断 |
| --- | --- | --- |
| review:5–28：十四文件范围、日志签认、用户AGENTS与旧截图不混入 | 本轮只读十四文件相关caller；sha256sum发布器=`02bb3440…194c`、CompanyPanel=`e42d0762…58b2`与:147/:149一致 | 不凭历史许可做本轮Git提交或发版，也不宣称十四hash全量重新核对。 |
| :30–36：A股单位/制度无改、日终fixture、stub不证明Rust恢复 | `apps/web/src/app/useSaveCommands.ts:70`validateDayEndArchive；`session/persistence.rs:258`独立深验 | 日终存档及信息边界保持；公开save能力不因内部检查点测试重新变成日内保存。 |
| :38–49：lint等价/Arc捕获对象/ignore范围 | `pipeline/decision_snapshot_capture.rs:273`CapturedExperienceObservation、`:367`观察后Arc、`:194`new_shared、`:231`并行装回；`.gitignore`工作树/Agent JSON边界 | 实际captured与装回生产caller存在；没有按Arc类型变化认定经历算法已修G08，旧深复制也不自动核销G16。 |
| :51–61：异步错误/cleanup/重试、R1 indexOf假阳性修复 | `app/session-host-lifecycle.test.ts:144`load-retried次数为1、`:145`register次数、`:146`顺序；生产SessionReplacementGate/InitialSaveSource | R1已修，缺load不会以-1排序虚过。旧晚到host/baseline问题还须按实际G04边界判断，不把这组测试扩大为全部重连正确。 |
| :63–77：两符号价格fixture加强实际受理顺序、对照反向拒单 | `packages/engine/tests/session.rs:4883`先实际Sell Accepted，再Highest；`:4918`反向先Highest、后Sell笼子拒绝 | 保留原价/量/冻结恢复断言，符合当前并发实际受理事实契约，不要求回退自由enqueue全局顺序；G39仍不同层验收缺口。 |
| :79–87：secondSHA/保留draft/成功顺序/非原子限制 | `publish-release.mjs:109–113`和对应测试`publish-release.test.mjs:149`附近 | G27已真实生产接线且测试保护，不新增tag锁实现任务。 |
| :89–104：CompanyPanel选择/loading/error/empty、披露边界、R2 scope fixture修复 | `components/company/CompanyPanel.tsx:60`只ready/empty协调、`:86`只ready渲染；`company-report-selection.test.ts:90`scope与C-002156一致 | 旧报告刷新选择丢失已修；没有用公共查询读取未披露事实。R2已修，不能只按旧初审fixture判缺陷。 |
| :105–107：R3共享deadline | 工作runner递归discovery受末尾五分钟外部监督，文件child/case十秒不放宽 | 已关闭；监督批次13675ms须称长验收，历史14234ms数据不覆写。 |
| :109–128：工作中文/日志忽略、复用preview/exit2历史、最终fresh/full/production补证、本地不等远端 | review:121–124明确Rust2193/Web608、freshE2E12及production和supervised脚本补证；本轮不运行或读取被忽略全部日志 | 历史失败被后续明确核销，只有可定位历史汇总证据，不称本轮全仓绿/线上已发。 |
| :130–153：历史保留限制与各文件hash | 当前两抽检hash对应已审版；其他hash/性能/三宿主及ignored长例没有本轮补验 | 不将早期“仍有失败”段落忽略最终补录，也不把最终补录证明范围扩为所有extra gates。 |

## Scripts 32行结果的覆盖与时限解释

scripts-final-results:9–40列出32文件：构建目标/前端依赖、workflow/缓存、desktop matrix/平台打包/static web、Pages隔离、发布policy/publish、root/web/shard/deadline runners、性能报告、simulation基线/诊断/manifest/escrow矩阵、smoke等全部表格行已读。这是当时独立脚本验收清单，当前root的生产入口及手动CI白名单不能据该表声称持续自动覆盖全部32文件（Q05）。:3每文件/每case十秒与:5聚合14234ms不同范围，review:107之后明确长验收并补共享五分钟supervisor；原历史结果不算当前未修代码。

## 两项残余候选与反证

- **S78-01，最终collector预期格式组合校验不足。** 原文checklist:48直接登记。生产发布器当前只确认十个产品/平台身份、每组非空且manifest自报的文件完整。正式ADR-0027固定三平台包格式，ADR-0028:19–20要求缺失制品拒绝；upstream`package-distributions.mjs:196–210`确实检查native格式，这证明正常构建有前置保护，却不证明最终collector独立强制各平台完整组合或35资产。当前`publish-release.test.mjs:33`21资产fixture反证该独立门禁尚未实现。建议按产品/平台最低格式契约验最终manifest，避免仅硬编码总数；未运行复现，更未声称已发布资产不足。
- **S78-02，Rust普通case缺独立十秒监督。** 原文checklist:50、正式testing:91及AGENTS规定单case硬上限；当前`run-full-regression.mjs:341–350`逐binary长阶段剩余deadline、只传`--test-threads`。长批次五分钟允许多case累计执行，不等于允许任一普通case占满阶段；必跑ignored长case已单独分类，此项只针对普通case。建议有实际个例超时终止/报告及短负控制，保留并行与共享长批次deadline。本轮未运行耗时case，不断言当前正常case实际超时。

上述候选须与其他sweep台账去重，不重新编号旧G27。已知Q05保留；普通commit/PR不自动、发布build-only/无smoke/不签名属于最新决定，均不作为新增遗漏。
