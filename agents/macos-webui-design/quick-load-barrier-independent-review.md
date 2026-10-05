# 快速槽读取屏障独立复核

2026-10-05。读取完整未提交 diff 及新增 quick-load-barrier.spec.ts。未实施产品代码或提交。独立从 apps/web 执行 save-commands、day-end-persistence、session-replacement 三文件，24/24通过、167ms，concurrency=3、case/外部进程树deadline均10000ms；未写dist或重复E2E。

## 三项门禁

1. 大 A 语义：仍只读取已有合法日终候选，不创建日内档、不改价格数量或会计数据。修复让读档先等点击前的写入完成，避免误读上一交易日，符合日终存档契约。
2. 必要性：pendingWrite保留当前待完成operation的原错误，tail保留串行清理能力；beforeRead同步捕获pending和await表达式中的tail，后续新写入不会无限延长本次屏障。完成回调只清理匹配operation，防止旧完成清除新pending。handleLoad等待前不invalidate已提交写入，等待后与读取后继续检查宿主/替换令牌，范围直接对应问题。
3. 错误与边界：pending失败经读取入口报错，不偷偷加载旧槽；清理后下一次显式重试允许读上一有效档。捕获前提、排队顺序、重复点击、等待期间换host与新epoch保护均有针对性断言。E2E在真实gzip之前加流门闩，继续使用原生压缩及真实IndexedDB，不是伪造存档。注入的压缩异常是受控故障，不应描述为无注入自然发生的浏览器故障。

## 有效发现

P2：原异步repository读取后的宿主替换边界测试退化。save-commands.test.ts中“load失败先同步…旧宿主响应”的尾部case调用load后立即置hostRef=null；新增beforeRead的await让执行在repository.load之前退出，pending.resolve未再对应已开始的读取。现有calls为空/notice仅一次仍会绿，但不再检验读取已开始后晚到档案不可安装。建议repository.load增加entered deferred，等待entered后替换宿主、再resolve，并保留原无回写断言。生产实现的第二次isCurrent检查静态正确，本项是必须保留的旧测试覆盖，已报主agent。

当前静态没有新增产品缺陷，但上述有效测试发现待修复复核，暂不关闭整批门禁。真实浏览器最终结果仍在主agent执行；不声明所有读写竞态或终端任务完成。

## 修复后最终复核

P2 已关闭：旧异步 repository 用例新增 entered，并明确等 load 实际进入后才替换 host/resolve 返回。由此恢复读取已在途的原边界，继续断言无 host/setup 回写及只有最初等待提示。生产代码无额外变更。

UX-CONTRACT 快速槽等待、pending失败停止读取、显式重试及后续invalidate契约与实现一致；同时修正第139行已批准排序/手机无效工具清理的过时描述，是正式文档对既有行为的追认，没有引入新产品范围。完整当前diff没有新有效finding。

读取 quick-load-barrier-reviewed-short.log 确认为25/25通过、193.16ms（不是口述约185ms）。Reviewer独立重跑原三文件24/24通过、151.59ms，apps/web cwd、concurrency3、case与外部进程树10000ms；两批文件范围不同，不混淆case数。主agent提供真实WASM/IndexedDB浏览器8/8通过、14.6秒。git diff --check通过。

独立审查门禁通过，无剩余有效finding。完整unit及后续production最终结果由主agent补充；旧净利gold与全部终端目标仍未据此关闭。
