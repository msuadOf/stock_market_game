# 工具链缺口复核

## 范围与判定方法

- 当前源码基线为 `2247f4f`；在 `b89afb3..8cf34a1` 原核查基础上，已继续逐项检查后续代码和政策差异。本记录只复核旧审计 G21、G26、G27、G39、Q05，及相关 build/CI/script 契约。没有运行全量回归、构建或 K7 矩阵，也没有修改生产代码。
- 完整阅读了 `AGENTS.md`、`docs/principles.md`、`agents/implementation-audit/implementation-audit-2026-10-02.md`、`docs/superpowers/plans/2026-09-24-single-world-multithreading.md` 和 `docs/decisions/0028-tagged-release-and-static-pages.md`。另全文读取更新后的 ADR-0028、testing、actions-cache，并核对量价清单、测试清理清单、脚本与实际调用路径。
- “缺”表示契约问题在当前生产/工具路径仍存在；“部分”表示存在有效守卫但闭环不足；“原误判”表示旧审计错误归因或把本来允许的行为称作错误。自由并发下不同运行顺序不自动等于重放失败。

## 逐项状态

### G21 — CLI 输入投影契约：缺

- 依据：`docs/price-volume-simulation-gap-checklist.md:265-276` 明确说工具只取 `SessionSetup`，忽略的快照、委托及账户状态不参与验证；总账沿用G21编号。
- 当前生产调用链：`packages/engine/examples/price_volume_baseline.rs:29-34` 将整份输入反序列化为 `SaveSlot`，随后才只消费 `slot.setup`。因此无关快照/委托字段无法反序列化时，CLI 在读取 setup 前即拒绝。调用量价报告与因果诊断的后续路径确实只使用 setup（该文件 `:33-38`），但并未落实投影读取。
- 与 `b89afb3` 对照：该 example 在两基线间无相关改动；当前差异未补上投影入口或输入边界测试。
- 判定：仍缺。这里是工具承诺与实际解码边界不一致，不代表 SaveSlot 正常业务加载应放宽验证，也不涉及交易规则。

### G26 — Web lint 将 warning 作为错误：缺

- 依据：`docs/tech-stack.md:23` 写明“CI 以 warning 为错误”；该要求当前适用于独立手动开发CI，发布链路按最新ADR-0028不执行lint。
- 当前调用链：`apps/web/package.json:8-10` 的 lint 命令仅为 `oxlint`；`.github/workflows/ci.yml:219` 直接运行 `pnpm --filter web lint`。工作区 `package.json:20` 虽以 `-D warnings` 严格运行 Rust clippy，但该选项没有传给 oxlint。当前 oxlint 配置中允许 warning 级规则，故 CI 可在 warning 存在时通过。
- 与 `b89afb3` 对照：前端 lint 命令与 CI 调用保持原样，审计基线至今无严格 warning 选项或等效配置。
- 判定：仍缺。Rust warning 门禁有效，不能据此核销 Web lint 约定。

### G27 — Release 公开前复核 tag SHA：已修复

- 依据：现行ADR-0028要求冻结SHA、拒绝标签移动，并在公开Release前核验上传资产；发布不再依赖CI通过。
- 当前链路：`.github/workflows/release.yml` 为 validate → distributions → publish → pages。脚本83行先查询tag，86行上传draft，随后重新读取远端资产并核对名称、大小和SHA-256；`scripts/publish-release.mjs:109–113` 在公开前再次解析tag。与RELEASE_SHA不一致或查询失败时不会执行公开，已有draft保留并显式报错。
- `0e64ae7` 已补上这个此前缺少的守卫。上一基线复核读取正常路径的二次查询顺序断言及“上传draft期间tag移动”行为测试，并在定向短测中执行通过；该用例确认两次SHA查询、已创建draft且未调用edit。本轮对应代码未变且未重跑。
- 判定：核销G27，不再列为未实现。仍然存在两个GitHub请求之间的平台竞争窗口，但原要求是补公开前重查，不是实现平台级原子锁，不能据此继续保留同一缺口。

### G39 — K7 跨 worker 完整产物相等：缺（需求判断不变，工具契约仍过严）

- 计划依据：多线程计划开头明确无关账户/股票无需确定跨运行顺序，允许并发调度产生不同受理先后（`docs/superpowers/plans/2026-09-24-single-world-multithreading.md:3-5`）；同计划生产改造顺序要求账户争用按真实接收顺序、股票按适用价格时间规则协调，而非建立全局交易顺序（`:21-26`）。这允许独立工作并行，并不承诺整局 deterministic replay。
- 当前真实入口：`packages/engine/examples/escrow_verification_harness/committed.rs:514-519` 经 `step_frame_with_commit_evidence()` 运行正式生产 step；脚本 `scripts/simulation/run-escrow-verification-matrix.mjs:598` 调起 harness。生产受理边界 `packages/engine/src/session/pipeline/local_admission.rs:264-309` 将账户资源及股票 gate 依赖纳入局部 DAG，排队次序由真实 gate 到达登记，不构造跨独立股票的全序。
- 当前工具门禁：`run-escrow-verification-matrix.mjs:583-594` 对所有非负控模式保存首个完整 artifact vector，后续必须逐字节相等；vector 包含 state、event stream、receipts、save slot 的 hash。独立 contracts 入口 `scripts/simulation/escrow-verification-contracts.mjs:135-160` 也要求每种 worker budget/repeat 与 `1/0` 的 artifacts 和 execution coverage JSON 相等。矩阵包含 1/2/4/auto worker，故这里不是只验证同一已发生受理事实的重放，而是把跨 worker 的完整交易输出相等当作硬门禁。
- 与 `b89afb3` 对照：完整产物比较和跨预算矩阵仍在；`8cf34a1` 改动了脚本/fixture 的证据组织与校验，但没有移除这项全 artifact 相等要求。`8cf34a1..dddcc31` 对该harness仅作格式调整，仍未固定实际受理轨迹；新增session测试通过先确认实际受理再提交下一单消除错误fixture假设，但没有改变K7门禁。
- 判定：旧审计把“尚未冻结同一受理轨迹”列为工具遗漏，是有效发现；但不能据此推导生产必须跨 worker 产生相同整局事件顺序。当前工具仍把非确定跨账户/股票调度误判为 determinism drift，故契约缺口保留。应验证局部受理事实、资源守恒、价格时间、依赖、失败隔离等不变量；只有输入了完全相同的受理事实时，才把结果相等称为 replay 要求。不能通过删除失败负控或守恒断言来规避。
- `7198348..2247f4f` 新增Session/规模测试已改为各自实际成交对账、存档立即恢复等价，量价报告注释及测试也移除自由调度必然整局一致的错误前提；但本节两个K7比较入口没有变化，重新查看仍要求完整artifact相等。因此不能将别处测试契约修正核销为G39完成。

### Q05 — scripts 测试正式发现与持续覆盖策略：部分已有，入口待定

- 任务目录已有 `agents/oop-release-validation/run-scripts-tests.mjs:17–26` 递归发现scripts下全部test文件，排序后以4个worker并行执行，每文件进程外deadline和case timeout均为10000ms；整个聚合另用300000ms长验收期限并留1000ms清理。不能再写成“没有自动发现入口”。
- `agents/oop-release-validation/scripts-supervised-results.md` 登记历史32/32文件通过、整批13675ms；它是另一任务的长验收记录，不是本轮重新运行的结果。
- 根test与手动CI尚未调用这个任务目录runner，正式开发诊断如何持续维护发现范围与完整性仍需收口。产品构建和发布不跑测试是最新已批准政策，不能要求恢复distributions中的测试步骤来解决Q05；获批仅测试调用的helper也不必接生产。

## build / CI / script 承诺交叉检查

- ADR-0028现已明确仅构建发布：`ci.yml` 删除workflow_call，仅保留workflow_dispatch；Release不调用CI，产品分发删除契约测试、Pages/原生smoke及Playwright浏览器安装。这是接受的政策变化，不登记为退化或缺口。
- 发布仍保留生产TypeScript编译、工具版本校验、十组manifest和远端资产核对；成功只能说明构建、打包、完整性核验与部署结果，不能证明游戏回归通过。
- Release/产品分发缓存清理直接运行受限清理程序；只有手动CI清理先运行缓存契约短测。活动run检查、删除前重读计划与10GB容量核对未因移除测试步骤而取消。
- 手动CI仍保留原长回归build/execute、lint、Clippy和E2E；G26只约束这个独立开发入口。Q05与G39也不能被发布成功核销。
- `scripts/build-targets.mjs` 的BuildRun私有目录、finally清理和ArtifactPublisher拒绝覆盖守卫没有在新增区间改变；本轮没有编译制品或重新运行线上发布。

## 定向验证

上一基线 `7198348` 复核使用Node v25.8.2执行（本轮未重跑）：

```sh
node scripts/run-with-deadline.mjs 10000 -- node --test --test-timeout=10000 --test-concurrency=4 scripts/publish-release.test.mjs scripts/release-policy.test.mjs scripts/distribution-workflow.test.mjs scripts/cache-workflow.test.mjs
```

退出0，4个测试文件通过、0失败，reporter耗时1526.195603ms；文件并发4，每case及整个命令进程树期限均为10秒。没有运行完整回归、K7、构建、浏览器或线上验收。测试使用替代GitHub调用核查脚本分支，不等于真实网络发布。

本轮完整读取 `scripts/simulation/baseline-run.test.mjs` 的新增差异：清理阶段在原剩余期限内等待被终止后代PID消失，替代执行器处理已中止signal，短fixture调整调度余量及对应错误断言。没有放宽正式共享deadline，也没有修改生产baseline执行器或K7矩阵；只核对源码，未将其他任务的运行结果计为本轮通过。
