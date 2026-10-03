# 完整验证与 test Release 流程检查

检查日期：2026-10-03。已全文阅读 AGENTS.md、principles、testing、build-and-deployment、ADR-0027/0028、所有 CI/Release/分发/七入口 workflows，以及完整回归、Web 分片、进程树 deadline、共享前端和发布 runner。本文是工作记录，不替代正式规范。

## 实际门禁范围

`node scripts/run-full-regression.mjs` 分 build/execute 两个独立 300000ms 阶段。build 编译 Rust workspace 全部 test executables，密封源码指纹、二进制 bytes/SHA-256；execute 校验 inventory 后运行 Rust binaries、一个必跑 ignored year-boundary case、workspace doctests、Web src 全部普通测试，最后重验源码。Web 独立受 10000ms 整批/child/case 门禁。

它不执行 scripts 测试、lint、生产前端构建或浏览器 E2E。scripts 32 文件已单独执行，见 [结果](scripts-unsandboxed-results.md)。其余 ignored scale/stress/cost Rust 用例没有被完整回归自动执行，应报告为额外长验收边界，不能称所有 ignored 已通过；具体计数以当次实际 binary 输出为准。

## 本地 CI 等价命令

固定 Node 24.18.0、pnpm 11.19.0、Rust 1.96.1、WASM nightly-2026-09-05/rust-src/wasm32、wasm-pack 0.13.1、Linux Tauri 系统开发依赖与 Playwright Chromium 必须准备完毕。本机实际 Node 25.8.2 满足 Web runner 下限，但 `.nvmrc` 帮助脚本要求严格 24.18.0；旧 Corepack 0.24.0 不能运行 pnpm 11，不能把它当产品测试失败。

依赖未准备时，先运行共享生产前端构建，再运行 scripts 中需要 Cargo offline graph 的依赖分离测试。下面每条独立执行并记录退出码和日志；命令中的 CPU budget 从 `os.availableParallelism()` 获取并记录，避免多个全 CPU Cargo 批次同时竞争：

```sh
node -e 'console.log({node:process.version,cpu:require("node:os").availableParallelism()})'
cargo fmt --all --check
node scripts/frontend-build.mjs --jobs 128 --package-manager pnpm
node scripts/run-full-regression.mjs build --inventory .tmp/full-regression/oop-release-inventory.json
node scripts/run-full-regression.mjs execute --inventory .tmp/full-regression/oop-release-inventory.json
node scripts/check-generated-types.mjs
node scripts/run-long-validation.mjs 300000 -- cargo clippy --workspace --all-targets --jobs 128 -- -D warnings
pnpm --filter web lint
pnpm --filter web exec playwright install chromium
node scripts/run-long-validation.mjs 300000 -- pnpm --filter web test:e2e
```

128 是本机实际 CPU 数，其他机器需替换。共享前端/build/execute/Clippy/E2E 每阶段均为进程外 300000ms deadline。Windows CI 另有 workspace native libraries 独立 300000ms 预编译，不能把全部冷编译总预算说成五分钟。Root runner 的 build/execute 串行，execute 内 Rust 最多 8 binaries 并行，128 CPU 时每 worker 12 harness + 4 Rayon；Web 最多 8 shards，Playwright 2 workers。必须运行中检查真实线程/CPU 使用率；日志中的最近完成 target 不等于活跃线程。

需要重跑本轮 scripts 短测时可执行 `node agents/oop-release-validation/run-scripts-tests.mjs <批次名>`。完整脚本聚合由外部 300000ms supervisor 覆盖；32 文件分别由 10000ms supervisor 包裹，Node case timeout=10000，最多 4 文件 worker，保留各自 TAP 日志。当前沙箱禁 spawnSync/loopback listen 并使 child pipe 输出为空，已通过工具审查在沙箱外完成实际测试。

## test Release 检查清单

1. 冻结待发提交；说明本地改动与待发 commit 关系。必须以现有授权为准确认 push/发版，不擅自推进远端。有效测试标签是 `test-` 后接非空字母/数字开头、其后仅字母/数字/下划线/连字符；`test-YYYYMMDD-HHMMSS` 可用。标签不自动同步产品版本。
2. 通过 `gh` 触发或查看工作流；普通 commit/PR 不触发 CI。tag push 的唯一自动链为 validate → 双平台 CI → 三平台 distributions → draft 上传及远端资产名称/bytes/SHA-256核验 → 公开 prerelease → 同 run Pages → 缓存清理。手动 All/Web 会更新 Pages，但不创建 Release。
3. 记录 Actions run id、完整 source SHA、tag。在下载/核验时固定 run id；不接受其他 run 的缓存产物。全部十组 distribution manifest 应包含三平台 Desktop、三平台 Server、三平台 WebUI Server、一个静态 Web。
4. 默认分发合计应为 24 个安装包/归档 + 10 份 manifest + `release-source.json`，共 35 Release 资产；逐资产下载/服务器 digest 对照 manifest 的文件名、bytes、SHA-256，并核验 release-source commit 等于 tag commit。检查平台 header、Unix 可执行权限、Mac 内部相对 symlink、CLI 与 webui 相对布局。
5. test Release 必须 `isPrerelease=true`、`isDraft=false`；上传失败可能留 draft，先检查再处理，不覆盖旧资产或移动 tag。只重跑失败任务，不把已通过 CI 重跑作为常规动作。
6. Pages 必须来自该 run 的 site artifact；线上 `build-info.json` 完整 SHA 与发布 SHA 一致。确认 HTTPS/安全上下文、子路径、首次 Service Worker 隔离、SAB、启动前零快速档读取、本地 WASM/Rayon Worker。短 fixture 只证明两股票/七 NPC 启动；不冒充默认两万 NPC 性能、真实 GUI 安装、发行签名、公证、HTTPS/WSS 代理或公网部署验收。
7. 缓存清理核验 ≤10,000,000,000 bytes；成功 tag cache 退休、失败进度保留、活动构建时暂缓。缓存/API失败须单独报告，即使 Release 已公开也不能说整 run 全绿。

## 现有流程缺口

- `scripts/**/*.test.mjs` 没有统一纳入 root full regression/CI。CI 和 distributions 仅分别运行若干白名单，因此相关脚本测试的全部 32 文件通过需要独立证据。正式 docs/test-cleanup-checklist.md 已登记这个问题。
- 已修复并通过独立审查：`publish-release.mjs` 原先只在开始核对 tag commit，上传与远端资产核验后直接公开 draft。现增加公开前第二次 SHA 查询，上传期间 tag 移动则保留 draft 并显式失败；新增失败测试与 15/15 相关绿灯证据见 [TDD 记录](publish-tag-sha-tdd.md)。查询与公开仍是两个请求，不声称原子 tag 锁。
- Release collector 验证十组产品/平台和 manifest 自身文件清单，但每组仅要求 files 非空（31–33 行）。它不独立强制 Windows MSI/NSIS/ZIP、Linux DEB/RPM/AppImage/ZIP、Mac DMG/ZIP/tar.gz 和其他 ZIP/tar.gz 的预期完整组合。当前打包入口确实另有格式检查，故是发布最终门禁的防御缺口；本轮测试 fixture 每组单 ZIP、总21资产也被 collector 接受，不能说 collector 自己强制35资产。
- 文档的直接 `test:e2e` 和 `lint` 示例没有外部 deadline；E2E 应按上方 CI 等价 supervisor 执行。Playwright 本地 `reuseExistingServer: !CI` 可重用端口已有 server，需隔离端口或 CI 模式，避免把其他 preview 当本轮生产/E2E构建。
- 多数 Rust 普通用例只受整个 binary/execute 的长 deadline，没有独立 10 秒 case 门禁；项目规定每 case 10 秒，但现 runner 不提供 Rust case timeout 证明。不能以总阶段在五分钟内声称每 case 满足十秒。
