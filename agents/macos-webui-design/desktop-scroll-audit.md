# 看盘交易栏的 document 滚动边界

2026-10-05，基线 c8dfe6e；本批只有公共 index.css 的 sr-only 增加 top:0/left:0、两尺寸真实浏览器测试及对应契约/工作记录。上一批22a1c68/c8dfe6e均已独立复核并推送，本地与origin一致。

## 实际问题与 TDD

IAB 原tab2、902×833、09:15:51暂停1x，K线及MA显示时从报价买入展开底部看盘栏。app-root/body均833，document却996。terminal-trade-scroll-before.json及terminal-kline-trade-final-902.png保存原值；两个条件单sr-only标签定位bottom959.3125/996.3125，未设锚点，因其定位包含块而逃离内部滚动区。没有修改可见布局高度、交易字段或滚动owner。

首次新E2E未启动，因Node侧tsconfig没有全局document/window而编译失败：desktop-scroll-red-e2e.log及red-type-diagnostics.log；该失败不是产品TDD红。改用真实html locator的element/ownerDocument后，desktop-scroll-behavior-red-e2e.log中902/1020两个case均为实际document contentHeight999，期望833。测试case保持10000ms，外部共享300000ms；fixture与IAB字体几何差异没有抹去document越界事实。

随后公共sr-only仅补定位锚点。902/1020真实WASM定向2/2、8.2s，desktop-scroll-green-e2e.log；再增加完整交易页和320手机交易底页的名称、编辑、草稿及关闭回列表检查，2/2、7.4s，desktop-scroll-crossscreen-e2e.log。完整67/67、47.9s是增强前两case的完整批次；增强后最终完整批次另存desktop-scroll-final-full-e2e.log，不把两个批次拼成一份证明。

## 实际效果与边界

同一IAB会话仅CSS HMR，无重启或推进：terminal-trade-scroll-after.json记录document clientHeight/scrollHeight均833、scrollY0；order-panel clientHeight203/scrollHeight497，内部滚动仍有效，两个标签锚点0且裁切。一张新截图terminal-kline-trade-after-902.png保留K线、五条MA、盘口与看盘交易栏同屏，外层多余滚动撤下。没有玩家委托、条件单提交或名单修改，仍09:15:51暂停1x。

新增两case保留精确document高度和scrollY0，真正聚焦/填写两个条件单输入，检查内部scrollTop>0；完整交易页保持值及高度，跨320手机底页按可访问名称读/写草稿、聚焦、关闭并访问共享搜索框。sr-only四处使用均为非聚焦标签/描述，未用display:none或aria-hidden删除辅助名称；原可见输入、事件处理和表单校验未改。没有新增状态、依赖、设备分支或第二份滚动策略。

本批未改JavaScript产品代码，沿用上一批完整849 Web的相同源；本批验证CSS以真实浏览器为主。增强后的最终完整67/67、1.2分钟，workers3/RAYON10、共享外部300000ms，见desktop-scroll-final-full-e2e.log。记录一次原CPU观察失败：16次ps加8秒sleep的开销超过监督总10000ms，工具exit1、进程树终止，desktop-scroll-live-cpu.log为空；后续读空文件的JSON诊断也失败，没有可用CPU数据。没有延长上限，而缩为6次/单ps最多1秒并逐条flush后，仅为资源核对重跑增强两case：2/2、27.2s（共享300000ms的长资源观察验证，case仍10000ms，不归类为普通10秒短命令），观察器exit0，但六条processes均空，仍未取得本批live CPU证据。数据见desktop-scroll-resource-e2e.log/resource-cpu.log；配置workers与成功并发case不冒充实际CPU饱和，也不将该观察当产品红。先前完整批次实际CPU样本只适用于其当时记录，不移作本批成功采样。无需为取得漂亮数值继续重复已经通过的测试。

最终资源验证结束后独立production完成tsc/Vite1.15s/releaseWASMverified，日志desktop-scroll-final-production-build.log；不存在浏览器运行时覆盖dist的并行写入。新E2E变更lint exit0，普通case10000ms未增；全库五个原children-prop告警仍按上一批失败保留，不改无关测试。最终strict premium0finding、diff-check通过。本批Independent review来自全新desktop_scroll_restarted_review_sol_high（gpt-6.1-sol high），完整diff/最终证据复核后才可提交及关闭整体goal；不能沿用上一批记录提前宣称完成。

用户要求停止当前Independent review并重新新开；desktop_scroll_final_review_sol_high已中断，本次另开desktop_scroll_restarted_review_sol_high，不复用旧agent或其结论。新审查记录使用desktop-scroll-restarted-independent-review.md。

IAB收尾：从实际界面收起交易栏、切回分时与600101自选列表，reset临时viewport并markDeliverable保留原tab2。自然902×833、document833/833、scrollY0，09:15:51暂停1x保持；terminal-restored-final-current-window.png为恢复截图。lsof确认node37203仍监听127.0.0.1:3000。未提交玩家委托或更改名单。

最终门禁：全新desktop_scroll_restarted_review_sol_high独立完成完整diff/19项/原始日志与截图核对，确认本批产品、测试及契约通过，无待修复finding，正式记录desktop-scroll-restarted-independent-review.md。审查自身的strict premium只读监督也为exit0/0finding。最后Git提交推送由本记录后实际执行并核对远端，不在本记录写入时宣称已发生。
