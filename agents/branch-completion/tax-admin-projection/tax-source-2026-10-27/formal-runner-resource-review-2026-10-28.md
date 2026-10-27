# 正式 runner 资源独审：READY，待 E2 实际交接

未实施复核者仍为原生 `gpt-6.1-sol` / `xhigh` task `ordinary_tax_source_review2_28`；仅增量只读核runner，不运行Cargo、不修改writer文件。

`formal-green-03/plan-template.json` SHA `1e12d2a9ad4549ee4c4ab09a6eed89b0d43e0d18c88a6ff5e71c9552dbdd7fad`，全部runner/product绑定吻合，四产品文件未变。
runner复用R4/P1已审模板：唯一RAM target、j8/INC0；外300000ms、共享执行299000ms；11个exact各外10000ms、8并发；guard按真实Cargo进程树CPU和日志增长采样，不新造guard。
新增E2前槽default单件保护：仅一个cp --reflink=never，seal在target.parent且在Cargo target和待删owner外；如已有独立seal核SHA/bytes/inode后复用。构建后再核seal。finally只清本batch scratch，不删共享target或保护单件。
静审时target和parent同tmpfs、可用约98.22GiB；这些是当时读数，不能替代启动前实际读数。

仍待E2 actualwait/owned空后给其当时default binary的真实path/fullSHA/bytes/features和已有seal，不沿用R4旧绑定。交接时须核canonical路径确在同RAM、设备号匹配且不落owner，因runner的is_relative_to本身仅词法检查，不能单独识别symlink或..逃逸。

目前目录只有计划，无prior-artifact.json、batch receipt或正式执行结果，不能称本税树正式构建/11项已过。
