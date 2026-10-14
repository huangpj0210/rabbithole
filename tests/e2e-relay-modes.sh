#!/usr/bin/env bash
# rabbithole relay_mode E2E(方案 §7.2 命令集补缺 / §7.3-3,P2 新增):
# tcp/tls × {默认(relay_mode=splice: mux 连接走 chunk 快路径), copy} ——
# ① 登录+SYNC+池通道+TCP 转发+半关闭 功能全链路;
# ② relay 路径计数器断言(rabbithole_relay_path_total):
#    - 默认 + multiplex=true → 计入 path="chunk"(B1 剥壳后 chunk 可达);
#    - copy + multiplex=true → 计入 path="copy",且 chunk/splice 计数为 0
#      (B7:relay_mode=copy 必须禁用 chunk 快路径);
#    - Linux 附加非复用轮:默认 → path="splice"(内核 splice 仅 Linux);
#      copy + 非复用 → path="copy" 且 splice 为 0。
# 用法: tests/e2e-relay-modes.sh [二进制目录(默认 target/release)]
set -u
BIN_DIR="${1:-$(dirname "$0")/../target/release}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SERVER="$ROOT/$BIN_DIR/rh-server"
CLIENT="$ROOT/$BIN_DIR/rh-client"
PASS=0; FAIL=0
ok()  { PASS=$((PASS+1)); echo "PASS: $1"; }
bad() { FAIL=$((FAIL+1)); echo "FAIL: $1"; }

echo "== rabbithole relay_mode E2E =="
# 只清理本轮拥有的进程(前缀 /tmp/rh-rm-;不碰 e2e-transports 的 rh-tr- 资源)
for pid in $(ps -eo pid,args | grep -E "rh-(client|server) --config /tmp/rh-rm-" | grep -v grep | awk '{print $1}'); do kill "$pid" 2>/dev/null; done
sleep 0.5

# 从 Prometheus 端点取路径计数(缺失返回 0)
relay_count() { # $1=metrics端口 $2=path
  curl -s -m 5 "http://127.0.0.1:$1/metrics" \
    | grep -E "^rabbithole_relay_path_total\{path=\"$2\"\} " \
    | awk '{print $2}' | head -1
}
zero_if_empty() { [ -n "$1" ] && echo "$1" || echo 0; }

run_one() {  # $1=proto $2=ctl $3=api $4=bind $5=echo $6=metrics $7=mode $8=mux
  local PROTO="$1" CTL="$2" API="$3" BIND="$4" ECHO="$5" MPORT="$6" MODE="$7" MUX="$8"
  local TAG="$PROTO-$MODE-mux$MUX"
  local W; W=$(mktemp -d "/tmp/rh-rm-$TAG.XXXXXX")
  local PIDS=()
  cleanup() { for p in "${PIDS[@]:-}"; do kill "$p" 2>/dev/null; done; sleep 0.3; rm -rf "$W"; }
  trap cleanup EXIT

  python3 - "$ECHO" <<'EOF' &
import socket, threading, sys
s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("127.0.0.1", int(sys.argv[1]))); s.listen(16)
def echo(c):
    try:
        while True:
            d = c.recv(65536)
            if not d: break
            c.sendall(d)
    except OSError: pass
    finally: c.close()
while True:
    c, _ = s.accept(); threading.Thread(target=echo, args=(c,), daemon=True).start()
EOF
  PIDS+=($!)

  local RELAY_LINE=""
  [ "$MODE" = "copy" ] && RELAY_LINE=$'relay_mode = "copy"'
  local MUX_T="false"
  [ "$MUX" = "1" ] && MUX_T="true"
  cat > "$W/server.toml" <<EOF
bind_addr = "127.0.0.1"
bind_protocol = "$PROTO"
bind_port = $CTL
multiplex = $MUX_T
$RELAY_LINE
[web]
bind_addr = "127.0.0.1"
port = $API
[metrics]
enable_prometheus = true
bind_addr = "127.0.0.1"
port = $MPORT
[vhost]
http_port = 0
https_port = 0
tcpmux_port = 0
[ports]
auto_range = "27700-27800"
[log]
level = "info"
EOF
  "$SERVER" --config "$W/server.toml" admin create admin Rm-123-456 >/dev/null 2>&1
  "$SERVER" --config "$W/server.toml" >"$W/server.log" 2>&1 &
  PIDS+=($!)
  sleep 2
  local TOKEN
  TOKEN=$(curl -s -X POST "http://127.0.0.1:$API/api/auth/login" -H 'Content-Type: application/json' -d '{"username":"admin","password":"Rm-123-456"}' | python3 -c 'import json,sys;print(json.load(sys.stdin)["data"]["accessToken"])' 2>/dev/null)
  [ -n "$TOKEN" ] || { bad "$TAG: 登录"; cleanup; trap - EXIT; return 1; }
  api(){ curl -s -m 15 -H "Authorization: Bearer $TOKEN" "$@"; }
  local CR CID SEC
  CR=$(api -X POST "http://127.0.0.1:$API/api/clients" -H 'Content-Type: application/json' -d '{"name":"main"}')
  CID=$(echo "$CR"|python3 -c 'import json,sys;print(json.load(sys.stdin)["data"]["id"])')
  SEC=$(echo "$CR"|python3 -c 'import json,sys;print(json.load(sys.stdin)["data"]["secret"])')

  local EXTRA=""
  case "$PROTO" in
    tls|wss|quic) EXTRA=$'tls_insecure_skip_verify = true' ;;
  esac
  cat > "$W/client.toml" <<EOF
server_addr = "127.0.0.1:$CTL"
protocol = "$PROTO"
multiplex = $MUX_T
client_id = "$CID"
secret = "$SEC"
$EXTRA
log_level = "warn"
EOF
  "$CLIENT" --config "$W/client.toml" >"$W/client.log" 2>&1 &
  PIDS+=($!)
  local online=""
  for i in $(seq 1 20); do
    online=$(api "http://127.0.0.1:$API/api/clients" | python3 -c 'import json,sys;print(json.load(sys.stdin)["data"][0]["online"])' 2>/dev/null)
    [ "$online" = "True" ] && break; sleep 0.5
  done
  if [ "$online" != "True" ]; then
    bad "$TAG: 客户端上线"
    cp "$W/server.log" "/tmp/rm-$TAG-server.log" 2>/dev/null; cp "$W/client.log" "/tmp/rm-$TAG-client.log" 2>/dev/null
    cleanup; trap - EXIT; return 1
  fi
  ok "$TAG: 客户端上线"

  api -X POST "http://127.0.0.1:$API/api/services" -H 'Content-Type: application/json' \
    -d "{\"client_id\":\"$CID\",\"name\":\"tcp\",\"kind\":\"tcp\",\"bind_port\":$BIND,\"target_host\":\"127.0.0.1\",\"target_port\":$ECHO,\"enabled\":true}" >/dev/null
  sleep 2
  # 功能:转发 + 半关闭(写 EOF → 回显 → 对端 EOF)
  python3 - "$BIND" <<'EOF'
import socket, sys, time
t0=time.time()
s = socket.create_connection(("127.0.0.1", int(sys.argv[1])), timeout=10)
s.sendall(b"ping\n"); s.shutdown(socket.SHUT_WR); s.settimeout(8)
g=b""
while True:
    d=s.recv(256)
    if not d: break
    g+=d
sys.exit(0 if g==b"ping\n" and time.time()-t0<8 else 1)
EOF
  local rc=$?
  if [ $rc -ne 0 ]; then
    bad "$TAG: 转发+半关闭"
    cp "$W/server.log" "/tmp/rm-$TAG-server.log" 2>/dev/null; cp "$W/client.log" "/tmp/rm-$TAG-client.log" 2>/dev/null
    cleanup; trap - EXIT; return 1
  fi
  ok "$TAG: 转发+半关闭"

  # 计数器:期望路径 >0、被禁用路径 ==0(B7)
  local WANT NOSPY
  if [ "$MODE" = "copy" ]; then WANT="copy"; else
    if [ "$MUX" = "1" ]; then WANT="chunk"; else WANT="splice"; fi
  fi
  if [ "$MODE" = "copy" ]; then
    if [ "$MUX" = "1" ]; then NOSPY="chunk"; else NOSPY="splice"; fi
  else
    NOSPY=""
  fi
  sleep 1
  local WANT_N NOSPY_N
  WANT_N=$(zero_if_empty "$(relay_count "$MPORT" "$WANT")")
  if [ "$WANT_N" -lt 1 ] 2>/dev/null; then
    bad "$TAG: 路径计数器 path=\"$WANT\" ≥1(实际 $WANT_N)"
    cp "$W/server.log" "/tmp/rm-$TAG-server.log" 2>/dev/null
  else
    ok "$TAG: 路径计数器 path=\"$WANT\" = $WANT_N"
  fi
  if [ -n "$NOSPY" ]; then
    NOSPY_N=$(zero_if_empty "$(relay_count "$MPORT" "$NOSPY")")
    if [ "$NOSPY_N" -ne 0 ] 2>/dev/null; then
      bad "$TAG: copy 模式不得走快路径 path=\"$NOSPY\" = $NOSPY_N(B7)"
    else
      ok "$TAG: copy 模式快路径已禁用 path=\"$NOSPY\" = 0"
    fi
  fi
  cleanup; trap - EXIT
  return 0
}

run_one tcp 19510 19511 19512 19519 19513 default 1
run_one tcp 19530 19531 19532 19539 19533 copy    1
run_one tls 19550 19551 19552 19559 19553 default 1
run_one tls 19570 19571 19572 19579 19573 copy    1
# 非复用轮:默认走内核 splice(仅 Linux);copy 同样必须禁用 splice
if [ "$(uname -s)" = "Linux" ]; then
  run_one tcp 19590 19591 19592 19599 19593 default 0
  run_one tcp 19610 19611 19612 19619 19613 copy    0
else
  echo "(非 Linux:跳过非复用 splice/copy 轮 —— splice 为 Linux-only,见 lib.rs cfg 门)"
fi

echo
echo "== 结果: PASS=$PASS FAIL=$FAIL =="
[ "$FAIL" = "0" ]
