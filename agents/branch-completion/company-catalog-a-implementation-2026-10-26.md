# Company catalog A 实施记录

日期：2026-10-26。范围：仅共同 engine 的公开公司身份 DTO 与纯查询；HostUpdate、UI、parser、账户、library 和报告来源不在本片。

## 冻结接口与来源

- 基础提交：真实 `refs/heads/main` 的 `a29d9be40a7bf59f5c8bdb9563ed70e8e3bed143`；独占 worktree `.worktree/company-catalog-a`，分支 `feat/engine-company-catalog-a`。
- `CompanyCatalogEntry { id: CompanyId, code: String, name: String, listed_stock: Option<StockId> }`：只派生 Serialize 与 TS，不创建持久化副本。
- `GameSession::company_catalog_view() -> Result<Vec<CompanyCatalogEntry>, CompanyCatalogViewError>`；专用 `UnknownListedStock { company: CompanyId }` 错误只携数值身份，不占新全局 fault 码；后续宿主 wire 映射另审。
- Some(Simple) 读取当前 `CompanySystem.issuers()`；None 读取当前 `company_registry.iter()` 的实际 spec。保留原 map key，不按静态表、股票数组或输出下标分配身份。
- 上市映射通过当前 `stock_catalog().id()`；未知映射显式失败，真正非上市实体才输出 null。Simulation 当前创建入口拒绝，本片不另增 fallback。
- 此接口经未实施者 sol61_review_exempt 预审确认。设计依据：根工作区 `company-catalog-bridge-design-2026-10-25.md` 与 `company-catalog-design-review-2026-10-26.md` 最后一节。

## 领域与资源边界

遵循 ADR-0041 的 CompanyId／StockId 独立数值命名空间与实际目录翻译，以及 ADR-0035 的 selected 唯一来源。本片只公开已有身份事实，不改变沪深撮合、费用、T+1、证券类别或股东规则，不新增现实交易制度假设。A 目录存在不代表旧 library 已成为 Simple 报告权威源，也不代表图表到公司的完整链已修复。

构建 target：root 明确协调后，从无 cargo/rustc 在途的 `.tmp/company-read-views-target` 用 `cp -a --reflink=always` 创建独占 `.tmp/company-catalog-a/target`；btrfs COW 成功，无 full-copy fallback，无硬链接。原/新 engine rlib inode 为 31340410／31520883；原 `libengine-b7714b84cb9f18d0.rlib` SHA-256 为 `47ea929479bc92998608984d04bc9f3dcc9458804248f19c22267cd60307235b`。最终已实际重编当前源码，green compiler artifact 的 fresh 均为 false；结束后复核原 target rlib SHA 完全相同。

## TDD 与验收

先新增 `packages/engine/tests/company_catalog_view.rs`，覆盖实际单股、非默认子集、反序自定义配置含非上市实体、Unicode／`__proto__`／`constructor`、selected 来源、save 纯读与 restore 以及未知／重复映射拒绝。随后补齐 DTO／query 可编译骨架，暂只返回空 Vec，等待真实断言红；不把缺 API 编译失败充作业务红。模块内另有两种 selected 来源的局部 StockCatalog 映射破坏测试，要求明确返回专用错误，不引公开测试旋钮。

原件统一在 [`company-catalog-a/`](company-catalog-a/)，构建 JSON 原文无损 gzip；`build-artifacts.json` 提取实际 executable 与 fresh 字段，`evidence-manifest.json` 绑定原件 SHA。源码清单含 fixture、Cargo.lock 与生成 DTO；构建、执行之后核对未漂移。

- 第一轮红 build：`cargo test -p engine --lib --test company_catalog_view --no-run --offline -j 8 --message-format=json`，Cargo 报 1m10s。内部未知映射 case 真实失败：`Ok([])` 不等于 `Err(UnknownListedStock { company: CompanyId(0) })`，0.19s。
- 第一轮 integration 在身份四例已红、非法配置已通过后，纯读／restore 大 fixture 导致整命令触达 10000ms，被外部 supervisor 杀止。完整失败原件保留为 `red-large-fixture-integration.*`，不能算完整短红套。此轮 integration binary 后被重编，未密封原 binary；后续有效红证另存独立 binary。
- 缩小 fixture：保留三个真实公司、真实两年前史、上市／非上市与所有身份断言，仅将与目录无关的日经营 demand／生产／补货／admin 置零，消除大量 journal。未放宽门限。重编 1.17s，integration **2 过／4 败，1.39s**，完整显示空目录与真实身份期望的差异。
- 在业务红后实施：只从 selected 实际来源投影，每个上市代码通过本局 StockCatalog 查 ID；失败返回专用 error，不部分返回目录。
- 最终 green build：相同 build 命令与 profile，Cargo 报 **30.07s**；最终 integration **6/6，2.76s**；模块内部防线与真实 TS 导出 **2/2，0.41s**。两个预构建 binary 并行，各用 `node scripts/run-with-deadline.mjs 10000 -- ... --test-threads=8` 整命令进程树监督，故该命令内每 case 也受不大于 10s 的上限。未运行 all 或完整回归。
- debug profile 显式 `CARGO_PROFILE_DEV_DEBUG=0`／`CARGO_PROFILE_TEST_DEBUG=0`、`CARGO_BUILD_JOBS=8`；build 由 `run-long-validation.mjs 300000` 进程外监督，与普通测试分开。红构建中观察到 engine rustc 10 threads、累计 CPU 134%；单 crate 前端受 rustc 调度限制，未将 jobs=8 冒称全程 8 核。green 采样已在构建完成之后，只能证明当时无 cargo 在途，不能冒充运行期 CPU 样本。
- TS 导出隔离到 `.tmp/company-catalog-a/bindings`，只将生成的 `CompanyCatalogEntry.ts` 复制进正式目录；CompanyId.ts 与 StockId.ts 经 cmp 与现行生成文件一致。未导出或覆盖 PlanId 绑定。
- 有效红与最终绿 binary 分别用独立 reflink 原件保存在根 `.tmp/company-catalog-a/red-binaries/` 与 `final-binaries/`，SHA 见对应清单。

最终实现候选已准备交未实施者审查完整 diff，独立终裁待回传；没有合并 main 或推送。
