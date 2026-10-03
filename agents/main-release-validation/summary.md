# main 完整验收与 test Release

本轮从合并后的 `main` 提交 `7198348bdd9c0a57b14dc3e1da836de119031f48` 开始，执行默认回归、可选 feature 回归、脚本、浏览器及全部九项 ignored 验收。必要修正在 `fix/main-release-validation` 保存为六个小提交，当前实现冻结于 `2247f4f`；每批修正均由未实施的 subagent 独立复核。发布沿用现行 build-only 链路，不恢复标签 CI。

## 最终测试结果

| 验证 | 实际结果 | 执行证据与边界 |
|---|---|---|
| 默认完整回归 | 2199 个 Rust case、608 个 Web case 通过 | 包含 74 个 Rust binary、必跑跨年、doctests，以及 Web 123 文件；最终发布源码 build 67.801 秒、execute 72.634 秒，退出成功 |
| 完整 all-feature 回归 | 2248 个普通 case、5 个 doctests 通过 | 78 个 binary；106.131 秒；9 个 ignored 在此入口跳过，另有逐项实测，不与默认回归重复相加 |
| scripts 全部普通测试 | 32/32 文件、372/372 case 通过 | 16.904 秒；真实 Cargo 查询独立执行，其余 31 文件四个 worker 并行；见 [逐文件结果](scripts-isolated-final-results.md) |
| Chromium E2E | 12/12 通过 | 43.0 秒；CI=1、独立端口 4193、两个 worker、零重试；Web 生产行为在后续测试修正中未改变 |
| 格式、类型与构建 | 格式、生成 TypeScript 契约、生产 WASM/前端构建通过 | 原始日志在本目录；不以构建代替游戏行为验收 |
| Rust 静态检查 | workspace、all-features、all-targets Clippy 通过 | 32 Cargo jobs，约 73 秒，`-D warnings` |
| Web lint | 命令退出成功 | 保留三个既有 `no-children-prop` warning，不称零 warning |

最终 all-feature 的源码 fingerprint 为 `b46ae03db571f33bf0567550fe05453031af866c56f179a892aa624e7edfa0cf`，测试前后及 tracked diff SHA 一致。默认最终发布回归的源码 fingerprint 与上述值完全一致，执行前后密封校验通过，日志为 `publish-full-build.log`、`publish-full-execute.log`。

机器可用 CPU 为 128。默认回归最多八个 Rust binary worker，每个 12 harness／4 Rayon threads；必跑跨年为一个 harness／127 Rayon，Web 八分片。生产 WASM 与 Clippy 使用 32 Cargo jobs，浏览器两个 worker，脚本最多四个文件 worker。长验收受进程外 300000ms deadline 监督；脚本每文件及 case 均为 10000ms。运行中核对实际 CPU、线程和内存，不以配置线程数代替资源证据。

## ignored 与容量验收

全部九项 ignored 均有实际通过证据：默认回归单独执行跨年，其余八项另跑 release 模式。2 万与 5 万账户完整日分别通过，耗时 68.48 秒、240.67 秒；10 万首次触及五分钟 deadline，测试层改为两独立实例并行及更严格的 JSON 字节比较后，通过，耗时 248.15 秒。人口、300 tick、成交和资产断言、五分钟门禁均未缩减。

其他通过项为 2 万经历成本、2 万跨季度（40.62 秒）、10 万多计划存档、四行业十年归档（104.76 秒，约 484.7 MB archive）和 WS push/pull 探针。WS 首次受 sandbox 本地监听限制，改在正常进程环境下原样验证通过；不将环境失败记为产品通过。

10 万完整日的最大存档为 **591,344,527 bytes**，超过宿主解码上限 **536,870,912 bytes**。typed JSON／`GameSession::restore` 在压力用例中通过，不能被解释为宿主有界解码入口能够加载这个档案。该容量边界没有通过放宽限额掩盖。

## 必要修正与失败记录

规模恢复用例原来比较两个自由调度实例的未来事件、PlanId 和整档字节，与 ADR-0017、ADR-0018 及 `docs/testing.md` 的现行实际受理契约不符。保留初始 seed 确定性和精确恢复，两个实例分别对账真实 Trade／双边 Fill、实收费用和现金、逐股股份、日 K、完整 DayBoundary 与末尾全字段恢复，并加入现金、股份、成交数量及缺少日界的负控制。跨季度保留 `npc_attention`、`retail_experience`、`public_library` 对照，新增中途和两个末尾 checkpoint 的精确字节恢复、计划引用与历史报告留存。首次四项失败及 10 万超时记录均保留。

诊断 feature 验收发现 actor 无条件预期 Unsupported；修正为按编译 feature 区分，debug／release 诊断分支及默认编译均验证。诊断查询改用实际会观察的 Growth 机构，在同一已提交时间线比较查询前后存档和 trace，避免低频 DeepValue 的空样本及无效跨未来比较。causal fixture 使用既有公司发行股份和 ST 类别、全部 retail 风格及三个真实交易日，保留真实成交、来源、守恒和篡改拒绝检查，在 1／2／8 Rayon workers 下通过。价格成交量基线保留 20 日、5 seed，删除仅用于错误跨运行相等的冗余长运行，全部有效行为和统计断言保留；公开文档同步说明实际调度契约。

shared deadline fixture 的初始真实 I/O 清理预算过小；另有 SIGKILL 后 Zombie 尚未被 OS reap 的短窗口。测试在原 cleanup reserve 内等待 PID 消失，仍检查 ESRCH、staged 清理和总期限；代表性批次采用 500ms／1000ms，生产十秒／五分钟门禁未改变。新增 checkpoint 普通用例的 debug exact 曾触及十秒期限，仅缩为单股、两个 tick，恢复字节和未知账户／股票引用负控制保留，最终 exact 通过十秒门禁。可选诊断模块的 Clippy 修正只合并 `Execution.side: Some(direction)` 模式，方向链、集合竞价无 aggressor 的忽略、pairs 和 empty None 不变。

首轮默认回归的用例虽全部通过，但末尾源码 fingerprint 瞬时变化，整轮退出失败；该轮未记为完整通过，后续封存和执行避免并行生成绑定。nightly 依赖图查询曾触及五秒 child 和十秒整命令期限；宿主 trace 只定位一次 compiler probe 周围 4.51 秒，未确认偶发慢点根因。随后独立原样三项通过（0.751 秒），最终完整脚本批次也通过。未修改依赖测试、参数、断言或时限，不宣称其环境时序问题已被根治。

## 发布与适用范围

本轮修正与验收记录已 fast-forward 合入 `main` 并同步，发布源码为 `b76ece39b3a1635adde52da07375607f19b56ecc`。新标签为 `test-20261003-075118`，对应 [GitHub Actions run 37108236778](https://github.com/msuadOf/stock_market_game/actions/runs/37108236778)。原子非强制同步 main 与标签时，GitHub 使用当前账户权限跳过了要求 Pull Request 的分支规则；未强制推送、改写历史或移动旧标签。

[test Release 已公开](https://github.com/msuadOf/stock_market_game/releases/tag/test-20261003-075118)，`prerelease=true`、`draft=false`，完整 Actions run 最终为 `completed / success`。共享生产前端及三平台 Desktop、Server、WebUI Server 构建全部通过，publish、Pages deploy 和末尾 prune-caches 均成功；`pages / build` 因复用本轮 site artifact 跳过，分发内部清理由发布末尾统一执行，均属于设计内的 skipped。发布链路未调用 CI 或 smoke。

下载十组 manifest 和 `release-source.json` 后逐项核对：来源 SHA 为上述发布提交，35 个资产的名称集合、大小和 GitHub 远端 SHA-256 与 manifest 完全一致，下载的 11 份 JSON 自身大小和摘要也一致。Release 说明已更新为中文。Pages 成功只表示本轮静态制品部署，不等于公网完整游戏验收。后续仅更新本记录，不移动已发布标签或覆盖资产。

原始日志、JSON、trace 和临时 runner 按忽略规则仅保留本地，不假称都可从 GitHub 下载。额外 K7 after/sensitivity 矩阵、真实 UI 性能报告、三平台 GUI 安装、签名、公证不属于本轮自动化测试与发行范围，未宣称通过。现有 Rust runner 没有每个普通 case 独立十秒 watchdog，批次通过不能证明逐 case 十秒上限。金额分、股数、T+1、实际受理顺序和日终存档语义保持不变。
