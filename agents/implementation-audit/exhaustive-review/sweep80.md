# Sweep80：OOP release-validation summary 全文审计

## 范围

已连续全文阅读 `agents/oop-release-validation/summary.md` 的41行（含两个发布轮次、全部日志限定与末尾未验收边界）。基线 `4ad5a2e`，产品树按父任务说明等同b76ece3。仅核对当前源码和正式命令caller；没有本轮测试、浏览器、Git写操作、网络发布或远端Release复查。原文历史PASS/资产摘要不能当作本轮重跑或在线状态保证。

## 条款矩阵

| 原文行号 / 条款族 | 当前状态与实际代码证据 |
|---|---|
| :3、:7 `.gitignore`、worktree、Agent JSON及不带入他人工作 | 当前 `.gitignore:105`/`:106` 忽略两类根worktree，`:129` 为 `/agents/**/*.json`，`:126`保留旧审计目录忽略；Markdown审计仍可跟踪。具体当时提交归属是历史Git流程，不能依据摘要再执行暂存/提交。 |
| :9 五个宿主/存档回归与次数断言返修 | 对应生命周期与统一request gate已有当前真实caller：`apps/web/src/App.tsx:287`生命周期、`:352`save facade；`app/useSessionHostLifecycle.ts:102`处置异步创建后取消。历史五case及review通过不是新产品能力承诺，本轮不重复宣称运行通过。 |
| :11 Rust lint/format及CapturedExperienceObservation Arc最终消费者 | 当前 `packages/engine/src/session/pipeline/decision_snapshot_capture.rs:273`定义enum，`:276`/`:285`持有Arc，`:171`真实capture调用。观察更新仍在候选中，与权威提交分离；不能把这段局部所有权改善核销全历史PlanBook复制G16。 |
| :13 符号价格fixture按实际受理、反序负控、没有生产撮合排序修改 | 当前 `packages/engine/tests/session.rs:4918`明确先受理Highest再Sell的相反顺序场景；旧入队次序并不等于同股入口实际受理。规则/固定挂单价/保存恢复仍沿现行pipeline，不能从fixture返修改出全局编号撮合优先。 |
| :15 CompanyPanel loading暂空列表不清选择 | 已实现真实组件caller。`apps/web/src/components/company/CompanyPanel.tsx:60`只在ready/empty协调选择，`:82`保留loading展示，`:88`真实DisclosureList。此为报告刷新选择稳定性，不核销财报内容/中期G09、合并G28等领域缺口。 |
| :17 上传draft后再次核tag SHA、API失败保留draft、非原子窗口 | 已实现。`scripts/publish-release.mjs:83`初检，`:86`创建draft，`:93`取已上传release对象，`:105`核资产大小/远端digest，`:109`公开前再次查询tag SHA，`:113`才edit --draft=false；真实workflow `.github/workflows/release.yml:63`调用脚本。`publish-release.test.mjs:149`含上传期间tag移动负例。两次API调用不构成锁，不新增声称消除竞态的能力。 |
| :21 正式build/execute各独立5分钟外部期限、多核参数、必跑跨年/Node分片 | 当前 `scripts/run-full-regression.mjs:557`建立binary预算、`:562`记录；`:564`执行Rustbinary，`:568`执行required长验证，`:588`doctest，`:593`Web。实际并发随cpuCount计算，不将摘要128CPU/74binary固定成永久机器/文件数量门槛；两阶段分别长验收期限符合原记录，未跑本轮。 |
| :23 pnpm缓存同版本与环境失败记录 | 属历史环境证据。当前workflow锁Node24.18.0，仓库固定pnpm；本轮没启动Corepack。spawnSync/loopback管道故障不视为已修产品，也不使用失败转成功措辞。 |
| :25 32 scripts/370case、单文件10s/批次300s、Rust/WASM/Server部署与lint warning | 历史短文件/长聚合验收证据。当前正式全回归没有把script所有测试纳入根发现的持续维护决定，沿Q05；`apps/web/package.json:9`仍裸oxlint，手动CIwarning不失败归G26。不因历史lint exit0宣称零warning，不要求build-only release恢复lint门禁。 |
| :27 最终2193Rust/608Web/fresh浏览器12、ignored不计、tsc返修与截图保留 | 历史产物/日志范围；当前runner存在不能独立重证case总数/耗时或浏览器结果。本轮无trace读取/执行。旧preview重用明确排除，尊重独立端口、零重试、fresh build证据限定。 |
| :29 原始log/JSON/trace本地忽略，远端以Actions/Release provenance核验 | 当前 `.gitignore`与源码发布manifest/provenance路径符合该边界；`publish-release.mjs:69`写release-source.json，`:30`/`:105`验证来源与资产digest。摘要链接不保证本机原日志或当前网络状态存在，不新增“必须把本地日志提交”任务。 |
| :33 首轮Windows五秒查询超时、无公开Release、人类取消重跑 | 已退出历史批次。未恢复取消的任务、未改五秒断言，不把Ubuntu成功覆盖Windows失败。该首轮结果不能作为后一轮发布通过证据。 |
| :35 用户改build-only release、保留手动CI、十组manifest→prerelease→Pages | 当前真实caller一致：`.github/workflows/release.yml:32` distributions调用，`:41` publish，`:65` Pages且reuse-site=true，`:76`统一prune；CI仅workflow_dispatch（`ci.yml:7`）。release没有uses ci/测试/smoke，符合最新决定，不能按旧验收计划重新登记“缺CI”。 |
| :37 第二轮发布成功、35资产大小/hash、中文说明 | 属特定d97ac393/test标签历史发布事实。本轮只确认脚本有manifest/remote digest/provenance机制，没有网络再次下载或查gh，不声称现在远端资产仍匹配。脚本默认英文notes与那次人工改中文release说明可以同时成立；不能据此制造实现矛盾。 |
| :39 Pages artifact复用与内部job跳过、末尾prune、未线上smoke | workflow按reuse-site复用站点并统一清cache，跳过内部重复build/cleanup不是漏接。部署success与公网游戏完整验收分开；无线上smoke是当次明确未做范围，不列当前发布漏功能。 |
| :41 K7/scale/stress/cost/GUI/签名公证未验；Rust普通case无独立10秒watchdog | 前半为明确未验收/未来范围。后半是真实现行工程缺口：`run-full-regression.mjs:341`给binary共享长deadline，`:346`仅--test-threads，`:349`用binary timeout；没有case独立10秒监督。已由 `exhaustive-review/sweep51.md` 的N51-T01登记，本轮仅补来源并核实，不重复新增候选。 |

## 反证与残余归并

- 发布成功记录没有承诺当前全量代码已穷尽功能，也没有承诺所列K7/GUI/签名测试通过。不能从历史PASS跳过总账G项。
- 生产tag SHA二次核验有workflow真实caller，CompanyPanel刷新选择修复有真实UIcaller，CapturedExperienceObservation Arc不是闲置测试helper；本轮没有发现它们承诺而未接线。
- build-only policy是用户后续决定，手动CI与发布互相独立；被删除的release测试/smoke门禁不能重开成遗漏。
- Rust普通case十秒门禁仍缺；这是工程监督代码缺口，不等于旧完整回归未执行。与已有N51-T01合并。G26 warning失败门槛及Q05 scripts持续入口仍不由历史32文件一次验收核销。

结论：41行已全文核对，无新增独立产品缺口；补强已有N51-T01、G26、Q05的证据与发布政策边界。未将任何历史通过结果写作本轮重跑。
