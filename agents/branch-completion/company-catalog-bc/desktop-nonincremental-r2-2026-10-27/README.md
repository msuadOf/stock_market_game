# Desktop 非增量第二次构建候选

旧批于193.525秒因磁盘低于20GiB终止，原件在根company-catalog-desktop-2026-10-27保持不变。root清理已撤权的5.8GiB incremental缓存后，本批复用同target，只将CARGO_INCREMENTAL置0并在runner启动时硬性验证该标志，不改变业务source、features或test profile其他参数。

保留外300000ms总监督、内cleanup1000ms、60秒无进展与20GiB磁盘停止，j8。原Sol窄核flags/profile与runner差异后由root重新交唯一槽；不自续、不因空间阈值放宽上限。现只准备计划，没有运行Cargo。
