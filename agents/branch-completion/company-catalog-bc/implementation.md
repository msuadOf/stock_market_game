# 公司目录 B/C 实施记录

- 原始 main base：`1afece4c88fca7688305a7bfb713d9f107d4b2ba`；A 已批依赖 cherry-pick：`01792dbf`（原 `478c63db`）。仅后续提交属于 B/C 增量。
- 冻结设计 SHA-256：`ccb52871a86f8662b26cf55eb4f685198a5e211c1ff2bf3ed51437bae82310a8`。
- Node `v25.8.2`；复用 root `node_modules` 两处软链接，不声称 clean dependency 安装。
- resolver 红测：`node scripts/run-with-deadline.mjs 10000 -- node --test --test-timeout=10000 --test-concurrency=4 apps/web/src/host/catalog-resolver.test.ts`，原输出 `resolver-red.stdout/stderr`，缺模块失败。
- 同命令首次绿尝试发现 unknown scope 错误消息未带路径，保留 `resolver-green.stdout/stderr`；补上下文后 `resolver-green2.stdout/stderr` 3/3 通过。命令进程树 deadline 与 case timeout 均为 10000ms；并发参数 4。
- resolver 使用实际原 ID 与 Map，严格校验 exact fields、u32、唯一性及证券 issuer 全覆盖；MemberId 映射核对目录存在和报告公司一致性。不写 generated，StockIdentityEntry 类型导入待真实 Rust 生成。
- `PublicationOrigin::MonthlyDisclosure` 的 shared stored parser 由协调者另管，不归本 writer。
- 当前未运行 Cargo、types:generate、build/all；Rust 与真实三宿主仍待窗口及验证。

## 后续批次在途

- `company-code-safety.test.ts` 在真实 Redux/Immer store 下先红（特殊键命中原型），null-prototype + own-key 修复后与 resolver 合跑 4/4；原输出 `cache-red/green`。
- `actual-stock-ingest.test.ts` 原红明确显示实际单股 StockId0 被译为 `000812`；要求显式 resolver 后短测绿色，与上述两组共 5/5，原输出 `stock-ingest-red/green`。测试运行命令继续采用外部10000ms、case10000ms、并发4。
- StockIdentity Rust 测试先写于 `packages/engine/tests/stock_identity_view.rs`，因尚无 Cargo 窗口，**未运行红编译，不写实现和不伪造 Rust 红灯**。
- 同代 protocol 参数、mandatory baseline prepare 与三 host 传输正在接入；这一阶段整树尚未验证。

## TDD 纪律更正

- resolver 首次 `ERR_MODULE_NOT_FOUND` 仅为接口诊断，不能当有效业务红；后续实现前未为该新增模块取得可编译业务红，这是本片纪律偏离，不能事后用 stub 负控追认为历史 TDD。
- stock translator 的 `000812 !== 600101` 与 Redux 特殊键运行失败是有效业务红，原输出原样保留。
- actual 公司 selector：首缺导出诊断不计业务红；新增空数组 placeholder 后，`company-options-business-red` 真实断言失败，再实现实际目录投影；`company-options-green` 共8/8。
- coordinator/atomic baseline 参数化在途阶段先做了接线再增加部分契约测试，相关测试只能算现有实现验证，不能声称全部先红。本记录如实区分，不回填历史。

## StockIdentity 实际 Rust 红绿

- 已授权唯一重 Cargo 窗口，独占 target：`/data1/baiyifan/workplace/stock_market_game/.tmp/company-catalog-bc/target`，从静止 `.tmp/company-catalog-a/target` 以 `cp -a --reflink=always` 创建；无 full-copy fallback、无 hardlink，inode 原件见 `target-inodes.txt`。
- build 均 `CARGO_BUILD_JOBS=8`，同默认 test profile（实际为 unoptimized + debuginfo），外部 `run-long-validation.mjs 300000`，命令 `cargo test -p engine --test stock_identity_view --no-run -j8 --message-format=json`。CPU/线程原采样见 `stock-red/green-cpu.txt`。
- 先最小可编译 DTO + 空 Vec placeholder，真实 binary `actual_stock_identity_preserves_catalog_ids_after_restore --exact --nocapture` 在外部10000ms内断言红：`[] != [{0,002156},{1,600610}]`，1.48s；再实现 actual `stock_catalog().iter()` 原 key 投影，同target build 24.66s，绿case 3.04s（shell总4.15s）。保留源码/binary SHA、原stdout/stderr。
- 必要 export：`cargo test -p engine --lib export_bindings_stockidentityentry --no-run -j8 --message-format=json`（build94s，300000外deadline）；随后真实 export case，`--test-threads=8`、10000ms外deadline。首次直接 binary 未继承 Cargo env，真实产物落 `bindings/`，没有冒称正确路径；显式 `TS_RS_EXPORT_DIR=<worktree>/apps/web/src/types/generated` 重跑1/1通过，实际生成 StockIdentityEntry.ts，未手写 generated。
- 最后已无 cargo/rustc，自有进程树退出、磁盘74G；立即通知协调者与W1移交窗口。并未编译三 host。

## StockIdentity 原件留存与观测局限补充

- 当时原红源码仅存 `stock-red-source.sha256`，未另存源码 snapshot；红 binary 在同路径的绿构建中被覆盖，没有保存可再运行的红 binary 原件。原 build/case 命令存在本会话原 tool 调用体，没有独立命令 receipt 文件。
- 2026-10-26 从原 `cat > packages/engine/src/session/stock_identity_view.rs` tool 调用体逐字恢复 `stock-red-source-recovered-from-tool.txt`；这是**事后从原调用恢复的副本**，不是当时落盘的 snapshot。SHA-256 `63488f07879c27257311faad93e5171e7aaa4d7e0037881818178c236a016778` 与红前原SHA匹配；原临时注释亦保留，不通过猜测内容凑SHA。
- green/export CPU 采样过晚，仅采到 shell/rg，未采到 cargo/rustc；`-j8` 仅证明显式并发参数，不能据此声称实际满核。red采样有rustc，单crate阶段有编译器固有限制。后续批次应在构建在途时采样。
- 真实 green/export binary 与生成物已由未实施者分别短跑及逐字核对；上述原红原件与采样局限仍保留，不用后验负控追认原始TDD。
