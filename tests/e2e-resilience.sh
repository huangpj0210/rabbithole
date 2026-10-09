#!/usr/bin/env bash
# rabbithole 韧性场景 E2E(§六.C 补充场景,独立环境,不依赖 tests/e2e.sh)。
# 覆盖:目标拒连 / DB 故障与回滚 / 仅数据通道断开 / 迟到连接 / 旧 ACK / 同一客户端重复上线。
# 用法: tests/e2e-resilience.sh [二进制目录(默认 target/release)]
set -u
BIN_DIR="${1:-$(dirname "$0")/../target/release}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SERVER="$ROOT/$BIN_DIR/rh-server"
CLIENT="$ROOT/$BIN_DIR/rh-client"
WORK=$(mktemp -d /tmp/rh-resil.XXXXXX)
API=26780; CTL=26700; ECHO=26891; P_TCP=26801; P_REFUSED=26805; DEAD=26999
MM_PORT=26901; P_MM=26911
PASS=0; FAIL=0; PIDS=()
cleanup() {
  for p in "${PIDS[@]:-}"; do kill "$p" 2>/dev/null; done
  rm -rf /tmp/rh-resil-last; cp -r "$WORK" /tmp/rh-resil-last 2>/dev/null
  rm -rf "$WORK"
}
trap cleanup EXIT
ok()   { PASS=$((PASS+1)); echo "PASS: $1"; }
bad()  { FAIL=$((FAIL+1)); echo "FAIL: $1"; }
check(){ [ "$1" = "0" ] && ok "$2" || bad "$2"; }

echo "== rabbithole 韧性 E2E($WORK)=="

# 全局卫生:清理先前运行残留的同端口进程(其他 WORK 目录的孤儿)
for pid in $(ps -eo pid,args | grep -E "rh-(client|server) --config /tmp/rh-" | grep -v grep | grep -v "$WORK" | awk '{print $1}'); do kill "$pid" 2>/dev/null; done
sleep 0.5

# ---------- 基础:echo / 哑服务端 / 管理端 ----------
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
    finally: c.close()  # 必须显式关闭:Thread._args 会延迟 GC,fd 不关则客户端永远收不到 FIN
while True:
    c, _ = s.accept(); threading.Thread(target=echo, args=(c,), daemon=True).start()
EOF
PIDS+=($!)

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
[ports]
auto_range = "27000-27100"
[log]
level = "debug"
EOF
"$SERVER" --config "$WORK/server.toml" admin create admin Res-123-456 >/dev/null 2>&1
"$SERVER" --config "$WORK/server.toml" >"$WORK/server.log" 2>&1 &
SRV=$!; PIDS+=($SRV)
sleep 2
login() { curl -s -X POST "http://127.0.0.1:$API/api/auth/login" -H 'Content-Type: application/json' -d '{"username":"admin","password":"Res-123-456"}' | python3 -c 'import json,sys;print(json.load(sys.stdin)["data"]["accessToken"])'; }
TOKEN=$(login)
api() { curl -s -m 12 -H "Authorization: Bearer $TOKEN" "$@"; }

CR=$(api -X POST "http://127.0.0.1:$API/api/clients" -H 'Content-Type: application/json' -d '{"name":"main"}')
CID=$(echo "$CR"|python3 -c 'import json,sys;print(json.load(sys.stdin)["data"]["id"])')
SEC=$(echo "$CR"|python3 -c 'import json,sys;print(json.load(sys.stdin)["data"]["secret"])')
cat > "$WORK/client.toml" <<EOF2
server_addr = "127.0.0.1:$CTL"
protocol = "tcp"
multiplex = true
client_id = "$CID"
secret = "$SEC"
log_level = "info"
EOF2
"$CLIENT" --config "$WORK/client.toml" >"$WORK/client.log" 2>&1 &
MAIN=$!; PIDS+=($MAIN)
api -X POST "http://127.0.0.1:$API/api/services" -H 'Content-Type: application/json' \
  -d "{\"client_id\":\"$CID\",\"name\":\"tcp\",\"kind\":\"tcp\",\"bind_port\":$P_TCP,\"target_host\":\"127.0.0.1\",\"target_port\":$ECHO,\"enabled\":true}" >/dev/null
sleep 3
online=$(api "http://127.0.0.1:$API/api/clients" | python3 -c 'import json,sys;print(json.load(sys.stdin)["data"][0]["online"])')
[ "$online" = "True" ]; check $? "主客户端在线"
fwd() {
python3 - "$1" <<'EOF'
import socket, sys
try:
    s = socket.create_connection(("127.0.0.1", int(sys.argv[1])), timeout=10)
    s.sendall(b"ping\n"); s.shutdown(socket.SHUT_WR); s.settimeout(8)
    g = b""
    while True:
        d = s.recv(256)
        if not d: break
        g += d
    print("FWD:", g, file=sys.stderr)
    sys.exit(0 if g == b"ping\n" else 1)
except Exception as e:
    print("FWD_ERR:", e, file=sys.stderr); sys.exit(1)
EOF
}
fwd $P_TCP; rc=$?; check $rc "基线转发"
api "http://127.0.0.1:$API/api/services?client_id=$CID" | python3 -c 'import json,sys;print("POOLDBG:",[(x["name"],x["status"],x["pool_ready"]) for x in json.load(sys.stdin)["data"]])' >&2

# ---------- 1) 目标拒连 ----------
api -X POST "http://127.0.0.1:$API/api/services" -H 'Content-Type: application/json' \
  -d "{\"client_id\":\"$CID\",\"name\":\"dead\",\"kind\":\"tcp\",\"bind_port\":$P_REFUSED,\"target_host\":\"127.0.0.1\",\"target_port\":$DEAD,\"enabled\":true,\"pool_size\":1}" >/dev/null
sleep 2
python3 - "$P_REFUSED" <<'EOF' && ok "目标拒连:用户连接被快速干净关闭" || bad "目标拒连:用户连接被快速干净关闭"
import socket, sys, time
s = socket.create_connection(("127.0.0.1", int(sys.argv[1])), timeout=10)
s.settimeout(15); t0 = time.time(); got = b""
try:
    while True:
        d = s.recv(4096)
        if not d: break
        got += d
except Exception: pass
sys.exit(0 if got == b"" and time.time() - t0 < 15 else 1)
EOF
st=""
for i in 1 2 3 4 5 6; do
  st=$(api "http://127.0.0.1:$API/api/services?client_id=$CID" | python3 -c 'import json,sys;print([s["status"] for s in json.load(sys.stdin)["data"] if s["name"]=="dead"][0])' 2>/dev/null)
  [ "$st" = "running" ] && break; sleep 1
done
[ "$st" = "running" ]; check $? "目标拒连:规则状态不受影响($st)"
fwd $P_TCP; check $? "目标拒连后其他规则转发正常"

# ---------- 2) DB 故障与回滚(EXCLUSIVE 锁注入) ----------
DBF="$WORK/rabbithole.db"
[ -f "$DBF" ]; check $? "定位 SQLite"
python3 - "$DBF" <<'EOF' &
import sqlite3, sys, time
c = sqlite3.connect(sys.argv[1], timeout=25, isolation_level=None)
c.execute("BEGIN EXCLUSIVE"); time.sleep(8); c.execute("ROLLBACK")
EOF
LOCKPID=$!
sleep 1.5
SVC_ID=$(api "http://127.0.0.1:$API/api/services" | python3 -c 'import json,sys;print([s["id"] for s in json.load(sys.stdin)["data"] if s["name"]=="tcp"][0])')
ucode=$(api -X PUT "http://127.0.0.1:$API/api/services/$SVC_ID" -H 'Content-Type: application/json' \
  -d "{\"client_id\":\"$CID\",\"name\":\"tcp\",\"kind\":\"tcp\",\"bind_port\":$P_TCP,\"target_host\":\"127.0.0.1\",\"target_port\":$ECHO,\"enabled\":true,\"pool_size\":9}" | python3 -c 'import json,sys;print(json.load(sys.stdin)["code"])')
[ "$ucode" != "0" ]; check $? "DB 故障时更新被拒绝(code=$ucode)"
fwd $P_TCP; check $? "DB 故障期间原转发不受影响(运行态零扰动)"
wait $LOCKPID 2>/dev/null
sleep 1
rcode=$(api -X PUT "http://127.0.0.1:$API/api/services/$SVC_ID" -H 'Content-Type: application/json' \
  -d "{\"client_id\":\"$CID\",\"name\":\"tcp\",\"kind\":\"tcp\",\"bind_port\":$P_TCP,\"target_host\":\"127.0.0.1\",\"target_port\":$ECHO,\"enabled\":true,\"pool_size\":9}" | python3 -c 'import json,sys;print(json.load(sys.stdin)["code"])')
[ "$rcode" = "0" ]; check $? "DB 恢复后更新成功"
sleep 2; fwd $P_TCP; check $? "更新后转发正常"

# ---------- 3) 仅数据通道断开(multiplex=false + 中间人) ----------
CR2=$(api -X POST "http://127.0.0.1:$API/api/clients" -H 'Content-Type: application/json' -d '{"name":"mm"}')
CID2=$(echo "$CR2"|python3 -c 'import json,sys;print(json.load(sys.stdin)["data"]["id"])')
SEC2=$(echo "$CR2"|python3 -c 'import json,sys;print(json.load(sys.stdin)["data"]["secret"])')
MM_DIR="$WORK/mmcmd"; mkdir -p "$MM_DIR"
python3 "$ROOT/tests/tcpkill-mm.py" $MM_PORT 127.0.0.1 $CTL "$MM_DIR" >"$WORK/mm.log" 2>&1 &
MMPID=$!; PIDS+=($MMPID)
sleep 0.5
cat > "$WORK/client-mm.toml" <<EOF2
server_addr = "127.0.0.1:$MM_PORT"
protocol = "tcp"
multiplex = false
client_id = "$CID2"
secret = "$SEC2"
log_level = "info"
EOF2
"$CLIENT" --config "$WORK/client-mm.toml" >"$WORK/client-mm.log" 2>&1 &
CLIMM=$!; PIDS+=($CLIMM)
api -X POST "http://127.0.0.1:$API/api/services" -H 'Content-Type: application/json' \
  -d "{\"client_id\":\"$CID2\",\"name\":\"mmtcp\",\"kind\":\"tcp\",\"bind_port\":$P_MM,\"target_host\":\"127.0.0.1\",\"target_port\":$ECHO,\"enabled\":true,\"pool_size\":1}" >/dev/null
sleep 3
python3 - "$P_MM" <<'EOF' && ok "仅数据通道断开:断开前转发正常(裸模式)" || bad "仅数据通道断开:断开前转发正常(裸模式)"
import socket, sys
s = socket.create_connection(("127.0.0.1", int(sys.argv[1])), timeout=10)
s.sendall(b"mm1\n"); s.settimeout(8)
sys.exit(0 if s.recv(256) == b"mm1\n" else 1)
EOF
touch "$MM_DIR/kill_data"; sleep 2
python3 - "$P_MM" <<'EOF' && ok "仅数据通道断开:控制会话保持,新连接重建通道恢复" || bad "仅数据通道断开:控制会话保持,新连接重建通道恢复"
import socket, sys, time
for _ in range(4):
    try:
        s = socket.create_connection(("127.0.0.1", int(sys.argv[1])), timeout=10)
        s.sendall(b"mm2\n"); s.settimeout(8)
        if s.recv(256) == b"mm2\n": sys.exit(0)
    except Exception: pass
    time.sleep(2)
sys.exit(1)
EOF
mmonline=$(api "http://127.0.0.1:$API/api/clients" | python3 -c 'import json,sys;print([c["online"] for c in json.load(sys.stdin)["data"] if c["name"]=="mm"][0])')
[ "$mmonline" = "True" ]; check $? "仅数据通道断开:mm 客户端控制会话未被波及"

# ---------- 4) 迟到连接(扣留 HELLO → 规则停用失效 → 放行被拒 → 恢复) ----------
touch "$MM_DIR/hold"; sleep 0.2
python3 - "$P_MM" >"$WORK/lateconn.out" 2>&1 <<'EOF' &
import socket, sys, time
s = socket.create_connection(("127.0.0.1", int(sys.argv[1])), timeout=20)
s.sendall(b"late\n"); s.settimeout(15)
g = b""
try:
    while True:
        d = s.recv(256)
        if not d: break
        g += d
except Exception: pass
sys.exit(0 if g == b"late\n" else 1)
EOF
LATEPID=$!
sleep 2
MM_SVC=$(api "http://127.0.0.1:$API/api/services" | python3 -c 'import json,sys;print([s["id"] for s in json.load(sys.stdin)["data"] if s["name"]=="mmtcp"][0])')
api -X POST "http://127.0.0.1:$API/api/services/$MM_SVC/disable" >/dev/null
sleep 1
touch "$MM_DIR/release"
sleep 2
api -X POST "http://127.0.0.1:$API/api/services/$MM_SVC/enable" >/dev/null
sleep 2
wait $LATEPID; check $? "迟到连接被拒后规则恢复,新转发可用"
rm -f "$MM_DIR/hold.done" "$MM_DIR/release.done" "$MM_DIR/kill_data.done"
kill $CLIMM 2>/dev/null; kill $MMPID 2>/dev/null
sleep 1

# ---------- 5) 旧 ACK 迟到(冻结客户端跨两个版本) ----------
kill -STOP $MAIN
# 目标切到不存在的端口(旧版本),再切回 echo(新版本);期间客户端冻结,ACK 迟到
api -X PUT "http://127.0.0.1:$API/api/services/$SVC_ID" -H 'Content-Type: application/json' \
  -d "{\"client_id\":\"$CID\",\"name\":\"tcp\",\"kind\":\"tcp\",\"bind_port\":$P_TCP,\"target_host\":\"127.0.0.1\",\"target_port\":$DEAD,\"enabled\":true}" >/dev/null
sleep 1
api -X PUT "http://127.0.0.1:$API/api/services/$SVC_ID" -H 'Content-Type: application/json' \
  -d "{\"client_id\":\"$CID\",\"name\":\"tcp\",\"kind\":\"tcp\",\"bind_port\":$P_TCP,\"target_host\":\"127.0.0.1\",\"target_port\":$ECHO,\"enabled\":true}" >/dev/null
sleep 1
kill -CONT $MAIN
sleep 5
st=""
for i in 1 2 3 4 5 6; do
  st=$(api "http://127.0.0.1:$API/api/services?client_id=$CID" | python3 -c 'import json,sys;print([s["status"] for s in json.load(sys.stdin)["data"] if s["name"]=="tcp"][0])' 2>/dev/null)
  [ "$st" = "running" ] && break; sleep 1
done
[ "$st" = "running" ]; check $? "旧 ACK 迟到:最终收敛 running(不回退)"
fwd $P_TCP; check $? "旧 ACK 迟到:转发跟随最终目标"

# ---------- 6) 同一客户端重复上线 ----------
# 先冻结原进程(连接保持 ESTABLISHED 但不再发包)再启动同 ID 新进程:
# 保证服务端看到"同一 client_id 双活会话"并记录 session replaced,且不会引发双方重连互顶。
kill -STOP $MAIN
cp "$WORK/client.toml" "$WORK/client2.toml"
"$CLIENT" --config "$WORK/client2.toml" >"$WORK/client2.log" 2>&1 &
CLI2=$!; PIDS+=($CLI2)
sleep 4
grep -q "session replaced" "$WORK/server.log"; check $? "重复上线:服务端记录会话替换"
for i in $(seq 1 10); do
  online=$(api "http://127.0.0.1:$API/api/clients" | python3 -c 'import json,sys;print([(c["name"],c["online"]) for c in json.load(sys.stdin)["data"] if c["name"]=="main"][0][1])' 2>/dev/null)
  [ "$online" = "True" ] && break; sleep 1
done
[ "$online" = "True" ]; check $? "重复上线:新进程会话在线"
fwd $P_TCP; check $? "重复上线:会话替换后转发可用(经新进程)"
kill -9 $MAIN 2>/dev/null   # 冻结中的旧进程:SIGKILL 可直接终止,防止 CONT 后反扑替换新会话
kill $CLI2 2>/dev/null
sleep 2

echo
echo "== 结果: PASS=$PASS FAIL=$FAIL =="
[ "$FAIL" = "0" ]
