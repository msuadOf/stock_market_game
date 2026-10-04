# G26、G56：手动CI与Pages构建边界

Web lint添加--deny-warnings，现有手动开发CI继续调用该脚本；没有增加自动commit/PR触发，也不把lint或测试加到发布流程。短Fixture实际运行2线程oxlint：原不带flag的warning退出0，新入口warning退出1，合法输入退出0，并明确检查spawn无错误。

Pages只将与github.repository_owner对应的owner.github.io精确匹配仓库作为根站点（大小写不敏感），其他同后缀仓库仍是项目路径；owner和仓库名都有类型/字符检查。代表性VM执行真实工作流JS，模拟GITHUB_ENV写入，核根站点、普通项目、异owner后缀和缺失/非法env，未执行线上Actions或部署。

旧Pages反例真红。lint初次沙箱启动返回EPERM，不能据此宣称行为通过；获准沙箱外执行后，实际原命令/修复命令/合法输入对照成立。两个相关短测文件10项通过，约0.5s；外部timeout10s、Node case10000/concurrency4，Fixture终局清理，不运行全量lint或回归。

非作者web_gap_review完整代码/测试diff三门禁无阻断，建议非法owner短边界已补；最终完整diff与台账复核三门禁通过，没有阻断发现。
