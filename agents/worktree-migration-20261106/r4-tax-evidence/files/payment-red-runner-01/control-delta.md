# runner 模板差异

本目录由只读模板 `agents/branch-completion/joint-green50-r2-2026-10-29` 机械裁剪。执行器仅保留一个 `engine --lib` feature 构建图，双 feature、`-j8`、`CARGO_INCREMENTAL=0`、逐 exact 10 秒监督、共享 296/297/298/299/300 秒时限、subreaper 清场、scratch 整树回收及源码 mtime 恢复流程。三门各由独立 child 并发执行，池宽为 3；每门必须取得 exit 101、`0 passed; 1 failed`、单一指定源码 panic 与其 `requiredEarliestPanic`，并同时满足 `businessCaseRed` 和 `expectedMatched`。编译错误、timeout、fixture 或其他 panic 均不符合目标红。

裁剪删除50门及两个 integration test target；artifacts 仅接受 `engine` feature unit，normal `lib` artifact 可记录但非必需。`freeze-current.py` 固定 integration cwd，并在冻结前核验 receiver 两个付款测试源码 SHA 与付款 freeze。scratch owner 为 `/tmp/stock-market-game-validation-2026-10-27-1311109/engine-r4-payment-red-2026-10-29-01`。前槽仍 pending，root 授权仍为 false。

以下文件与模板字节相同（SHA-256 相同）：
- `absolute-supervisor.mjs` SHA-256 `3441ee156c98438c9a146532a007375c334faedbeac860acf63be99b412435b7`
- `subreaper-owner.py` SHA-256 `804b358e8758677c44ed22efc00f928e48fffe238800dea995c1dd6eaa472631`
- `build-verified.mjs` SHA-256 `71a796990f63dd134c7a053fdb36a61dda18a403f8161a6796680f7611c4e6e9`
- `terminate-owned.mjs` SHA-256 `4cd50b0a014ba7f1f3a4d00fed977086482dae3b186b7b794a2d40c32699f3f6`
