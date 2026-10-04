# G47：竞价价格线分段

投影直接遍历含null的竞价槽，按连续有效段返回auctionSegments；真实SVG分别绘polyline，单点段保留dot，整段空/null不绘价格，竞价累计量仍保留所有槽。不改竞价撮合或模拟价格。

新纯投影反例和实际SSR反例在原码都失败（无分段接口/实际SVG只有1根跨null线）。修复后两个相关文件共14项短测通过（91ms/818ms）；外部timeout10s，Node case10000/concurrency2；TypeScript编译通过，无浏览器或回归验收。

非作者web_gap_review完整代码及测试diff三门禁通过，无阻断发现；最终完整diff与台账再次复核通过。
