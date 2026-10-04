# 前端显示、导航与恢复边界

## 已实施项

| 项目 | 实现与验证 |
|---|---|
| G13 | 最新优先100条成交带只取前7条，不反转，不改完整缓存边界；独立100条Fixture验证100至94。 |
| G44 | 分时/K线零量保留槽位但高度为0，正量最小高度仍1；覆盖[0,1,1000]与竞价null零量，实际DOM/SVG不另造最小高度。 |
| G45 | 保留原primaryTab，App使用MobileDetailLayer按detailCode显示详情；自选进入详情不再因原market gate与隐藏列表组合而空白，返回自选。 |
| G53 | Worker恢复nextGeneration须正安全整数且严格推进，验旧/同代、零/负数、小数、不安全整数及字符串。 |

## 短测与复核

4个相关Node单测文件按普通case与整命令10s限制执行，`timeout 10s node --test --test-isolation=none --test-timeout=10000 --test-concurrency=4 <files>`。新增反例和修正原错误冻结断言后旧码25通过、6失败；修复及边界补测后33通过（179ms）。另SSR真实共用详情层精确过滤G45，1通过（整批579ms），不是浏览器旅程或全回归。

依赖仅借用主工作区既有node_modules的只读用途，不提交依赖目录；Vite SSR未启动真实游戏或浏览器。TypeScript定向编译检查通过。未运行完整回归、性能矩阵或发布流程。

非实施者web_gap_review发现G45仅改reducer会让App旧market gate不显示自选详情，已修真实consumer并补SSR；其余三项代码无阻断。按其建议补非法generation和K线混合量边界；最终完整diff和台账再次复核，三门禁均通过，无剩余阻断发现。
