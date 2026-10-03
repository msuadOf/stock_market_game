# 历史来源补扫 0–2

基线：来源计划 `source_baseline=43b1aa5`；三个来源均取未合入旧分支最后可达版本 `c139a69d2a09220e349912376d4be83d8859795f`，完整读取至 EOF。校验值与行数记录在同目录 `history-01.json`。历史分支记录只证明当时的设计/问题/认知，不自动成为现行需求。

## 来源 0：desktop-build-matrix/decisions.md

原文完整说明当时的桌面构建矩阵设计：Node planner、dry-run 的 `--host`、在 Tauri 项目目录运行 `cargo tauri`、结构化 argv、Linux/macOS 至 Windows 的 NSIS 限制，以及 Corepack 探测修订。文中先记录 `/VERSION`、再明确修订为 POSIX 主机 `-VERSION`，并记载 Windows 固定 `ComSpec` 向量曾从查找任意 `.cmd` 改成 Corepack 命令探测。

## 来源 1：desktop-build-matrix/issues.md

当时记录未遇到实现问题，且未触碰既有工作区修改。它没有提出尚未解决的产品需求。

## 来源 2：desktop-build-matrix/learnings.md

记录 Tauri 项目目录与 Cargo package 名称、WASM 产物准备、Tauri v2 平台包边界及 preflight 的责任边界。该文同样记录 LLVM 工具探测后来改为无参数并以成功 spawn 判断可用。

## 按当前决策与调用链复核

现行 ADR-0027/0028 已接受构建目标与原生平台优先、有限实验性交叉构建、独立发布/手动入口及发布只构建打包核验部署的边界；它们优先于旧 notepad。当前 `scripts/desktop/build-matrix.mjs` 的 `planCell`、`route` 与 preflight 消费这些规划，调用 `cargo tauri build`，并明确标注受限 NSIS 路径。当前 Corepack、NSIS 和 LLVM 探测实现已经演进（例如 Corepack 探测 `corepack pnpm --version`、NSIS 使用 `/CMDHELP`）；旧文里的命令细节不是现行契约。前端 WASM 生成物由既有 WASM 脚本消费，桌面矩阵只校验产物并报告准备方式。

这些记录既未显示当前调用者/消费者存在违反较新 ADR 的可确认缺口，也未形成新候选。旧跨平台矩阵具体实现方案和探测参数视为已被后续实现及决策取代；本次没有把旧实验性交叉构建记录扩展为新产品要求。resolution 已将历史 `bare pnpm` / `COREPACK_BIN` 建议裁为无现行必失败证据的维护/文漂观察；本组三篇未发现可与 G 或 Q 合并的新事实。无需新增 G/Q，也无候选需要复核合并。

本次仅静态审阅，没有改游戏代码或运行测试。
