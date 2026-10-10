#!/usr/bin/env bash
# 内嵌 Web 控制台 E2E:构建期检测 dist 并经 rust-embed 嵌入二进制,
# 验证单二进制托管(SPA 回退/资产 MIME//api JSON 404)与 static_dir 覆盖优先。
# 前置条件:console/dist 存在(pnpm build),且 rh-server 在其后构建过。
# 用法:bash tests/e2e-embed.sh [target/release]
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN_DIR="${1:-target/release}"
SERVER="$ROOT/$BIN_DIR/rh-server"
DIST="$ROOT/console/dist"
[ -x "$SERVER" ] || { echo "FAIL: $SERVER 不存在,请先 cargo build --release"; exit 1; }
[ -f "$DIST/index.html" ] || { echo "FAIL: 前端产物 $DIST 不存在,请先在 console/ 执行 pnpm build 并重编 rh-server"; exit 1; }

WORK=$(mktemp -d /tmp/rh-embed.XXXXXX)
PIDS=()
PASS=0
FAIL=0
cleanup() { kill "${PIDS[@]}" 2>/dev/null; rm -rf "$WORK"; }
trap cleanup EXIT
check() {
  if [ "$1" = "0" ]; then PASS=$((PASS + 1)); echo "PASS: $2"; else FAIL=$((FAIL + 1)); echo "FAIL: $2"; fi
}

W1=18197
W2=18198

# 环境一:不配 static_dir → 应使用内嵌资产
cat > "$WORK/s1.toml" <<EOF
bind_addr = "127.0.0.1"
bind_protocol = "tcp"
bind_port = 17950
[web]
bind_addr = "127.0.0.1"
port = $W1
[vhost]
http_port = 0
https_port = 0
tcpmux_port = 0
[log]
level = "info"
EOF
"$SERVER" --config "$WORK/s1.toml" >"$WORK/out1.log" 2>&1 &
PIDS+=($!)
sleep 2

grep -q "embedded ui" "$WORK/out1.log"
check $? "启动日志声明内嵌 UI(files=N)"

curl -s -o "$WORK/index.html" -w "%{http_code} %{content_type}" "http://127.0.0.1:$W1/" >"$WORK/r_index" 2>/dev/null
grep -q "^200 text/html" "$WORK/r_index" && grep -q 'id="root"' "$WORK/index.html"
check $? "根路径返回内嵌 index(html 200)"

curl -s "http://127.0.0.1:$W1/rabbithole/overview" >"$WORK/deep.html"
cmp -s "$WORK/index.html" "$WORK/deep.html"
check $? "深层路由 SPA 回退 index(200)"

ASSET=$(grep -o '/umi[^"]*\.js' "$WORK/index.html" | head -1)
[ -n "$ASSET" ]
check $? "index 引用 hashed 资产($ASSET)"
curl -s -o "$WORK/asset.bin" -w "%{http_code} %{size_download} %{content_type}" "http://127.0.0.1:$W1$ASSET" >"$WORK/r_asset" 2>/dev/null
grep -qE "^200 [0-9]+ text/javascript" "$WORK/r_asset" && [ "$(awk '{print $2}' "$WORK/r_asset")" -gt 1000 ]
check $? "内嵌 JS 资产可取(200 + MIME + 体积)"

curl -s "http://127.0.0.1:$W1/api/nonexistent" | grep -q '"code":404'
check $? "未知 /api/* 返回 JSON 404 envelope(不混 HTML)"

curl -s -o /dev/null -w "%{http_code}" "http://127.0.0.1:$W1/favicon.ico" | grep -q "^200"
check $? "favicon 可取"

# 环境二:显式 static_dir → 磁盘优先,覆盖内嵌
mkdir -p "$WORK/override"
echo '<html>OVERRIDE-MARKER</html>' >"$WORK/override/index.html"
sed "s/port = $W1/port = $W2/" "$WORK/s1.toml" | sed 's|^\[web\]|[web]\nstatic_dir = "'"$WORK"'/override"|' >"$WORK/s2.toml"
"$SERVER" --config "$WORK/s2.toml" >"$WORK/out2.log" 2>&1 &
PIDS+=($!)
sleep 2

grep -qc "embedded ui" "$WORK/out2.log" 2>/dev/null; [ "$(grep -c "embedded ui" "$WORK/out2.log")" = "0" ]
check $? "配置 static_dir 时不启用内嵌分支"
curl -s "http://127.0.0.1:$W2/" | grep -q "OVERRIDE-MARKER"
check $? "static_dir 磁盘内容覆盖内嵌(优先级正确)"

echo
echo "== 结果: PASS=$PASS FAIL=$FAIL =="
[ "$FAIL" = "0" ]
