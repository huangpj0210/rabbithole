# 会话进度记录(供跨上下文恢复)

## 基线与环境
- frp 基线: /tmp/ref/frp commit d20a232996007dfe6ab425abc0a39a3ae9a0889b (v0.71.0)
- Rust: ~/.rustup/toolchains/stable-aarch64-apple-darwin/bin (rustc 1.98.1,无 rustup shim)
  每条命令需 `export PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH"`
- Node 26.10.0 / pnpm 11.16.0(经 pnpm 自身 manage-package-manager-versions);无 docker
- macOS 测试无 timeout 命令,用 `perl -e 'alarm N; exec @ARGV' --`
- **重要教训: cargo build 不重建 test 二进制;改完源码必须 cargo test --no-run 再跑测试二进制**

## 已完成
1. docs/requirements-matrix.md — 全部稳定能力映射(状态 planned,实现后更新)
2. web/ — vben commit d27a5b8a 裁剪版,pnpm install+build 通过(见 web/FRONTEND-NOTES.md)
   - dev proxy /api → http://127.0.0.1:8080;VITE_NITRO_MOCK=false
3. crates/rh-protocol — RHTP 帧 + RHCP 消息 + ProxySpec/VisitorSpec/PluginSpec(测试通过)
4. crates/rh-transport — tcp/tls/ws/wss/quic/kcp + yamux mux + 代理拨号 + snappy compress_stream(14 测试全绿)
   - 关键接口: transport::{connect,listen, Multiplexer{open_stream,accept}, BoxedIo, compress_stream}
   - tls: client_config 返回 (config, ClientVerify{Pin,Standard,Insecure});pin 为握手后指纹比对
   - kcp: 服务端单 UDP conv 分流;yamux on top;客户端 UdpOutput connected=true 用 try_send
5. crates/rh-core — proxyproto v1/v2, TokenBucket, BandwidthQuantity, stun client+测试服务器, PortsRange/白名单/冲突判定
6. crates/rh-server/src/config.rs + db.rs + migrations/0001_init.sql(schema: users/clients/proxies/visitors/events)
7. Cargo workspace + LICENSE(Apache-2.0) + NOTICE + rust-toolchain.toml(1.98.1)

## 关键架构决策(已定)
- 数据通道全部服务端指令发起(CREATE_DATA_CHANNEL + request_id 一次性)
- START_FORWARD 自包含(含 flags: COMPRESSED/TCPMUX_PASSTHROUGH, proxy_proto) — 客户端不查本地配置
- visitor: USER_CONNECT{visitor_id,user_conn_id} → 服务端双向数据通道中继(A: START_FORWARD; B: START_VISITOR_RELAY)
- 传输加密: TLS(自签自动生成+pin)/quic;kcp 明文 UDP(文档注明);noise 放弃(偏差记录)
- 状态机: disabled/bind_error/offline/pending/running/apply_error + unhealthy
- 压缩: compress_stream 是 async fn 包装(pump 任务+duplex),无手写 poll
- API envelope: 200+{code,data,message};401 用 HTTP 状态码(vben 契约)
- 前端在 web/(独立 pnpm workspace),API 前缀 /api

## 下一步
- rh-server: state.rs(Registry/ProxyManager/serial domain) → control.rs(auth/session/dispatch) → proxy/tcp+udp → api → main
- 然后 rh-client;再扩展 http/https/tcpmux/stcp/sudp/xtcp/插件;ssh gateway;metrics;前端页面;E2E

## 会话 3(2026-10-08 下午)—— 服务端可运行 + 客户端全功能 + 全类型 E2E 打通

### 已修复的关键 bug(勿再踩)
1. **get_channel 自死锁**:ready(tokio Mutex) 守卫跨 tcp_refill().await,内部再取同锁 → 用户连接永久挂起。修复:pop 后先 drop(ready)。
2. **read_frame 残留字节丢失**:read_frame 会把帧后同批到达的数据留在调用方 BytesMut;客户端 open_data_channel 丢弃该缓冲 → 用户首包数据丢失。修复:leftover 前传(handle_start_forward prefix / udpchan fbuf 预置 / deliver_relay 前写本地连接 / PrependedIo)。
3. **spec.id=0 冲突**:创建时 spec.id 未回填 DB rowid → 所有规则在客户端/运行时 map 中键冲突(键全 0,后写覆盖前写)。修复:db.rs insert 回填 + 读取强制 spec.id=rowid;VisitorSpec 同。
4. **信封不一致**:服务端会话队列下发 ServerMessage 信封(枚举包装),客户端按裸结构解析 → sync 解析失败。修复:客户端按帧类型先解信封。AUTH_RESPONSE 为裸帧(握手不走队列)。
5. **splice_relay 重复指派帧**:on_relay_proxy_channel 已写 START_FORWARD,splice_relay 又写一帧+双端 compress → 用户收到裸协议帧。修复:splice_relay 纯裸 splice,压缩由两端客户端按 flags 自包装。
6. **ProxyKindConfig flatten 冲突**:kind 字段与 flatten tag 同名。重构:每 kind 独立 Option 配置字段(非 flatten,显式嵌套键 http:{...} 等)。
7. **SQLite FOR UPDATE 不支持**:bump_revision 改为 UPDATE...+1 后同事务 SELECT。
8. **迁移器分号切分遇注释内分号** → 改 sqlx::raw_sql 整段执行。
9. **XTCP 打洞三连修**:(a) 同客户端双角色报告只记一次 → on_report 双槽;(b) nat_socks 单 socket 被覆盖 → 每事务 VecDeque(dial 取首/listen 取尾,与到达顺序无关);(c) kcp 打洞期无输出 → 哨兵消息 RHTP-PUNCH!(kcp 重传即打洞包,listen 剥离);(d) macOS 通配绑定报 0.0.0.0 候选不可拨 → 127.0.0.1 替代。单测 punch_dial_listen_loopback 通过。
10. sqlx 0.9 query() 需 &'static str → 迁移用 raw_sql;governor check_key 返回 Result;argon2 需 rand_core0.6 getrandom feature。
11. **端口冲突**:macOS ControlCenter 占 7000 → 默认 7100。

### E2E 已验证(127.0.0.1 全链路,/tmp/rh-e2e)
TCP(大payload/半关闭/10并发)✓ UDP ✓ HTTP vhost(域名路由+Basic auth 就绪)✓ STCP ✓ SUDP ✓ XTCP 直连+复用 ✓ snappy 压缩(600KB)✓ PROXY protocol v1 ✓ 密钥轮换(旧拒新通)✓ 错误密钥拒绝 ✓ 服务端重启恢复(TCP+UDP)✓ 创建时端口冲突拒绝 ✓ 更新时冲突保留原状态(7301 仍通)✓ tcpmux CONNECT 200(yamux 流路径待 Rust 集成测试)✓ 状态机/API 状态/池计数正确 ✓

### 环境
- echo 9101(tcp,记录到 received.log)/udpecho 9102/http 9103(python http.server)/hdrsrv 9104;web vhost 8081/tcpmux 8082/api 8080/控制 7100;规则端口 7301-7312。
- cargo test 必须先 --no-run 再跑;无 timeout 命令用 perl alarm。

## 会话 4(2026-10-08 晚)—— 根因修复收官 + SSH 网关 + 负载 + 全部质量门 + 文档

### 根因修复(勿再踩)
1. **服务端 read_frame 残留字节丢失(SUDP 间歇丢帧根因)**:mux 数据接受任务读 DATA_CHANNEL_HELLO 后把同批到达的后续帧留在本地缓冲并丢弃;客户端"HELLO 后立即冲首包"与之竞速,同 TCP 段即丢。修复:PrependedIo 下沉 rh-transport,服务端 mux 接受与裸模式首帧两处前传残留。教训:cargo build 输出被 tail 截断会掩盖编译失败 → 二进制陈旧、假象排查;build 必须看 error 计数。
2. **配对表登记竞速**:relay_tcp/relay_udp 在发送 CREATE 后才登记 relay_pairs,localhost 下客户端可能先回环开流,deliver_pair 找不到槽位静默丢流。修复:state.rs issue_relay_pair 先登记配对表与 pending,再发 CREATE;失败全量回滚。
3. **90s 心跳无条件下线**:心跳 arm 到期即杀会话(无最近收帧跟踪),每 90s 全量连接重置。修复:30s 检查 + 90s 无帧才判死。
4. **数据通道限额泄漏**:data_conn_count 只加不减,256 条通道后全拒。修复:QuotaIo 守卫流,流释放即归还限额。
5. **warm_proxy 对 sudp 预热常驻通道**被立即丢弃:移除(sudp 由 visitor 中继按需建立)。
6. **单测修复**:proxyproto encode_v2 字段序(应地址后端口)+ v2 部分前缀误判;stun 测试服务器属性长度;ratelimit 微秒断言。原则:修实现/修测试自身的确定性错误,不放宽验收。
7. **ssh-key PublicKey 相等含注释**:authorized_keys 比较须用 key_data();**russh 默认拒绝会话通道** → ssh -R 带命令时注册即退出,须接受并空跑 exec;**forwarded-tcpip connected_address 必须与注册地址一致**(OpenSSH 据此匹配)。

### 新增能力
- **SSH Tunnel Gateway**(sshgateway.rs,russh 0.64):密码=客户端密钥(常量时间)/公钥=authorized_keys;-R 0 自动分配;断开级联释放;每连接开 forwarded-tcpip 通道双向搬运。
- **客户端自定义 DNS**(dns_server + dnsquery.rs):目标域名解析可指向指定 DNS;失败回退系统解析。
- e2e 新用例:健康检查(失败/恢复)、TCPMux CONNECT 200、static_file/uds 插件、group LB、带宽限速、自定义 DNS、SSH 网关三连 → **43/43**(debug+release)。
- 负载测试脚本 tests/load.sh:100 并发×10 分钟 **491,560 轮 0 错误**。

### 质量门(全绿)
cargo fmt ✓;clippy workspace --all-targets **0 警告**(死代码清理+风格统一+大枚举 Box 化:SyncEntity::Proxy/ServerMessage::ServiceUpdate,wire 兼容);test **43/43**;release 构建 ✓(rust-objcopy 本机 libLLVM 缺失仅影响调试信息剥离);浏览器验收复验全链路通过(dev server+真实后端;生产构建空白为已知限制)。

### 交付物
README.md;deploy/(server/client 示例 + systemd 单元×2);.github/workflows/ci.yml(fmt/clippy -D warnings/test/release/前端构建/E2E);docs/requirements-matrix.md(全行回填);docs/acceptance.md;docs/review-log.md;design.md 实现状态附录。

### 遗留(如实)
前端生产构建空白视图;OIDC/TLS/WS/QUIC/HTTPS-vhost/头改写/6 插件的自动化 E2E 证据缺口;被拒客户端错误文案 unparsable。

## 会话 5(审计修复收尾)

- 独立审计 10 契约中 6 项缺口全部修复(api.rs 接线单事务 db 调用、非端口变更
  原位发布、UDP 试绑、禁用跳步、spawn 白名单资格;runtime bound 门控与
  bind_error_retry 重构)。
- 发现并修复 macOS 通配 bind 与具体地址监听共存的试绑盲区(补环回探针)。
- 新增 3 个 E2E 用例(46 总);E2E 46/46 连续三遍、fmt/clippy 0 警告/测试 43/43。
- 文档:review-log R8、acceptance E2E 计数与用例更新、design 附录审计结论。

## 会话 6(前端生产构建空白视图修复)

- 复现并根因定位:登录后主界面空白 = vue-router 在 pnpm 图中双实例(app 与
  @vben/* 各一 peer 变体),rolldown 保留两份 → 两套 routerKey 注入符号,
  布局 useRouter() 返回 undefined(useLayoutScroll 默认参数读 beforeEach 抛错)。
  dev 正常因 Vite 依赖优化合并单份。
- 修复:vite.config resolve.dedupe(vue/vue-router/pinia/@vue+devtools-api)。
- 顺带:SPA 回退 200、未知 /api/* 返回 JSON 404、路径穿越规范化;前端
  typecheck 13 处既有类型错误清零(bodyCell record 断言)。
- 复验:生产构建 + 真实后端浏览器验收全链路(含修改目标即时切换、停用/启用、
  轮换踢线、登出、负向检查);E2E 46/46×2;Rust 门全绿。文档同步
  (acceptance 前端质量门+生产浏览器验收+限制-1 关闭、review-log R9、README)。

## 会话 7(隔离网络/韧性/传输变体验证 + 前端 lint/vitest + 两个真实缺陷修复)

- **XTCP 隔离网络**(tests/xtcp-isolated.sh + natgw.py,9/9×3):用户态 STUN+
  双 NAT 网关,打洞直连与拦截回退双路径字节级证据;修复 5 个真实缺陷(回退
  无限重入、relay_pairs 跨会话撞号、punch_listen 泄漏、裸模式握手识别、
  固定端口误重分配)。
- **韧性 E2E**(tests/e2e-resilience.sh,19/19×3):§六.C 五场景(目标拒连/
  DB EXCLUSIVE 锁故障回滚/仅数据通道断开/迟到连接/旧 ACK 迟到/重复上线);
  **修复真实缺陷:会话替换后池内旧会话通道无人驱逐 → 转发黑洞**,引入池
  epoch 前向认领+驱逐(data.rs claim_pool);测试侧教训:echo 必须显式
  c.close()(CPython Thread._args 延迟释放会吞 FIN,曾误判产品缺陷)。
- **传输变体 E2E**(tests/e2e-transports.sh,12/12×3):tcp/tls/ws/wss/quic/kcp
  独立环境全链路;修复 TLS/WSS/QUIC 自动自签落盘未建 certs/ 目录缺陷;
  矩阵 B2/B3/B4/C9 转 done。
- **前端 lint + vitest**:恢复上游根级 lint 配置五件套(裁剪时丢失,系 lint
  门此前无法运行的真实根因);pnpm 11 设置位置修正;element-plus 精确回归;
  16 项死 catalog 清理;`pnpm run lint` **exit 0**、`test:unit` **575/575**、
  typecheck 0 错、build+SPA 冒烟通过。
- **回归**:fmt ✓ / clippy 0 警告 / test 43/43 / e2e 46/46 / resilience 19/19×3 /
  xtcp 9/9 / transports 12/12×3。文档:matrix 4 行转 done+2 行补证、acceptance
  §6/§7/§8 新增+前端门+限制-3 收窄、review-log R10。
- 遗留(如实):HTTPS vhost SNI、OIDC、头改写/Basic 鉴权/自定义 404、6 插件的
  自动化 E2E 证据缺口;被拒客户端错误文案 unparsable。

## 会话 8(插件/vhost/OIDC 三套件:矩阵 partial 清零 + 三个真实缺陷)

- **tests/e2e-plugins.sh(15/15×3)**:七插件全链路(http_proxy/socks5 经本地
  代理辅助服务断言握手与隧道;四个 HTTP 改写类上游侧逐头断言;tls2raw 裸回显),
  关闭 C17~C24。
- **tests/e2e-vhost.sh(10/10×3)**:HTTPS SNI 双域名分流(curl --resolve + 自签
  证书)、头改写、Basic 鉴权、自定义 404,关闭 A4/C11/C12/C27。**修复真实缺陷:
  peek_sni 用 try_read 消费 ClientHello 且 WouldBlock 断连**(该路径此前零 E2E)。
- **tests/e2e-oidc.sh(8/8×3)**:本地 IdP 桩(python + openssl CLI:发现文档/
  JWKS RS256//token)联调,合法令牌上线+转发、三种拒绝路径 + 服务端拒绝原因
  日志(无令牌泄漏),关闭 B9。**修复两个真实缺陷**:JWKS 获取丢 URL 端口
  (host:port 连成 :80 解析失败);客户端首帧不接受 oidc 路径的 HELLO_ACK
  ——OIDC 客户端链路此前从未端到端可用。
- **回归**:fmt/clippy 0 警告/43 单测;七套 e2e 全绿(46+19+9+12+15+10+8=119)。
  requirements-matrix **partial 清零**;acceptance §9~§11 新增、限制 3~6 关闭;
  review-log R11。

## 会话 9(按用户决策移除 OIDC)

- 移除 oidc.rs / auth_oidc / 协议 oidc_token+auth_method / 配置与部署示例 /
  e2e-oidc.sh / CI 步骤;jsonwebtoken 保留(管理端 JWT)。矩阵 B9 → excluded。
- 删除脚本误伤 auth_and_run 挑战路径,按协议契约重建(HMAC 挑战 + nonce
  一次性消费 + 常量时间比较),以 e2e.sh 认证类用例(46/46)验证。
- 回归:fmt/clippy 0 警告、test 42/42、六套 e2e 全绿(46+19+9+12+15+10=111)。

## 会话 10(首启引导 admin + 控制台改密,用户指令)

- 用户判定手动 `admin create` 不合理 → **bootstrap_initial_admin**:users 表为空时
  自动建 admin,24 字符随机密码仅 stdout 横幅一次性展示(tracing 不含密码,重启
  幂等);db 新增 count_users/update_user_password。
- **POST /api/auth/password**:验旧密(argon2,code=1001)→ ≥8(code=1002)→ 落库
  → **轮换 web_jwt_secret(全部已签发 JWT 立即失效)**→ 审计。`admin create`
  保留为追加管理员方式。
- 前端「设置」页改密表单(成功强制重登);README 快速开始去掉建号步骤。
- 浏览器验收(生产构建):横幅密码登录 → 设置 → 改密 → 强制重登 → 新密码登录
  回跳设置页。修复设置页空表单误显灰色"两次不一致"(antd FormItem help 恒渲染)。
- e2e.sh 新增 boot 独立环境 9 例 → **55/55**;过程修复脚本默认 BIN_DIR 曾指向
  target/debug 陈旧二进制的假失败(改默认 release)与两处 curl JSON 引号错误。
- 回归:fmt(修复一处格式偏差)/clippy 0 警告/test 42/42;六套 e2e 全绿
  (55+19+12+9+15+10=120);前端 typecheck/lint/build 绿,vitest 575/575
  (上游 form-integration 时序 flake 如实披露:三连跑 1/1/0,第三次全绿)。
- 文档:README/acceptance(§5 55 例+boot 段、§12 三轮、§2 flake 披露)/
  matrix D1/E2/design §7.1 §9.5 §13/review-log R13。
