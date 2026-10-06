---
name: no-versions
description: 早期开发阶段没有版本概念——不用 v1/v2/v3 命名，一切可破坏性修改，不考虑兼容性
metadata:
  node_type: memory
  type: feedback
  originSessionId: f28cd04a-805e-42cf-b943-c0b6a1ae0d82
  modified: 2026-10-06T11:32:13.372Z
---

项目处于最初期开发阶段：没有版本，不用 V1/V2/V3 之类版本概念命名（协议、机制、接口都不要带 vN 后缀）；所有修改不用考虑兼容性，对历史设计/存档/API 都可以做破坏性修改。

**Why:** 用户 2026-10-06 明确要求。此前探索波产出的协议名带了"v1"（如 AIP-gate v1）——此类命名要去掉。

**How to apply:** 命名直接用描述性中文名（如"锚定交错配对"而不是"AIP-gate v1"）；改接口/改数据结构时直接改，不写迁移层、不留兼容分支、不做向后兼容 shim。相关：[[engine-perf-1000x]]
