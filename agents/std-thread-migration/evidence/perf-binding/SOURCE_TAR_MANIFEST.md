# 源码树 tar 清单（GitHub 100MB 上限剔除替代）

历史改写（2026-10-19）从 28efd680 剔除了两个 105MB 源码树 tar。证据效力不变——
SHA 可复现性意味着 tar 本体可再生，校验和+配方即完整证据：

| 原 tar | SHA256 | 再生方式 |
|---|---|---|
| 1f5863cd/source.tar | ab41e491eeec5a1cd10eb7d469b8c37a3fedca03ed3a65c1e992331fc66ef9ee | `git worktree add <p> 1f5863cd --detach && tar -C <p> --exclude=.git -cf source.tar .`（详见同目录 MANIFEST.md 的构建绑定记录） |
| 6dcc25b4/source.tar | f741464c24e7e93de14bcfcdd735f43341ac165178e02f26b578903b57b8b863 | 同上（6dcc25b4） |

双路补证结论（Sol 独立重建+实现侧 082edd50 一致：6dcc25b4 纯树重建 SHA 逐位复现）不受影响。
