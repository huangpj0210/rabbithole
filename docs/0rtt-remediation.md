# TLS 1.3 / QUIC 0-RTT 整改方案

> 适用版本:rabbithole v0.2.x(rustls 0.23.45 / quinn 0.11.12 / tokio-rustls 0.26)
> 文档日期:2026-10-12|责任人:待定|修订:v3(按评审修正:§3.4 quic 测试 API 事实、§4 sslscan 证据行、§3.1 早数据丢弃两分支机制、§3.5/§7 预热池论证)
> 结论先行:**当前代码不存在任何 0-RTT 收发路径**;整改目标是把"依赖库默认值"升级为
> "显式禁用 + 回归测试防再引入",并给出可提交给测评方的验证证据。

## 1. 背景

漏洞扫描 / 等保测评常见结论:目标端口开启 TLS 1.3 且下发会话票据(NewSessionTicket),
标记为「TLS 1.3 0-RTT 数据重放风险」。0-RTT(early data)允许客户端在握手完成前发送
应用数据,该数据不享受握手层的防重放保护,攻击者可原样重放,造成非幂等请求重复执行
(重复提交、重复扣款等)。

涉及资产(以默认部署为例):

| 资产 | 端口 | TLS 栈 | 说明 |
|---|---|---|---|
| RHTP 传输(tls/wss) | 7100/tcp | rustls + tokio-rustls | 客户端↔服务端隧道 |
| RHTP 传输(quic) | 7100/udp | rustls + quinn | 同上,UDP |
| Web 控制台 | 8080/tcp | 无(明文 HTTP) | 本方案不涉及;若经 nginx/CDN 终结 TLS,见 §7 |

## 2. 现状核查(代码级结论)

全部传输的 TLS 配置都汇聚在 `crates/rh-transport/src/tls.rs`:
`server_config_with_alpn()`(服务端)与 `client_config_with_alpn()`(客户端);
tcp.rs(含 wss,`tcp.rs:118`/`tcp.rs:191`)与 quic.rs(`quic.rs:39`/`quic.rs:92`)
均复用这两个函数,一处核查即覆盖 tls / wss / quic。

| 检查点 | 现状 | 0-RTT 是否存在 |
|---|---|---|
| 服务端 `ServerConfig` | 默认构建,`max_early_data_size = 0`(rustls 默认值,`tls.rs:47-52`) | **否**——下发的票据 `max_early_data = 0`,合规客户端不会发送早数据;强行发送的记录到不了应用层(两种情形的机制区分见 §3.1) |
| 客户端 `ClientConfig` | 默认构建,`enable_early_data = false`(ClientConfig 公有字段,rustls builder 默认;`tls.rs:168-209`) | **否**——客户端从不发送早数据 |
| QUIC 服务端 | quinn 接受 0-RTT 需 rustls 配置 `max_early_data_size == 0xFFFFFFFF`(rustls quic.rs 强制"0 或 2^32-1"二选一),未设置(`quic.rs:38-46`) | **否** |
| QUIC 客户端 | 未调用 `Connecting::into_0rtt()`(`quic.rs:91-132`) | **否** |
| 会话票据 | rustls 默认下发 **2** 个 TLS 1.3 NewSessionTicket(公有字段 `send_tls13_tickets` 默认值 2) | 仅支持 **1-RTT PSK 恢复**(无重放面) |

补充一个加强事实:客户端 TLS/QUIC 配置在**每次拨号时新建**(`tcp.rs:191`、`quic.rs:92`
都在 connect 函数体内),会话缓存不跨连接复用——当前客户端实际上连 1-RTT 恢复都不会
发生,更不存在 0-RTT。

纵深防御:即使未来误开 0-RTT,RHTP 内层认证为服务端随机 nonce 的挑战应答
(HMAC-SHA256(secret, "rh2:" + client_id + ":" + nonce_hex),
`crates/rh-protocol/src/message.rs:88`),重放的 0-RTT 握手数据无法通过认证,
攻击者最多消耗握手资源,拿不到会话。

**风险定性:当前为"合规留痕缺失",非实际漏洞。** 唯一问题是安全性依赖库默认值——
依赖升级或后续改动可能无声引入 0-RTT,且无法向测评方举证。

## 3. 整改措施

### 3.1 服务端显式禁用(必做)

`crates/rh-transport/src/tls.rs` → `server_config_with_alpn()`,构建后显式置零:

```rust
let mut cfg = rustls::ServerConfig::builder()
    .with_no_client_auth()
    .with_single_cert(certs, key)
    .map_err(|e| io::Error::other(format!("tls: build server config: {e}")))?;
cfg.alpn_protocols = alpn.iter().map(|s| s.as_bytes().to_vec()).collect();
// 0-RTT 永久禁用:early data 无防重放保护,见 docs/0rtt-remediation.md
cfg.max_early_data_size = 0;
```

行为说明(与默认值一致,现网无任何变化):下发的票据 `max_early_data = 0`,
合规客户端(rustls 客户端在 `enable_early_data == true` 且票据 `max_early_data > 0`
时才发送,rustls client/tls13.rs:386 门控)不会发送早数据。若恶意客户端无视票据
强行发送,按其 ClientHello 是否携带 `early_data` 扩展分两种情形,早数据均到不了
应用层:

- **携带扩展(被拒)**:rustls 服务端走 `RequestedButRejected` 分支(rustls
  server/tls13.rs),以**试解密(trial decryption)**方式切换到握手密钥——试解密
  失败的记录被直接跳过(有上界,rustls conn.rs),握手以 1-RTT 正常完成;
- **不携带扩展、直接灌早数据密文**:落入 `Disabled` 分支,不启用试解密,这些
  记录导致解密失败、握手中止——连接被断开,同样不进应用层(残余威胁仅为握手
  资源消耗,且受 §2 纵深防御约束)。

### 3.2 客户端显式禁用(必做)

`client_config_with_alpn()` 三个分支(pin / insecure / CA)统一在 `with_alpn()`
里落两句(`enable_early_data` 是 `ClientConfig` 公有字段,`resumption` 亦然):

```rust
fn with_alpn(mut cfg: rustls::ClientConfig, alpn: &[String]) -> Arc<rustls::ClientConfig> {
    cfg.alpn_protocols = alpn.iter().map(|s| s.as_bytes().to_vec()).collect();
    // 0-RTT 禁用(重放风险);会话恢复仅 1-RTT PSK,无 early data
    cfg.enable_early_data = false;
    cfg.resumption = rustls::client::Resumption::in_memory_sessions(256);
    Arc::new(cfg)
}
```

注:因客户端每次拨号新建配置(§2),`resumption` 这行当前是"声明意图"而非行为变化;
若未来引入跨拨号的配置复用,此行直接生效,同样只有 1-RTT 恢复、无 0-RTT 面。

### 3.3 QUIC 无需单独处理

`quic.rs:39` / `quic.rs:92` 复用 §3.1/§3.2 的配置源,tls / wss / quic 自动全覆盖;
rustls 的 QUIC 层强制 `max_early_data_size` 取 0 或 0xFFFFFFFF 二选一,§3.1 已排除
后者,即 QUIC 0-RTT 被结构性排除。

### 3.4 回归测试(必做,防再引入)

`crates/rh-transport` 新增测试(`cargo test -p rh-transport`):

1. **配置断言(单元)**:
   `server_config()` 产物 `max_early_data_size == 0`;
   `client_config()` 产物 `enable_early_data == false`;
2. **门控行为(集成,tls)**:测试客户端刻意置 `enable_early_data = true` 并复用同一
   `ClientConfig` 连接两次(首连拿票据,重连走恢复);断言第二次握手成功且全程
   未发送任何早数据——票据 `max_early_data = 0` 触发 rustls 客户端门控,恢复握手
   期间 `ClientConnection::early_data()` 恒为 `None`;
3. **门控行为(集成,quic)**:同第 2 项,客户端刻意置 `enable_early_data = true`
   并复用同一 `ClientConfig`/quinn `Endpoint` 连接两次(首连拿票据,重连才存在
   派生 0-RTT 密钥的可能);第二次对 `Connecting` 调用 `into_0rtt()`,断言返回
   `Err`——服务端票据 `max_early_data = 0` 触发客户端门控,不派生 0-RTT 密钥,
   QUIC 0-RTT 不可用。API 要点:`into_0rtt()` 返回
   `Result<(Connection, ZeroRttAccepted), Connecting>`,错误变体**返还 `Connecting`
   本身**(非 `ConnectionError`),测试应从 `Err` 中取回它继续 `await`,顺带断言
   握手仍正常完成;`has_0rtt()` 是 quinn-proto 内部方法,quinn 门面层不可调用,
   勿用作断言。

### 3.5 可选加强:停发会话票据(按测评要求)

若测评结论要求「禁用 TLS 会话票据」,追加一行(`send_tls13_tickets` 为公有字段,
默认值 2;代价:重连多 1-RTT + 一次非对称运算——控制面为长连接、重连罕见,且
当前客户端配置每拨新建、连 1-RTT 恢复都未发生(§2),现网零成本):

```rust
cfg.send_tls13_tickets = 0;
```

默认不启用——1-RTT PSK 恢复本身无重放风险,保留它属于正常性能优化。

### 3.6 文档与配置面

- `deploy/server.example.toml` 顶层 `bind_protocol = "tls"`(第 5 行)附近加注释:
  「0-RTT(early data)为永久禁用项,无配置开关,原因见 docs/0rtt-remediation.md」;
- `docs/design.md`「9. 安全设计」小节补记:0-RTT 禁用决策与回归测试项;
- 本文件随整改 PR 一并入库,作为测评留痕。

## 4. 验证方法(整改证据)

openssl 两步法(7100/tcp;OpenSSL ≥ 1.1.1;已在 OpenSSL 4.0.3 实测,2026-10-12):

```bash
# ① 首连拿票据(sleep 保活确保 NewSessionTicket 到达后再退出)
(sleep 1; echo) | openssl s_client -connect <host>:7100 -tls1_3 -sess_out /tmp/s.pem
# ② 用票据恢复重连(如需强制尝试早数据:1.1.1/3.x 加 -earlydata,4.x 加 -early_data <文件>)
(sleep 1) | openssl s_client -connect <host>:7100 -tls1_3 -sess_in /tmp/s.pem
```

| 手段 | 预期输出 | 说明 |
|---|---|---|
| openssl ① | 票据行 **`Max Early Data: 0`** | **决定性证据**:服务端票据禁止早数据 |
| openssl ② | `Reused, TLSv1.3` + `Early data was not sent` | 1-RTT PSK 恢复正常;即使强制指定早数据文件也不会发送(票据禁止) |
| §3.5 生效取证 | 重复步骤 ① | 执行 §3.5 后 `s_client` 不再输出 NewSessionTicket 摘要与 `Max Early Data` 行。勿用 sslscan 验证:其 ticket hint 探测是 TLS 1.2 机制,而 rustls 默认 ticketer 即 `NeverProducesTickets`,TLS 1.2 本就无可用票据,该工具对 §3.5 无区分度 |
| QUIC(7100/udp) | §3.4 第 3 项测试输出 | `into_0rtt()` 返回 Err |
| 全量回归 | `cargo test --workspace` | 通过 |

注:不存在「Early data was rejected」这类输出——合规客户端看到票据 `Max Early Data: 0`
即不发送,服务端"拒绝"只发生在恶意客户端强行发送的场景,取证以票据行为准。

## 5. 整改回复模板(提交测评方)

> 经核查,rabbithole 服务端与客户端均未启用 TLS 1.3 / QUIC 0-RTT(early data):
> 服务端显式设置 `max_early_data_size = 0`,客户端 `enable_early_data = false`,
> 会话恢复仅支持 1-RTT PSK 且不发送早数据;QUIC 传输复用同一 TLS 配置源,全链路
> 不存在 0-RTT 收发路径。已增加回归测试防止后续版本引入(见 rh-transport),
> 并附 openssl 探测记录(会话票据 `Max Early Data: 0`,恢复连接 `Early data was
> not sent`)作为佐证。应用层另具纵深防御:隧道内认证采用服务端随机数挑战应答,
> 重放数据无法通过认证。

## 6. 工作量与顺序

| 步骤 | 内容 | 量级 |
|---|---|---|
| 1 | §3.1 + §3.2 显式禁用(含注释) | ~0.5h |
| 2 | §3.4 三个回归测试 | ~2h(tls/quic 双连接门控用例较繁) |
| 3 | §3.6 注释与文档 | ~0.5h |
| 4 | §4 验证取证(公网机器实测 + 输出归档) | ~0.5h |
| 5 | (按需)§3.5 停发票据 + 复测 | ~0.5h |

- 已部署实例升级:重跑 `deploy/install-server.sh` / `install-client.sh`(保留配置与
  数据,脚本即升级),升级后按 §4 复测取证;
- 依赖升级(rustls/quinn)时,以 §3.4 回归测试为准绳;若 API 变动导致断言失败,
  先修断言再合入,不允许删测试放行。

## 7. 边界说明

- 本方案覆盖 rabbithole 自身监听的 TLS/QUIC 端口。若扫描目标实为控制台(8080)或
  其他经 **nginx / CDN / 云负载均衡** 终结 TLS 的入口,需在该层整改
  (nginx 默认 `ssl_early_data off`;Cloudflare 等需关闭 0-RTT 开关),与本项目无关;
- 本产品**永不建议**开启 0-RTT:rustls 不提供服务端反重放机制,tunnel 场景下
  early data 收益(省 1-RTT)远小于重放面;且当前架构下客户端配置每拨新建、
  会话恢复本就未启用(§2),省 1-RTT 的收益并不存在(数据面的 TCP 预热池
  服务的是后端目标连接,与控制面 TLS 握手延迟无关,不构成另论)。
