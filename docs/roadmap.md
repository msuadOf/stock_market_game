# Roadmap

## Stage 1 — 本地可玩（已实现）

Rust/WASM Worker 权威引擎、行情与交易 UI、A 股核心竞价规则、本地/文件存档。

## Stage 2 — 可选权威后端（进行中）

已实现 actor-per-session、REST 命令和快照、WebSocket 事件、显式重同步、容量限制和会话销毁。
正式公网部署前仍需落地真实身份认证、授权、TLS、持久化数据库、配额和运维指标。

## Stage 3 — 桌面应用（进行中）

Tauri actor、事件桥、暂停/恢复、存档/读档已实现。后续工作是安装包签名、自动更新与平台级 E2E。
