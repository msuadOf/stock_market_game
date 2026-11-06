# Tauri 历史红绿测试证据

本目录归档来自 `web-origin-tauri-2026-10-29` 工作区的六份未跟踪历史输出。历史红测记录 47 项中 1 项通过、46 项失败，退出码 1；历史绿测记录 47 项全部通过，退出码 0。它们仅反映当时的测试输出，不能证明当前版本的状态。本次归档未执行当前测试，也未确认当时的测试命令或代码版本。

为脱敏，日志中的源工作区绝对路径前缀替换为 `<WORKTREE>/`，另一验证工作区的绝对路径前缀替换为 `<VALIDATION_WORKTREE>/`；其余字节保持不变。下表逐项记录替换前源文件和归档文件的 SHA-256，以及各前缀替换次数；空 stderr 也保留。

| 文件 | 源 SHA-256 | 归档 SHA-256 | 源工作区次数 | 验证工作区次数 |
| --- | --- | --- | ---: | ---: |
| `red.exit` | `4355a46b19d348dc2f57c046f8ef63d4538ebb936000f3c9ee954a27460dd865` | `4355a46b19d348dc2f57c046f8ef63d4538ebb936000f3c9ee954a27460dd865` | 0 | 0 |
| `red.stderr` | `d0ebb86e275cd92eb1d6df203cd2616b61cfe02d0cbdadfe0b71ebae146ec6e4` | `d0ebb86e275cd92eb1d6df203cd2616b61cfe02d0cbdadfe0b71ebae146ec6e4` | 0 | 0 |
| `red.stdout` | `154d4df0b45bd753adb23f8b50eab2437f8c58a03eba2fa2f56a7bd166a0995f` | `8cf8932e6afc560bb7bed1e47618512691088af08c12070626ca7dd24dcd512b` | 596 | 2 |
| `green.exit` | `9a271f2a916b0b6ee6cecb2426f0b3206ef074578be55d9bc94f6f3fe3ab86aa` | `9a271f2a916b0b6ee6cecb2426f0b3206ef074578be55d9bc94f6f3fe3ab86aa` | 0 | 0 |
| `green.stderr` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` | 0 | 0 |
| `green.stdout` | `f145aa3cae70f6454fd5aeaf20bb4928e64775270d383fc8136abdcf71dccb7f` | `f145aa3cae70f6454fd5aeaf20bb4928e64775270d383fc8136abdcf71dccb7f` | 0 | 0 |
