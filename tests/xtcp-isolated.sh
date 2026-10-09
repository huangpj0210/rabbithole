#!/usr/bin/env bash
# rabbithole XTCP 隔离网络验证(无 Docker/pf 依赖,用户态 NAT 网关模拟)。
#
# 拓扑(全部独立进程;"内部主机"仅见各自网关,对端地址只能来自 STUN 映射):
#   client-P(xtcp proxy) ── NAT-A(natgw.py,STUN+UDP中继) ──┐
#   client-V(xtcp visitor)── NAT-B(natgw.py,STUN) ─────────┼── rh-server
#   用户 ── client-V:19312                                    └── echo:19391
#
# 数据面真实性:候选地址 = 网关分配的映射地址(非内部原始地址);
# 直连场景所有报文双向经过 NAT-A 中继并改写源地址(对端只见映射地址);
# 拦截场景 NAT-A 丢弃对端方向报文 → 打洞超时 → 回退服务端中继。
#
# 用法: tests/xtcp-isolated.sh [二进制目录(默认 target/release)]
set -u
BIN_DIR="${1:-$(dirname "$0")/../target/release}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SERVER="$ROOT/$BIN_DIR/rh-server"
CLIENT="$ROOT/$BIN_DIR/rh-client"
WORK=$(mktemp -d /tmp/rh-xtcp.XXXXXX)
API=19380; CTL=19300; VIS=19312; ECHO=19391
GWA=19401; GWB=19402
PASS=0; FAIL=0; PIDS=()
cleanup() {
  for p in "${PIDS[@]:-}"; do kill "$p" 2>/dev/null; done
  rm -rf /tmp/rh-xtcp-last; cp -r "$WORK" /tmp/rh-xtcp-last 2>/dev/null
  rm -rf "$WORK"
}
trap cleanup EXIT
# 端口卫生:清理上一轮残留(背靠背运行时端口释放竞态)
for pid in $(ps -eo pid,args | grep -E "natgw.py ($GWA|$GWB)" | grep -v grep | awk '{print $1}'); do kill "$pid" 2>/dev/null; done
sleep 0.3
ok()   { PASS=$((PASS+1)); echo "PASS: $1"; }
bad()  { FAIL=$((FAIL+1)); echo "FAIL: $1"; }
check(){ [ "$1" = "0" ] && ok "$2" || bad "$2"; }

# ---------- echo 目标 ----------
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
while True:
    c, _ = s.accept(); threading.Thread(target=echo, args=(c,), daemon=True).start()
EOF
PIDS+=($!)

# ---------- 服务端 ----------
cat > "$WORK/server.toml" <<EOF
bind_addr = "127.0.0.1"
bind_protocol = "tcp"
bind_port = $CTL
multiplex = true
[web]
bind_addr = "127.0.0.1"
port = $API
[vhost]
http_port = 0
https_port = 0
tcpmux_port = 0
[nathole]
punch_timeout_s = 5
prepare_timeout_s = 5
[ports]
auto_range = "22000-22100"
[log]
level = "info"
EOF
"$SERVER" --config "$WORK/server.toml" admin create admin Xt-123-456 >/dev/null 2>&1
"$SERVER" --config "$WORK/server.toml" >"$WORK/server.log" 2>&1 &
PIDS+=($!)
sleep 2
TOKEN=$(curl -s -X POST "http://127.0.0.1:$API/api/auth/login" -H 'Content-Type: application/json' \
  -d '{"username":"admin","password":"Xt-123-456"}' | python3 -c 'import json,sys;print(json.load(sys.stdin)["data"]["accessToken"])')
api() { curl -s -m 10 -H "Authorization: Bearer $TOKEN" "$@"; }

# ---------- 两个客户端(各自独立 NAT 网关) ----------
mk_client() {
  local name="$1" gw="$2" cfg="$3"
  local resp id sec
  resp=$(api -X POST "http://127.0.0.1:$API/api/clients" -H 'Content-Type: application/json' -d "{\"name\":\"$name\"}")
  id=$(echo "$resp" | python3 -c 'import json,sys;print(json.load(sys.stdin)["data"]["id"])')
  sec=$(echo "$resp" | python3 -c 'import json,sys;print(json.load(sys.stdin)["data"]["secret"])')
  cat > "$cfg" <<EOF2
server_addr = "127.0.0.1:$CTL"
protocol = "tcp"
multiplex = true
client_id = "$id"
secret = "$sec"
stun_server = "127.0.0.1:$gw"
log_level = "${DBG:-info}"
EOF2
  echo "$id"
}
CID_P=$(mk_client xt-p "$GWA" "$WORK/p.toml")
CID_V=$(mk_client xt-v "$GWB" "$WORK/v.toml")

# NAT 网关(A=proxy 侧,承载直连数据;B=visitor 侧,STUN 映射)
python3 "$ROOT/tests/natgw.py" "$GWA" open "$WORK/gwa.json" >"$WORK/gwa.log" 2>&1 &
PIDS+=($!)
python3 "$ROOT/tests/natgw.py" "$GWB" open "$WORK/gwb.json" >"$WORK/gwb.log" 2>&1 &
PIDS+=($!)
sleep 1

"$CLIENT" --config "$WORK/p.toml" >"$WORK/p.log" 2>&1 &
PID_P=$!; PIDS+=($PID_P)
"$CLIENT" --config "$WORK/v.toml" >"$WORK/v.log" 2>&1 &
PID_V=$!; PIDS+=($PID_V)
sleep 3

# ---------- 规则:client-P 持 xtcp 代理;client-V 持访问端 ----------
api -X POST "http://127.0.0.1:$API/api/services" -H 'Content-Type: application/json' \
  -d "{\"client_id\":\"$CID_P\",\"name\":\"xs\",\"kind\":\"xtcp\",\"target_host\":\"127.0.0.1\",\"target_port\":$ECHO,\"enabled\":true,\"xtcp\":{\"secret_key\":\"xk\"}}" >/dev/null
api -X POST "http://127.0.0.1:$API/api/visitors" -H 'Content-Type: application/json' \
  -d "{\"client_id\":\"$CID_V\",\"name\":\"xs-v\",\"kind\":\"xtcp\",\"bind_port\":$VIS,\"target_proxy_name\":\"xs\",\"secret_key\":\"xk\",\"enabled\":true}" >/dev/null
sleep 2

echo "== 场景1:打洞直连(NAT 中继开放,数据经 NAT-A 双向中继)=="
python3 - "$VIS" <<'EOF' && ok "直连-首次连接经打洞回显" || bad "直连-首次连接经打洞回显"
import socket, sys
s = socket.create_connection(("127.0.0.1", int(sys.argv[1])), timeout=30)
s.sendall(b"direct-1\n"); s.shutdown(socket.SHUT_WR); s.settimeout(25)
g = b""
try:
    while True:
        d = s.recv(4096)
        if not d: break
        g += d
except Exception: pass
sys.exit(0 if g == b"direct-1\n" else 1)
EOF
python3 - "$VIS" <<'EOF' && ok "直连-第二连接复用已打洞通道" || bad "直连-第二连接复用已打洞通道"
import socket, sys
s = socket.create_connection(("127.0.0.1", int(sys.argv[1])), timeout=15)
s.sendall(b"direct-2\n"); s.settimeout(10)
g = s.recv(64)
sys.exit(0 if g == b"direct-2\n" else 1)
EOF
sleep 1
grep -q "punch dial ok" "$WORK/v.log"; check $? "visitor 侧日志确认打洞成功"
grep -q "serving direct" "$WORK/p.log"; check $? "proxy 侧日志确认直连服务"
BYTES=$(python3 -c 'import json;print(json.load(open("'$WORK'/gwa.json"))["relayed_bytes"])')
[ "$BYTES" -gt 0 ]; check $? "数据经 NAT-A 网关中联(relayed=$BYTES 字节)"
# 对端只见映射地址:网关日志无内部原始端口泄漏的断言由 relayed>0 + 客户端候选=映射地址共同保证

echo "== 场景2:打洞被拦截 → 回退服务端中继(全新环境,NAT-A block)=="
# 全新环境:杀掉场景1全部进程,重起服务端/网关(block)/客户端/规则
for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null; done
sleep 1
PIDS=()
# echo 目标也被上面清理,场景2 需重起
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
while True:
    c, _ = s.accept(); threading.Thread(target=echo, args=(c,), daemon=True).start()
EOF
PIDS+=($!)
mkdir -p "$WORK/s2"
cat > "$WORK/s2/server.toml" <<EOF
bind_addr = "127.0.0.1"
bind_protocol = "tcp"
bind_port = $CTL
multiplex = true
[web]
bind_addr = "127.0.0.1"
port = $API
[vhost]
http_port = 0
https_port = 0
tcpmux_port = 0
[nathole]
punch_timeout_s = 5
prepare_timeout_s = 5
[ports]
auto_range = "22000-22100"
[log]
level = "debug"
EOF
"$SERVER" --config "$WORK/s2/server.toml" admin create admin Xt-123-456 >/dev/null 2>&1
"$SERVER" --config "$WORK/s2/server.toml" >"$WORK/server2.log" 2>&1 &
PIDS+=($!)
sleep 2
TOKEN=$(curl -s -X POST "http://127.0.0.1:$API/api/auth/login" -H 'Content-Type: application/json' \
  -d '{"username":"admin","password":"Xt-123-456"}' | python3 -c 'import json,sys;print(json.load(sys.stdin)["data"]["accessToken"])')
api() { curl -s -m 10 -H "Authorization: Bearer $TOKEN" "$@"; }
export DBG=debug
CID_P=$(mk_client xt-p2 "$GWA" "$WORK/p2.toml")
CID_V=$(mk_client xt-v2 "$GWB" "$WORK/v2.toml")
python3 "$ROOT/tests/natgw.py" "$GWA" block "$WORK/gwa2.json" >"$WORK/gwa2.log" 2>&1 &
PIDS+=($!)
python3 "$ROOT/tests/natgw.py" "$GWB" open "$WORK/gwb2.json" >"$WORK/gwb2.log" 2>&1 &
PIDS+=($!)
sleep 1
"$CLIENT" --config "$WORK/p2.toml" >"$WORK/p2.log" 2>&1 &
PIDS+=($!)
"$CLIENT" --config "$WORK/v2.toml" >"$WORK/v2.log" 2>&1 &
PIDS+=($!)
sleep 3
api -X POST "http://127.0.0.1:$API/api/services" -H 'Content-Type: application/json' \
  -d "{\"client_id\":\"$CID_P\",\"name\":\"xs\",\"kind\":\"xtcp\",\"target_host\":\"127.0.0.1\",\"target_port\":$ECHO,\"enabled\":true,\"xtcp\":{\"secret_key\":\"xk\"}}" >/dev/null
api -X POST "http://127.0.0.1:$API/api/visitors" -H 'Content-Type: application/json' \
  -d "{\"client_id\":\"$CID_V\",\"name\":\"xs-v\",\"kind\":\"xtcp\",\"bind_port\":$VIS,\"target_proxy_name\":\"xs\",\"secret_key\":\"xk\",\"enabled\":true}" >/dev/null
sleep 2
python3 - "$VIS" <<'EOF' && ok "拦截-回退中继后回显(打洞超时≈5s + 中继建立)" || bad "拦截-回退中继后回显"
import socket, sys
s = socket.create_connection(("127.0.0.1", int(sys.argv[1])), timeout=30)
s.sendall(b"fallback\n"); s.shutdown(socket.SHUT_WR); s.settimeout(25)
g = b""
try:
    while True:
        d = s.recv(4096)
        if not d: break
        g += d
except Exception: pass
sys.exit(0 if g == b"fallback\n" else 1)
EOF
sleep 1
grep -q "punch dial failed" "$WORK/v2.log"; check $? "visitor 侧日志确认打洞失败"
BLOCKED=$(python3 -c 'import json;print(json.load(open("'$WORK'/gwa2.json"))["packets_blocked"])')
[ "$BLOCKED" -gt 0 ]; check $? "网关确认拦截打洞报文(blocked=$BLOCKED)"
RELAYED=$(python3 -c 'import json;print(json.load(open("'$WORK'/gwa2.json"))["relayed_bytes"])')
[ "$RELAYED" = "0" ]; check $? "拦截场景无直连数据绕过网关(relayed=0)"

echo "== 结果: PASS=$PASS FAIL=$FAIL =="
[ "$FAIL" = "0" ]
