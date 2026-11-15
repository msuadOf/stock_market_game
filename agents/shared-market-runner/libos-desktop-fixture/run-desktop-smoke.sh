#!/usr/bin/env bash
# 必须由 run-long-validation 监督；测量制品的 Inspector 只绑定当前云机的 loopback。
set -euo pipefail
[[ "$#" -ge 1 && "$#" -le 2 && -x "$1" ]]
mode="${2:-render}"
case "$mode" in
  render|stalled|fixed|max|all) ;;
  *) echo "Desktop fixture 模式无效：$mode" >&2; exit 2 ;;
esac
if [[ "${DISPLAY:-}" != 127.0.0.1:* ]]; then echo '请先进入 with-xvfb.sh 图形会话' >&2; exit 2; fi
: "${LOCAL_DESKTOP_REPORT:?缺少 Desktop 日志目录}"
port="${LOCAL_DESKTOP_INSPECTOR_PORT:-9338}"
[[ "$port" =~ ^[0-9]+$ && "$port" -ge 1024 && "$port" -le 65535 ]]
d=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
export WEBKIT_INSPECTOR_HTTP_SERVER="127.0.0.1:$port"
"$1" >"$LOCAL_DESKTOP_REPORT/product.log" 2>&1 &
app=$!
cleanup() {
  kill "$app" 2>/dev/null || true
  wait "$app" 2>/dev/null || true
}
trap cleanup EXIT
if [[ "$mode" == all || "$mode" == render ]]; then
  if [[ "$mode" == all ]]; then
    for scenario in stalled fixed max; do
      node "$d/desktop-controlled.mjs" "$port" "$scenario" | tee "$LOCAL_DESKTOP_REPORT/$scenario.ndjson"
    done
  fi
  node "$d/desktop-ui-smoke.mjs" "$port" | tee "$LOCAL_DESKTOP_REPORT/ui-samples.ndjson"
else
  node "$d/desktop-controlled.mjs" "$port" "$mode" | tee "$LOCAL_DESKTOP_REPORT/$mode.ndjson"
fi
xwininfo -root -tree >"$LOCAL_DESKTOP_REPORT/windows.log"
grep '股票模拟游戏' "$LOCAL_DESKTOP_REPORT/windows.log"
