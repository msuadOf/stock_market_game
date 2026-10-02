# ADR-0028：标签发布、独立手动入口与静态 Pages

- 状态：accepted（用户于 2026-10-02 确认标签发布与静态托管）
- 日期：2026-10-02
- 关联：ADR-0027

## 决策

普通 commit 与 PR 不触发任何自动工作流。唯一自动入口是 `release.yml` 的标签
push：`vMAJOR.MINOR.PATCH`（非负整数，不接受前导零）或 `test-SUFFIX`（非空的
字母、数字、下划线、连字符，首字符为字母或数字）。宽泛标签过滤后仍须脚本校验；
不合规则标签明确失败，不编译或发布。正式版本为普通 Release，测试标签为预发行。
所有有效标签均更新 Pages，不以版本号大小判断是否跳过部署。

发布先校验标签及冻结提交 SHA，调用现有 CI 门禁，再编译三平台 Desktop、纯 Server、
WebUI Server 与一个静态 Web。所有阶段成功后核对当前 run 下载的十组 manifest、
文件数量、大小及 SHA-256，拒绝缺失、篡改、符号链接、重名与标签移动。先上传为
draft，再公开 Release，避免把部分上传冒充完整发行。中途失败可能留 draft，需
检查后处理，重跑不自动覆盖资产。
在公开前再次读取 GitHub 实际上传资产，逐项核对名称、大小与 SHA-256；不一致则保持
draft 并显式失败。安装包外部文件名使用带平台前缀的 ASCII，避免 GitHub 重写中文名称。
标签不自动改源码中的产品版本；每份 Release
额外携带源码 SHA 及制品清单，不伪称安装器版本号已同步为标签。

Windows、Linux、macOS 三个独立手动入口各只构建对应 Desktop；Server 手动入口
构建三平台纯 CLI；Web 手动入口构建静态包并部署 Pages。手动以 Actions 所选分支
启动时的最新提交为来源，随后整轮固定 `github.sha`，不是执行中随分支更新漂移。
另设 WebUI Server 手动入口构建三平台网页服务＋游戏后端部署包；All 手动入口
一次构建全部制品，成功后复用本轮静态制品部署 Pages，合计七个独立产品按钮。
保留 CI 与全量分发的维护用手动入口。手动编译不创建 Release。
构建步骤由可复用 workflow 和现有仓库脚本负责，不复制七套构建逻辑。

## 静态运行边界

Pages 仅承载静态资源。本地引擎仍在访问者的 WASM 多线程 Worker 中，远程模式
仍由已有 `RemoteHost` 连接用户填写的 Server。没有部署 Rust 服务或 Node 服务。
项目 Pages 使用仓库子路径；用户/组织根站点使用 `/`。桌面和 Rust 静态服务的
普通生产构建不启用 Pages 引导，不改其运行方式。

Pages 无法配置 COOP/COEP 响应头；`pages` 构建模式先注册限定到仓库路径的
自有 Service Worker，仅为同源静态入口和资源补隔离头。原请求、凭据、响应正文、
状态和内容类型保持不变，非同源与 API 请求不拦截，不创建离线/存档缓存。
首次受控导航最多重载一次，且在动态导入 App、读档、建立游戏宿主之前完成。
URL 标记防止无限重载，隔离成功后仅移除标记、不刷新。不在更新 SW 或游戏运行
期间强制刷新，更不在日内重新载入存档。能力缺失、权限失败或超时明确显示原因，
不静默切单线程、远程宿主或生成另一份行情。

浏览器定向 smoke 使用没有隔离响应头的普通静态服务，真实验证子路径、首次注册、
SharedArrayBuffer、本地游戏和 Rayon Worker；它不替代完整游戏回归或线上部署验收。
Pages 需 HTTPS/安全上下文；远程连接仍受 CORS、证书与混合内容限制。

## 缓存、权限与验收

编译权限为只读，Release 发布 job 才有 `contents: write`，Pages 部署 job 才有
`pages: write`/`id-token: write`；缓存删除仅在独立清理 job 使用 `actions: write`。
Pages 部署使用官方 `queue: max` 独立串行化、不取消正在部署的站点，也不以默认
single pending 行为丢弃中间标签。平台队列最多 100 个 pending，超额任务会由 GitHub
取消，需显式重跑，不声称无限队列。不同标签按实际进入部署队列顺序
更新站点，不承诺并发标签按提交时间排序。

不可变 tag 缓存无法作为其他 tag 的回退。标签流程统一在结尾清理：成功公开 Release
后退休本标签缓存，失败时保留编译进度；不删除默认分支、其他标签或未知分支的
最新缓存。仍检查活动 run、重新读取删除计划并核对剩余总量 ≤10,000,000,000 字节。
其他构建活动时暂缓；后续清理查询已公开且含 `release-source.json` 的有效标签
Release，补退休以前暂缓的标签缓存，不丢失回收计划、不依赖存入另一份状态文件。
只剩受保护缓存且超预算时明确失败，不谎报已经控制到 10GB。

Cargo 缓存显式只保存编译目录和依赖，不保存整个 `target/`：其中也有已发布的
`target/distributions/`、`target/build-artifacts/` 和当前 run 的前端制品。整目录回填
会让下一轮误见旧发行产物并触发拒绝覆盖。切换缓存前缀隔离旧快照；原生分发仍只
缓存自身 `target/build-cache/<product>`，静态/原生最终制品继续只通过本轮 artifact 传递。

2026-10-02 首次真实标签的 reusable CI 缓存 API 使用了
`refs/heads/refs/tags/<tag>` 而不是通常的 `refs/tags/<tag>`。清理显式兼容此 provider
编码，但每次计划/删除前均查询真实 Git heads；仅在没有同名真实分支时退休已
成功发布标签的 alias。查询失败、格式异常或同名分支存在时不以标签名猜测删除，
失败标签仍保留进度。这一兼容不扩大 `--retire-tag` 的当前 ref/成功发布 guard。

Windows 标签冷构建另增加明确的原生库编译阶段，阶段独立受五分钟外部期限约束；
原密封构建/执行和所有门禁不减少，总编译预算的增加如实见 [testing.md](../testing.md)。

本次只执行代表性短测、Pages 定向编译/浏览器 smoke、Actions 语法验证及独立完整
diff 复核。本地不运行完整回归，标签发布保留现有 CI 门禁。三平台实际发布、Pages
启用及线上可玩状态须分别以真实 Actions/部署结果报告，不能以 YAML 或 dry-run
代替。签名、公证、GUI 安装验收仍不在本次范围。

不改 A 股交易规则、资金/股数单位、策略与日终存档语义。

## 技术依据（2026-10-02 查阅）

- [GitHub 官方 deploy-pages](https://github.com/actions/deploy-pages)：同 run 的
  upload-pages-artifact、独立部署权限、`github-pages` environment 与 OIDC 校验。
- [官方 concurrency 文档](https://github.com/github/docs/blob/main/data/reusables/actions/actions-group-concurrency.md)：
  默认 single 会取消已有 pending，`queue: max` 按进入等待队列顺序保留至多 100 个 pending。
- [COI Service Worker 参考实现](https://github.com/gzuidhof/coi-serviceworker)：
  受控导航的隔离头注入方式。本项目未引入该依赖；不采用其自动更新刷新、跨源
  响应改写、credentialless 降级或静默错误处理。
