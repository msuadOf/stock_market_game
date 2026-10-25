# 公司目录 B/C 实施记录

- 原始 main base：`1afece4c88fca7688305a7bfb713d9f107d4b2ba`；A 已批依赖 cherry-pick：`01792dbf`（原 `478c63db`）。仅后续提交属于 B/C 增量。
- 冻结设计 SHA-256：`ccb52871a86f8662b26cf55eb4f685198a5e211c1ff2bf3ed51437bae82310a8`。
- Node `v25.8.2`；复用 root `node_modules` 两处软链接，不声称 clean dependency 安装。
- resolver 红测：`node scripts/run-with-deadline.mjs 10000 -- node --test --test-timeout=10000 --test-concurrency=4 apps/web/src/host/catalog-resolver.test.ts`，原输出 `resolver-red.stdout/stderr`，缺模块失败。
- 同命令首次绿尝试发现 unknown scope 错误消息未带路径，保留 `resolver-green.stdout/stderr`；补上下文后 `resolver-green2.stdout/stderr` 3/3 通过。命令进程树 deadline 与 case timeout 均为 10000ms；并发参数 4。
- resolver 使用实际原 ID 与 Map，严格校验 exact fields、u32、唯一性及证券 issuer 全覆盖；MemberId 映射核对目录存在和报告公司一致性。不写 generated，StockIdentityEntry 类型导入待真实 Rust 生成。
- `PublicationOrigin::MonthlyDisclosure` 的 shared stored parser 由协调者另管，不归本 writer。
- 当前未运行 Cargo、types:generate、build/all；Rust 与真实三宿主仍待窗口及验证。
