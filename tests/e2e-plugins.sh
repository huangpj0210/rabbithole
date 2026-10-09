#!/usr/bin/env bash
# rabbithole 客户端插件 E2E(七个无外部依赖插件):
#   http_proxy / socks5 / https2http / https2https / http2https / http2http / tls2raw
# 每插件一条规则,经 服务端→客户端→插件→本地辅助服务 全链路验证语义:
#   - 改写类(http2http/http2https/https2http/https2https):上游记录收到的请求头,
#     断言 Host 重写 / HeaderOp(set 后 del)/ 响应回传
#   - tls2raw:TLS 入 → 裸 echo 上游回显
#   - http_proxy/socks5:本地代理辅助服务断言握手(CONNECT/Basic 认证;SOCKS5
#     方法协商/用户密码/域名目标)后建立隧道,载荷经隧道回显
# 用法: tests/e2e-plugins.sh [二进制目录(默认 target/release)]
set -u
BIN_DIR="${1:-$(dirname "$0")/../target/release}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SERVER="$ROOT/$BIN_DIR/rh-server"
CLIENT="$ROOT/$BIN_DIR/rh-client"
WORK=$(mktemp -d /tmp/rh-plugins.XXXXXX)
API=30580; CTL=30500; ECHO=30501; HDR=30502; HDR_TLS=30503; HPROXY=30504; SOCKS=30505
PASS=0; FAIL=0; PIDS=()
cleanup(){
  for p in "${PIDS[@]:-}"; do kill "$p" 2>/dev/null; done
  rm -rf /tmp/rh-plugins-last; cp -r "$WORK" /tmp/rh-plugins-last 2>/dev/null
  rm -rf "$WORK"
}
trap cleanup EXIT
ok()   { PASS=$((PASS+1)); echo "PASS: $1"; }
bad()  { FAIL=$((FAIL+1)); echo "FAIL: $1"; }
check(){ [ "$1" = "0" ] && ok "$2" || bad "$2"; }

echo "== rabbithole 插件 E2E($WORK)=="
for pid in $(ps -eo pid,args | grep -E "rh-(client|server) --config /tmp/rh-plugins" | grep -v grep | awk '{print $1}'); do kill "$pid" 2>/dev/null; done

# ---------- 证书(插件 TLS 侧 + TLS 上游辅助) ----------
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$WORK/plugin.key" -out "$WORK/plugin.crt" \
  -days 2 -subj "/CN=local.test" -addext "subjectAltName=DNS:local.test,IP:127.0.0.1" >/dev/null 2>&1
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$WORK/up.key" -out "$WORK/up.crt" \
  -days 2 -subj "/CN=up.test" -addext "subjectAltName=DNS:up.test,IP:127.0.0.1" >/dev/null 2>&1

# ---------- 辅助服务(全部显式 close,避免依赖 GC 释放 fd) ----------
python3 - "$ECHO" <<'EOF' &
import socket, threading, sys
s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("127.0.0.1", int(sys.argv[1]))); s.listen(64)
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

# 请求头记录型 HTTP 上游(明文/TLS 同一实现;记录完整请求头到 $LOG)
python3 - "$WORK/hdr-plain.log" "$HDR" "plain" <<'EOF' &
import socket, ssl, threading, sys
LOG, PORT, MODE = sys.argv[1], int(sys.argv[2]), sys.argv[3]
s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("127.0.0.1", PORT)); s.listen(64)
if MODE == "tls":
    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    ctx.load_cert_chain(sys.argv[4], sys.argv[5])
    s = ctx.wrap_socket(s, server_side=True)
def serve(c):
    try:
        buf = b""
        while b"\r\n\r\n" not in buf:
            d = c.recv(65536)
            if not d: return
            buf += d
        with open(LOG, "ab") as f:
            f.write(b"---REQ---\n" + buf.split(b"\r\n\r\n", 1)[0] + b"\n")
        c.sendall(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
    except OSError: pass
    finally: c.close()
while True:
    c, _ = s.accept(); threading.Thread(target=serve, args=(c,), daemon=True).start()
EOF
PIDS+=($!)
python3 - "$WORK/hdr-tls.log" "$HDR_TLS" "tls" "$WORK/up.crt" "$WORK/up.key" <<'EOF' &
import socket, ssl, threading, sys
LOG, PORT, MODE = sys.argv[1], int(sys.argv[2]), sys.argv[3]
s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("127.0.0.1", PORT)); s.listen(64)
if MODE == "tls":
    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    ctx.load_cert_chain(sys.argv[4], sys.argv[5])
    s = ctx.wrap_socket(s, server_side=True)
def serve(c):
    try:
        buf = b""
        while b"\r\n\r\n" not in buf:
            d = c.recv(65536)
            if not d: return
            buf += d
        with open(LOG, "ab") as f:
            f.write(b"---REQ---\n" + buf.split(b"\r\n\r\n", 1)[0] + b"\n")
        c.sendall(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
    except OSError: pass
    finally: c.close()
while True:
    c, _ = s.accept(); threading.Thread(target=serve, args=(c,), daemon=True).start()
EOF
PIDS+=($!)

# 本地 HTTP 正向代理:校验 Basic 凭据后隧道到固定 echo(记录 CONNECT 行与凭据)
python3 - "$HPROXY" "$WORK/hproxy.log" "$ECHO" <<'EOF' &
import socket, threading, base64, sys
PORT, LOG, ECHO = int(sys.argv[1]), sys.argv[2], int(sys.argv[3])
s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("127.0.0.1", PORT)); s.listen(64)
def pump(a, b):
    try:
        while True:
            d = a.recv(65536)
            if not d: break
            b.sendall(d)
    except OSError: pass
    finally:
        try: a.close()
        except OSError: pass
def serve(c):
    try:
        buf = b""
        while b"\r\n\r\n" not in buf:
            d = c.recv(65536)
            if not d: c.close(); return
            buf += d
        head = buf.split(b"\r\n\r\n", 1)[0].decode("latin1")
        with open(LOG, "a") as f: f.write("---CONNECT---\n" + head + "\n")
        auth = ""
        for l in head.split("\r\n"):
            if l.lower().startswith("proxy-authorization:"):
                auth = l.split(":", 1)[1].strip()
        userpass = ""
        if auth.startswith("Basic "):
            try: userpass = base64.b64decode(auth[6:]).decode()
            except Exception: pass
        with open(LOG, "a") as f: f.write("AUTH=" + userpass + "\n")
        if userpass != "pu:pp":
            c.sendall(b"HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: Basic\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            c.close(); return
        c.sendall(b"HTTP/1.1 200 Connection established\r\n\r\n")
        up = socket.create_connection(("127.0.0.1", ECHO))
        if buf.split(b"\r\n\r\n", 1)[1]:
            up.sendall(buf.split(b"\r\n\r\n", 1)[1])
        threading.Thread(target=pump, args=(c, up), daemon=True).start()
        pump(up, c)
    except OSError: pass
    finally:
        try: c.close()
        except OSError: pass
while True:
    c, _ = s.accept(); threading.Thread(target=serve, args=(c,), daemon=True).start()
EOF
PIDS+=($!)

# 本地 SOCKS5 服务:方法协商/用户密码校验/域名目标断言,成功后隧道到固定 echo
python3 - "$SOCKS" "$WORK/socks.log" "$ECHO" "$SOCKS" <<'EOF' &
import socket, threading, sys
PORT, LOG, ECHO = int(sys.argv[1]), sys.argv[2], int(sys.argv[3])
SocksSelfPort = int(sys.argv[4])
s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("127.0.0.1", PORT)); s.listen(64)
def pump(a, b):
    try:
        while True:
            d = a.recv(65536)
            if not d: break
            b.sendall(d)
    except OSError: pass
    finally:
        try: a.close()
        except OSError: pass
def serve(c):
    log = []
    try:
        greet = c.recv(2)
        nmethods = greet[1]
        methods = c.recv(nmethods)
        log.append("methods=" + ",".join(str(m) for m in methods))
        if 0x02 in methods:
            c.sendall(b"\x05\x02")
            ver = c.recv(1); ulen = c.recv(1)[0]; user = c.recv(ulen)
            plen = c.recv(1)[0]; pwd = c.recv(plen)
            creds = user.decode() + ":" + pwd.decode()
            log.append("auth=" + creds)
            if creds != "su:sp":
                c.sendall(b"\x01\x01"); c.close(); return
            c.sendall(b"\x01\x00")
        else:
            c.sendall(b"\x05\x00")
        head = c.recv(4)
        atyp = head[3]
        if atyp == 0x03:
            dl = c.recv(1)[0]; dom = c.recv(dl)
        elif atyp == 0x01:
            dom = c.recv(4)
        else:
            dom = c.recv(16)
        port = int.from_bytes(c.recv(2), "big")
        log.append("target=" + (dom.decode() if isinstance(dom, bytes) and atyp == 0x03 else str(dom)) + ":" + str(port))
        with open(LOG, "a") as f: f.write("---SOCKS---\n" + "\n".join(log) + "\n")
        c.sendall(b"\x05\x00\x00\x01" + b"\x7f\x00\x00\x01" + port.to_bytes(2, "big"))
        up = socket.create_connection(("127.0.0.1", ECHO))
        threading.Thread(target=pump, args=(c, up), daemon=True).start()
        pump(up, c)
    except OSError: pass
    finally:
        try: c.close()
        except OSError: pass
while True:
    c, _ = s.accept(); threading.Thread(target=serve, args=(c,), daemon=True).start()
EOF
PIDS+=($!)

# ---------- server / client ----------
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
auto_range = "30600-30700"
[log]
level = "info"
EOF
"$SERVER" --config "$WORK/server.toml" admin create admin Pl-123-456 >/dev/null 2>&1
"$SERVER" --config "$WORK/server.toml" >"$WORK/server.log" 2>&1 &
PIDS+=($!)
sleep 2
TOKEN=$(curl -s -X POST "http://127.0.0.1:$API/api/auth/login" -H 'Content-Type: application/json' -d '{"username":"admin","password":"Pl-123-456"}' | python3 -c 'import json,sys;print(json.load(sys.stdin)["data"]["accessToken"])')
api(){ curl -s -m 15 -H "Authorization: Bearer $TOKEN" "$@"; }
CR=$(api -X POST "http://127.0.0.1:$API/api/clients" -H 'Content-Type: application/json' -d '{"name":"main"}')
CID=$(echo "$CR"|python3 -c 'import json,sys;print(json.load(sys.stdin)["data"]["id"])')
SEC=$(echo "$CR"|python3 -c 'import json,sys;print(json.load(sys.stdin)["data"]["secret"])')
cat > "$WORK/client.toml" <<EOF
server_addr = "127.0.0.1:$CTL"
protocol = "tcp"
multiplex = true
client_id = "$CID"
secret = "$SEC"
log_level = "info"
EOF
"$CLIENT" --config "$WORK/client.toml" >"$WORK/client.log" 2>&1 &
PIDS+=($!)
for i in $(seq 1 20); do
  on=$(api "http://127.0.0.1:$API/api/clients" | python3 -c 'import json,sys;print(json.load(sys.stdin)["data"][0]["online"])' 2>/dev/null)
  [ "$on" = "True" ] && break; sleep 0.5
done
[ "$on" = "True" ]; check $? "客户端上线"

mk_svc(){ r=$(api -X POST "http://127.0.0.1:$API/api/services" -H 'Content-Type: application/json' -d "$1"); code=$(echo "$r"|python3 -c 'import json,sys;d=json.load(sys.stdin);print(d.get("code"))' 2>/dev/null); [ "$code" = "0" ] || echo "MK_SVC_FAIL: $r" >&2; }
wait_rule(){ # $1 name → 等 running
  for i in $(seq 1 12); do
    st=$(api "http://127.0.0.1:$API/api/services?client_id=$CID" | python3 -c "import json,sys;print([s['status'] for s in json.load(sys.stdin)['data'] if s['name']=='$1'][0])" 2>/dev/null)
    [ "$st" = "running" ] && return 0; sleep 0.5
  done
  return 1
}
HDRASSERT='import sys
log = open(sys.argv[1], "rb").read().decode("latin1")
last = log.rsplit("---REQ---\n", 1)[1] if "---REQ---" in log else ""
checks = sys.argv[2:]
ok_all = True
for i in range(0, len(checks), 2):
    key, want = checks[i], checks[i+1]
    if want.startswith("!"):
        found = key in last
        if key == "X-Del" or key == "Connection":
            # 头存在性检查:精确按行匹配
            found = any(l.startswith(key + ":") for l in last.split("\r\n"))
        if found: print("FAIL: %s should be absent/gone" % key); ok_all = False
    else:
        if key + ": " + want not in last:
            print("FAIL: %s: %s not found; head=%r" % (key, want, last)); ok_all = False
sys.exit(0 if ok_all else 1)'

# ---------- 1) http2http:头改写(Host 重写 + set + del) ----------
mk_svc "{\"client_id\":\"$CID\",\"name\":\"h2h\",\"kind\":\"tcp\",\"bind_port\":30511,\"target_host\":\"127.0.0.1\",\"target_port\":1,\"enabled\":true,\"plugin\":{\"kind\":\"http2_http\",\"local_addr\":\"127.0.0.1:$HDR\",\"host_rewrite\":\"rewritten.example\",\"request_headers\":{\"set\":[{\"name\":\"X-Rew\",\"value\":\"on\"}],\"del\":[\"X-Del\"]}}}"
wait_rule h2h
body=$(curl -s -m 8 -H "X-Del: yes" "http://127.0.0.1:30511/first")
[ "$body" = "OK" ]; check $? "http2http:响应回传($body)"
python3 -c "$HDRASSERT" "$WORK/hdr-plain.log" Host "rewritten.example" X-Rew "on" X-Del "!"; check $? "http2http:Host 重写+set+del 生效"

# ---------- 2) http2https:明文入 → TLS 上游 ----------
mk_svc "{\"client_id\":\"$CID\",\"name\":\"h2s\",\"kind\":\"tcp\",\"bind_port\":30512,\"target_host\":\"127.0.0.1\",\"target_port\":1,\"enabled\":true,\"plugin\":{\"kind\":\"http2_https\",\"local_addr\":\"127.0.0.1:$HDR_TLS\",\"host_rewrite\":\"secure.up.test\"}}"
wait_rule h2s
body=$(curl -s -m 8 "http://127.0.0.1:30512/second")
[ "$body" = "OK" ]; check $? "http2https:TLS 上游响应回传($body)"
python3 -c "$HDRASSERT" "$WORK/hdr-tls.log" Host "secure.up.test"; check $? "http2https:经 TLS 到达上游且 Host 已重写"

# ---------- 3) https2http:TLS 终结(插件证书)→ 明文上游 ----------
mk_svc "{\"client_id\":\"$CID\",\"name\":\"s2h\",\"kind\":\"tcp\",\"bind_port\":30513,\"target_host\":\"127.0.0.1\",\"target_port\":1,\"enabled\":true,\"plugin\":{\"kind\":\"https2_http\",\"local_addr\":\"127.0.0.1:$HDR\",\"crt_path\":\"$WORK/plugin.crt\",\"key_path\":\"$WORK/plugin.key\",\"host_rewrite\":\"term.example\",\"request_headers\":{\"set\":[{\"name\":\"X-Tls\",\"value\":\"terminated\"}]}}}"
wait_rule s2h
body=$(curl -sk -m 8 "https://127.0.0.1:30513/third")
[ "$body" = "OK" ]; check $? "https2http:TLS 入响应回传($body)"
python3 -c "$HDRASSERT" "$WORK/hdr-plain.log" Host "term.example" X-Tls "terminated"; check $? "https2http:TLS 终结+Host 重写+注入头"

# ---------- 4) https2https:TLS 终结 → TLS 上游 ----------
mk_svc "{\"client_id\":\"$CID\",\"name\":\"s2s\",\"kind\":\"tcp\",\"bind_port\":30514,\"target_host\":\"127.0.0.1\",\"target_port\":1,\"enabled\":true,\"plugin\":{\"kind\":\"https2_https\",\"local_addr\":\"127.0.0.1:$HDR_TLS\",\"crt_path\":\"$WORK/plugin.crt\",\"key_path\":\"$WORK/plugin.key\",\"host_rewrite\":\"both.example\"}}"
wait_rule s2s
body=$(curl -sk -m 8 "https://127.0.0.1:30514/fourth")
[ "$body" = "OK" ]; check $? "https2https:双 TLS 链路响应回传($body)"
python3 -c "$HDRASSERT" "$WORK/hdr-tls.log" Host "both.example"; check $? "https2https:上游收到重写后请求"

# ---------- 5) tls2raw:TLS 入 → 裸 echo 上游 ----------
mk_svc "{\"client_id\":\"$CID\",\"name\":\"t2r\",\"kind\":\"tcp\",\"bind_port\":30515,\"target_host\":\"127.0.0.1\",\"target_port\":1,\"enabled\":true,\"plugin\":{\"kind\":\"tls2_raw\",\"local_addr\":\"127.0.0.1:$ECHO\",\"crt_path\":\"$WORK/plugin.crt\",\"key_path\":\"$WORK/plugin.key\"}}"
wait_rule t2r
out=$(python3 - <<'EOF2'
import socket, ssl
try:
    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT); ctx.check_hostname = False; ctx.verify_mode = ssl.CERT_NONE
    s = socket.create_connection(("127.0.0.1", 30515), timeout=10)
    t = ctx.wrap_socket(s)
    t.sendall(b"tls2raw-ping\n"); t.settimeout(8)
    print(t.recv(64).decode())
except Exception as e:
    print("ERR", e)
EOF2
)
[ "$out" = "tls2raw-ping" ]; check $? "tls2raw:TLS 解密后裸转发回显($out)"

# ---------- 6) http_proxy:CONNECT + Basic 认证 + 隧道 ----------
mk_svc "{\"client_id\":\"$CID\",\"name\":\"hpx\",\"kind\":\"tcp\",\"bind_port\":30516,\"target_host\":\"127.0.0.1\",\"target_port\":$HPROXY,\"enabled\":true,\"plugin\":{\"kind\":\"http_proxy\",\"http_user\":\"pu\",\"http_password\":\"pp\"}}"
wait_rule hpx
tun=$(python3 - <<'EOF'
import socket
try:
    s = socket.create_connection(("127.0.0.1", 30516), timeout=10)
    s.sendall(b"tun-payload\n"); s.settimeout(8)
    d = s.recv(64)
    print(d.decode())
except Exception as e:
    print("ERR", e)
EOF
)
[ "$tun" = "tun-payload" ]; check $? "http_proxy:经代理隧道回显($tun)"
grep -q "CONNECT 127.0.0.1:$HPROXY" "$WORK/hproxy.log" && grep -q "AUTH=pu:pp" "$WORK/hproxy.log"; check $? "http_proxy:CONNECT 目标与 Basic 凭据正确"
# 负向:错误凭据 → 代理 407 → 用户连接被关闭
mk_svc "{\"client_id\":\"$CID\",\"name\":\"hpxbad\",\"kind\":\"tcp\",\"bind_port\":30517,\"target_host\":\"127.0.0.1\",\"target_port\":$HPROXY,\"enabled\":true,\"plugin\":{\"kind\":\"http_proxy\",\"http_user\":\"pu\",\"http_password\":\"WRONG\"}}"
wait_rule hpxbad
neg=$(python3 - <<'EOF'
import socket
try:
    s = socket.create_connection(("127.0.0.1", 30517), timeout=10)
    s.sendall(b"x\n"); s.settimeout(8)
    d = s.recv(64)
    print("EOF" if not d else d.decode())
except Exception as e:
    print("CLOSED")
EOF
)
[ "$neg" = "EOF" ] || [ "$neg" = "CLOSED" ]; check $? "http_proxy:错误凭据被拒(连接干净关闭)"

# ---------- 7) socks5:方法协商 + 用户密码 + 域名目标 + 隧道 ----------
mk_svc "{\"client_id\":\"$CID\",\"name\":\"s5\",\"kind\":\"tcp\",\"bind_port\":30518,\"target_host\":\"127.0.0.1\",\"target_port\":$SOCKS,\"enabled\":true,\"plugin\":{\"kind\":\"socks5\",\"user\":\"su\",\"password\":\"sp\"}}"
wait_rule s5
tun=$(python3 - <<'EOF'
import socket
try:
    s = socket.create_connection(("127.0.0.1", 30518), timeout=10)
    s.sendall(b"socks-payload\n"); s.settimeout(8)
    d = s.recv(64)
    print(d.decode())
except Exception as e:
    print("ERR", e)
EOF
)
[ "$tun" = "socks-payload" ]; check $? "socks5:经 SOCKS5 隧道回显($tun)"
grep -q "methods=0,2\|methods=2,0\|methods=2" "$WORK/socks.log" && grep -q "auth=su:sp" "$WORK/socks.log" && grep -q "target=127.0.0.1:$SOCKS" "$WORK/socks.log"; check $? "socks5:协商/认证/域名目标正确"

echo
echo "== 结果: PASS=$PASS FAIL=$FAIL =="
[ "$FAIL" = "0" ]
