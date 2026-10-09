#!/usr/bin/env bash
# rabbithole 传输层变体 E2E:tls / ws / wss / quic / kcp 主控通道各起独立环境,
# 验证 登录+SYNC+池通道+TCP 转发+半关闭 全链路(tcp 已由 e2e.sh 覆盖,此处作对照)。
# 用法: tests/e2e-transports.sh [二进制目录(默认 target/release)]
set -u
BIN_DIR="${1:-$(dirname "$0")/../target/release}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SERVER="$ROOT/$BIN_DIR/rh-server"
CLIENT="$ROOT/$BIN_DIR/rh-client"
PASS=0; FAIL=0
ok()  { PASS=$((PASS+1)); echo "PASS: $1"; }
bad() { FAIL=$((FAIL+1)); echo "FAIL: $1"; }

echo "== rabbithole 传输变体 E2E =="
for pid in $(ps -eo pid,args | grep -E "rh-(client|server) --config /tmp/rh-tr-" | grep -v grep | awk '{print $1}'); do kill "$pid" 2>/dev/null; done
sleep 0.5

run_one() {  # $1=proto $2=ctl_port $3=api_port $4=bind_port $5=echo_port
  local PROTO="$1" CTL="$2" API="$3" BIND="$4" ECHO="$5"
  local W; W=$(mktemp -d /tmp/rh-tr-$PROTO.XXXXXX)
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

  cat > "$W/server.toml" <<EOF
bind_addr = "127.0.0.1"
bind_protocol = "$PROTO"
bind_port = $CTL
multiplex = true
[web]
bind_addr = "127.0.0.1"
port = $API
[vhost]
http_port = 0
https_port = 0
tcpmux_port = 0
[ports]
auto_range = "27500-27600"
[log]
level = "info"
EOF
  "$SERVER" --config "$W/server.toml" admin create admin Tr-123-456 >/dev/null 2>&1
  "$SERVER" --config "$W/server.toml" >"$W/server.log" 2>&1 &
  PIDS+=($!)
  sleep 2
  local TOKEN
  TOKEN=$(curl -s -X POST "http://127.0.0.1:$API/api/auth/login" -H 'Content-Type: application/json' -d '{"username":"admin","password":"Tr-123-456"}' | python3 -c 'import json,sys;print(json.load(sys.stdin)["data"]["accessToken"])' 2>/dev/null)
  [ -n "$TOKEN" ] || { echo "login failed"; return 1; }
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
multiplex = true
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
    bad "$PROTO: 客户端上线"
    cp "$W/server.log" /tmp/tr-$PROTO-server.log 2>/dev/null; cp "$W/client.log" /tmp/tr-$PROTO-client.log 2>/dev/null
    cleanup; trap - EXIT; return 1
  fi
  ok "$PROTO: 客户端上线"

  api -X POST "http://127.0.0.1:$API/api/services" -H 'Content-Type: application/json' \
    -d "{\"client_id\":\"$CID\",\"name\":\"tcp\",\"kind\":\"tcp\",\"bind_port\":$BIND,\"target_host\":\"127.0.0.1\",\"target_port\":$ECHO,\"enabled\":true}" >/dev/null
  sleep 2
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
  rc=$?
  if [ $rc -ne 0 ]; then
    bad "$PROTO: 转发+半关闭"
    cp "$W/server.log" /tmp/tr-$PROTO-server.log 2>/dev/null; cp "$W/client.log" /tmp/tr-$PROTO-client.log 2>/dev/null
  else
    ok "$PROTO: 转发+半关闭"
  fi
  cleanup; trap - EXIT
  return $rc
}

check(){ [ "$1" = "0" ] && ok "$2" || bad "$2"; }

run_one tcp  19310 19311 19312 19319
run_one tls  19330 19331 19332 19339
run_one ws   19350 19351 19352 19359
run_one wss  19370 19371 19372 19379
run_one quic 19390 19391 19392 19399
run_one kcp  19410 19411 19412 19419

echo
echo "== 结果: PASS=$PASS FAIL=$FAIL =="
[ "$FAIL" = "0" ]
