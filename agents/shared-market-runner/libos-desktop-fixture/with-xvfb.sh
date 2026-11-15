#!/usr/bin/env bash
# 在云电脑以带认证的独立 X11 实例运行 Desktop；只使用局部 sysroot，不改系统配置。
set -euo pipefail
: "${LOCAL_DESKTOP_SYSROOT:?请指定已解包的 Debian Desktop sysroot}"
: "${LOCAL_DESKTOP_REPORT:?请指定新建的外部日志目录}"
display="${LOCAL_DESKTOP_DISPLAY_NUMBER:-94}"
[[ "$display" =~ ^[0-9]+$ && "$display" -ge 1 && "$display" -le 99 ]]
[[ "$#" -gt 0 ]]
p=$(realpath "$LOCAL_DESKTOP_SYSROOT")
r=$(realpath -m "$LOCAL_DESKTOP_REPORT")
mkdir -m700 "$r"
lib="$p/usr/lib/x86_64-linux-gnu"
mkdir -m700 "$r"/{runtime,cache,data,config,schemas}
export XDG_RUNTIME_DIR="$r/runtime" XDG_CACHE_HOME="$r/cache" XDG_DATA_HOME="$r/data" XDG_CONFIG_HOME="$r/config"
export XDG_DATA_DIRS="$p/usr/share:/usr/share" GSETTINGS_SCHEMA_DIR="$r/schemas" GDK_BACKEND=x11
export DISPLAY="127.0.0.1:$display" XAUTHORITY="$r/Xauthority"
env LD_LIBRARY_PATH="$lib" "$p/usr/bin/glib-compile-schemas" --targetdir="$r/schemas" "$p/usr/share/glib-2.0/schemas"
: >"$XAUTHORITY"
chmod 600 "$XAUTHORITY"
xauth -f "$XAUTHORITY" add "$DISPLAY" MIT-MAGIC-COOKIE-1 "$(head -c 16 /dev/urandom | od -An -v -tx1 | tr -d ' \n')"
d=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
gcc -O2 -Wall -Wextra -Werror -fPIC -shared -o "$r/locating-webkit.so" "$d/locating-webkit.c" -ldl
env LD_LIBRARY_PATH="$lib" "$p/usr/bin/Xvfb" ":$display" -screen 0 1200x800x24 \
  -listen tcp -nolisten unix -nolisten local -auth "$XAUTHORITY" >"$r/xvfb.log" 2>&1 &
xvfb=$!
cleanup() {
  kill "$xvfb" 2>/dev/null || true
  wait "$xvfb" 2>/dev/null || true
  rm -- "$XAUTHORITY"
}
trap cleanup EXIT
for i in $(seq 1 30); do
  if xdpyinfo >"$r/xdpyinfo.log" 2>&1; then break; fi
  kill -0 "$xvfb"
  sleep 0.1
done
grep -F "name of display:" "$r/xdpyinfo.log"
export LD_LIBRARY_PATH="$lib" LD_PRELOAD="$r/locating-webkit.so" LOCAL_WEBKIT_HELPER_DIR="$lib/webkit2gtk-4.1"
export WEBKIT_INJECTED_BUNDLE_PATH="$LOCAL_WEBKIT_HELPER_DIR/injected-bundle/libwebkit2gtkinjectedbundle.so"
"$@"
