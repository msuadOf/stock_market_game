# Frozen refs 清理记录

日期：2026-11-01。以下 19 个 refs 经 Sol 审查后已删除；完整 SHA 记录如下。删除 spine 中间快照的结论以保留 `frozen/spine-rebuild21-wt` 为前提。

```text
refs/heads/frozen/bseries-af555e50e2-b01 @ 62c2a4f577958f565d5583c5211dfc744db26ed3
refs/heads/frozen/bseries-af555e50e2-b10 @ 63b4abf5b379c73dafcad6be3ccb35dc48e030af
refs/heads/frozen/bseries-af555e50e2-b11 @ d08c3abb191613ca6ed935d3e3423f2f8344270d
refs/heads/worktree-agent-a488feedc44747f95 @ 82566326232020fc42033a21024967099a7eec0d
refs/heads/worktree-agent-aa88876b0d940c34d @ 57ea2230793988e190e6cfd03d22ee927fdfe516
refs/heads/worktree-agent-af04da2cc5591f275 @ 320b20e1ce57312c120507267b19c32cc21eacfe
refs/heads/frozen/spine-o8-base @ ca59e9f72f3bfc5baf2bc1c07d4c75eecbf81aae
refs/heads/frozen/spine-gen-wt8 @ f602c2f0463c9372bb3c4abcfa4f6da352c325f7
refs/heads/frozen/spine-rebuild8-wt @ 3fe3924f201d2fb3efe1a5290510b1e31b1980e4
refs/heads/frozen/spine-rebuild9-wt @ 197177bf7b8231031640d52c95d27a7830388f7a
refs/heads/frozen/spine-rebuild10-wt @ 34ed6ac82b406e207a3145a8b710614d2fa4d4b4
refs/heads/frozen/spine-rebuild11-wt @ 3e08b5a39f3f0b9ba0234bf011c910f9777487f1
refs/heads/frozen/spine-rebuild12-wt @ 35e3e4bb3d728301a0a4181340612694c7397b71
refs/heads/frozen/spine-rebuild13-wt @ 23897a127a0d457abcfbb16640af94f0e5cc521f
refs/heads/frozen/spine-rebuild14-wt @ 379259f546158a6aa09df991c985b9baa065454e
refs/heads/frozen/spine-pre17-wt @ b2a4a965f35f004591b5fd68cb696258db7a237d
refs/heads/frozen/spine-rebuild17-wt @ cf8740ac0ce5a826213556d4fb4f1764ae25544a
refs/heads/frozen/spine-rebuild19-wt @ 4db20dcadedd9ffce051bc112095c1ee6dd65494
refs/heads/frozen/spine-rebuild20-wt @ cf5782284204db52a2d4bbb8005c57764e4e23ac
```

保留 `refs/heads/frozen/spine-rebuild21-wt`，SHA 为 `3b7e6ac4e526d6d602344b5bbcc1471f6e63b8d8`；当前保留 130 条本地分支。该候选与 main 的剩余差异尚未完整核销，本记录不代表分支收敛或其他待办已完成。

审查依据：`/tmp/stock-converge-frozen-review.md`；其中记录了各组删除依据及保留边界。
