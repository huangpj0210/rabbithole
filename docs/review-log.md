# rabbithole 评审日志(review-log)

## R1 功能对照审查(frp 0.71.0 基线)

- 以 /tmp/ref/frp(d20a232)为基线逐文件对照,产出 docs/requirements-matrix.md
  (A/B/C/D/E 五域 60 项 + 3 实验项),排除边界:frp 协议互操作、INI legacy、
  存量 frpc/frps 互操作。
- 结论:代理类型/传输/认证/转发能力/集中管理五域全部有实现落点;
  部分项在实现完成后按"实现+测试+证据"标准回填状态。

## R2 SUDP 间歇丢帧根因评审(本会话)

**症状**:SUDP visitor 在 E2E 脚本中约 1/3 概率收不到回显,独立环境通过;
加入打印日志后概率显著变化(时序敏感)。

**排查过程**(全部有日志证据):
1. 最初怀疑上一会话遗留:客户端二进制陈旧(上一会话最后修改 udpchan.rs 时引入
   编译错误,`cargo build` 输出被截断掩盖,rh-client 未重编)——修复编译后复测仍失败。
2. 最小化复现环境(仅 udbs+visitor)以 ~30% 概率失败,排除多服务互扰。
3. 字节级插桩后取得决定性证据:客户端已把 UDP 帧写入通道(`vs flushed n=5`),
   服务端 splice 两端就绪,但代理侧从未收到该帧(`ps pkt` 缺失)。

**根因(服务端 read_frame 残留字节丢失)**:控制连接数据接受任务读取
DATA_CHANNEL_HELLO 时,`read_frame` 可能将同批到达的后续帧留在本地缓冲;
路由把裸流交给下游后缓冲被丢弃。客户端"写 HELLO 后立即冲首包"与本服务端
"读 HELLO 后路由"竞速:同 TCP 段到达时首包被吃。eprintln 恰好隔开两次写,
故加日志即"修复"。客户端侧同类问题在前一会话已以 leftover 前传修复,
服务端两处(mux 数据流接受、裸模式首帧)此前遗漏。

**修复**:
- `PrependedIo` 下沉到 rh-transport 共用;服务端 mux 数据接受与裸模式
  首帧路由均以残留字节前转(control.rs)。
- 顺带修复同类时序:`relay_tcp/relay_udp` 的配对表登记移至 CREATE 帧发送之前
  (state.rs `issue_relay_pair`),消除 localhost 下客户端先回环、配对表未及登记
  而静默丢流的窗口。
- `warm_proxy` 不再对 sudp 预热常驻 UdpChannel(sudp 无服务端监听,预热通道
  被即刻丢弃,徒增噪声)。
**验证**:最小复现 24/24 通过;全量 E2E 多遍 43/43(debug+release)。

## R3 长稳缺陷两处(负载测试发现)

1. **90 秒心跳无条件下线**:服务端心跳 arm 在 interval 到期时无条件
   `break Err("heartbeat timeout")`,未跟踪最近收帧时间 → 每个会话每 90s 被杀。
   E2E(~70s)与 60s 负载均在窗口内,未暴露;90s+ 负载每 90s 全量连接重置。
   修复:30s 检查一次,仅当 90s 无任何客户端帧才判死(与注释/设计一致)。
2. **数据通道限额只增不减**:`data_conn_count` 仅在路由时 fetch_add,流关闭后
   不归还 → 单会话累计 256 条通道后全部新通道被拒。修复:`QuotaIo` 守卫流,
   流丢弃即释放限额(池内空闲通道按存活期占用,语义正确)。
   修复前 30s 负载 300 次重置/1280 轮;修复后 10 分钟 491,560 轮 0 错误。

## R4 单元测试修复(不得以放宽断言换取通过)

- proxyproto:encode_v2 端口/地址顺序与规范(及解码端)相反 → 修编码;
  parse_header 对 6–11 字节的 v2 签名前缀误报 NotProxyProtocol → 修前缀判定。
- stun 测试服务器 XOR-MAPPED-ADDRESS 长度字段计算错误(8 字节体记作 4)→ 修正。
- ratelimit 断言"两次 take 之间零补充"与微秒流逝现实矛盾 → 改为确定性上界断言。
- tcpmux 增加 yamux 流路径集成测试(见 acceptance §3)。

## R5 SSH Tunnel Gateway 实现评审(russh 0.64)

- 认证:密码=客户端密钥(常量时间比较)、公钥=authorized_keys(按 key_data 比较,
  **修复**:ssh-key PublicKey 派生相等含注释字段,authorized_keys 行注释与线上
  无关,初版比较恒假)。
- 会话通道:接受 channel_open_session 并对 exec/shell 回 success 仅作会话保持
  (**修复**:默认拒绝会使 `ssh -R ... <cmd>` 在注册后立即退出、监听被级联释放)。
- forwarded-tcpip 的 connected_address 必须与注册地址串一致(**修复**:初版传空串,
  OpenSSH 按该字段匹配转发注册,一律拒绝导致 RST)。
- 断开级联释放:会话结束取消全部监听 token;`-R 0:` 从端口分配器自动分配。
- E2E:注册/数据/释放三用例全过。

## R6 浏览器验收复验(后端多轮修复后)

以真实后端(本机 release server)+ vben dev server 复验全链路:
登录 → 总览真实数据 → 建客户端(凭据仅展示一次)→ 启动真实 rh-client →
在线状态/版本显示 → 建 TCP 规则 → 真实转发验证 → UI 停用(端口实测关闭)→
启用(转发实测恢复)→ 轮换(旧会话实测下线)→ 总览统计复核 → 登出(204+清令牌)。
期间发现"总览短暂显示全零"为硬导航后 2 秒内抢读(数据未返回),刷新后正确,
非缺陷;登出验证沿用等效链路(下拉菜单在自动化下不渲染,调用同一 API+清令牌+回登录页)。

## R7 文档与交付审查

- requirements-matrix 全部行按"实现+测试+证据"回填;无证据项标 partial 并在
  acceptance.md"已知限制"逐条注明(不虚报 done)。
- 新增 README、deploy/(server/client 示例配置 + systemd 单元)、CI 工作流
  (fmt/clippy -D warnings/test/release/前端构建/E2E)。
- design.md 附实现状态附录;PROGRESS.md 记录会话 4(全部根因与修复)。

## 遗留事项(不阻塞交付,如实列出)

- 前端生产构建空白视图(acceptance §已知限制-1)。
- OIDC/TLS/WS/QUIC/HTTPS-vhost/头改写/6 个插件的自动化 E2E 证据缺口(§限制-3~6)。
- 被拒客户端错误文案 "unparsable" 展示优化(§限制-8)。

## R8 独立审计(设计 §4 十条契约逐条代码核验)与修复

独立审计(不依赖既有测试结论,直接读代码核对 §4 契约)结果:
C-3(SERVICE_UPDATE upsert)、C-4(重复消息可重确认)、C-5(DB→发布→入队顺序)、
C-7(服务端发起数据通道一次性 request_id)PASS;
C-1(bind 失败保留旧状态)、C-2(期望态 enable 判定)、C-6(单事务落库+revision)、
C-8(UDP 屏障在途失效)、C-9(停用/轮换/换会话资源级联关闭)、
C-10(running = 已绑定 ∧ 已确认)存在缺口。修复批次:

- **C-10**:`ProxyRuntime.bound` 原子标志;`spawn_runtime` 绑定成功才置位,
  `bind_error` 保持错误态但照常入队(§4.1"期间收集确认");
  `on_apply_result` 仅 `bound ∧ confirmed≥latest` 才转 running;
  `bind_error_retry` 重试成功即拉起 serve loop 并按 confirmed 直达 running/pendioffline。
- **C-1**:update_service 端口变更路径改为「重叠拒绝 → 白名单 → 内存冲突 →
  **tcp/udp 双试绑(失败不动 DB、不动旧运行态)** → 单事务落库 → 重建」;
  create 同理(tcp/udp 均试绑)。macOS 通配 bind 可与既有具体地址监听共存,
  试绑补**环回探针**(0.0.0.0:P 再试 127.0.0.1:P)对齐 Linux 语义。
- **C-6**:新增 `db.insert_proxy_bump`/`update_proxy_bump`(规则行 + revision+1 同一
  SQLite 事务),api.rs 全部调用点切换;不再存在两步提交窗口。
- **C-2**:禁用客户端上创建 → 仅落库 disabled(不占端口、不推进 revision);
  禁用客户端上规则 enable → 仅置 enabled=1(等客户端恢复批量走全序);
  恢复路径 `spawn_runtime` 补白名单资格判定。
- **C-9**:`teardown_runtime` 调 `invalidate_proxy_requests`(失效该规则在途
  UdpChannel/RelayProxy/TcpPool 请求);`on_session_down` 增会话替换守卫(新会话已
  注册则跳过旧会话清理);`assign_forward` 级联 `rule_cancel`(停用/换端口时
  在途转发即断)。
- **C-8**:UDP 目标变更走 §4.3 原位发布(快照换新 + 入队),通道重建按 §4.5
  代次隔离;在途请求由 invalidate 失效。
- 新增 `publish_spec_in_place`:非端口变更不动 listener/池,原位换快照 + 入队,
  running→pending 等新确认(§4.3"转发不中断")。

修复后回归:cargo fmt --check 通过、clippy --workspace --all-targets 0 警告、
workspace 测试 43/43、release 构建 0 错误、E2E **46/46 连续三遍**
(新增 3 用例:非端口更新原位切换目标 / 通配探针拒绝外部占用 / 拒绝后原规则仍可用)。
定向验证脚本另证:原位更新后 revision 前进、状态回 running、外部占用端口 PUT
收 3005 且 DB/旧运行态零扰动。

## R9 前端生产构建空白视图:根因分析与修复(独立复验)

现象复现:生产构建(dist/)经 rh-server `web.static_dir` 托管,登录页正常渲染、
登录成功并跳转 `#/rabbithole/overview`,随后**主界面空白**(app 容器仅剩
`<div class="ant-app"><!----></div>`),此后任意路由均不渲染;无 window 报错、
无未处理 Promise 拒绝、资源与 /api/user/info 均 200。

根因定位(逐层取证):
1. 控制台捕获到真实异常:`TypeError: Cannot read properties of undefined
   (reading 'beforeEach')`,位于 basic chunk `useLayoutScroll` 的 setup——
   `useLayoutScroll(router = useRouter())` 默认参数求值时 `useRouter()` 返回 undefined。
2. 产物出现**两个同名 useApi chunk**(各含独立的 `Symbol("")` vue-router 注入键)
   → 依赖图中存在两份 vue-router 模块实例。
3. `pnpm why vue-router`:**1 version, 2 instances**——app(@vben/web-antd)解析到
   一个 peer 变体,@vben/*(layouts/stores/hooks 等)解析到另一个 peer 变体。
4. 机理:开发模式 Vite 依赖优化把 vue-router 合并为单份,故 dev 正常;生产
   (rolldown)按各包的真实解析路径打包,保留两份 → 两套 routerKey 注入符号 →
   app 安装的 router 用 A 符号 provide,布局内 useRouter() 用 B 符号 inject →
   undefined → 渲染中止。登录页不含该布局,故登录链路正常,掩盖了问题。

修复:
- `apps/web-antd/vite.config.ts` 增加 `resolve.dedupe: ['vue','vue-router','pinia',
  '@vue/devtools-api']`,构建期强制单实例(修后产物仅一个 useApi chunk)。
- 顺带修复托管行为:SPA 回退改回 **200**(原 tower-http `not_found_service` 语义
  返回 404 状态 + index.html 体);未知 `/api/*` 路径返回 JSON 404 envelope
  (不再混入 SPA HTML);路径穿越经 ServeDir 规范化(仅回退 index)。
- 前端既有类型错误清理:clients/services/visitors 三个视图 `bodyCell` 槽位
  `record` 为 `Record<string, any>`,调用处理器需 `as XxxRow` 断言,共 13 处,
  `typecheck` 修后 0 错误(此前该门实际未通过,系此前记录失实,一并纠正)。

复验(全部真实现场执行):
- 干净重建 dist ×2 均单 useApi chunk;`typecheck` 0 错误。
- **生产构建 + 真实后端浏览器验收全链路通过**(acceptance §7):登录/总览真实
  统计/建客户端取一次性凭据/启动真实 rh-client 显示在线/建 TCP 规则 running/
  真实转发 ECHO:/改目标即时切换 SECOND:/停用端口实测关闭/启用恢复/轮换后旧会话
  被拒、新密钥恢复在线且转发可用/统计复核/登出后刷新无自动登录;负向:错误密码
  提示、未登录路由守卫重定向、空表单校验。
- Rust 回归:fmt --check 通过、clippy 0 警告、test 43/43、release 构建 0 错误、
  E2E 46/46 两遍(main.rs 托管改动后全量回归)。

## R10(2026-10-09)XTCP 隔离网络验证、韧性 E2E、传输变体 E2E、前端 lint/vitest

### XTCP 隔离网络(tests/xtcp-isolated.sh + tests/natgw.py,9/9 ×3)
用户态 STUN+双 NAT 网关验证打洞直连(网关字节级 relayed=1094)与拦截回退
(blocked=50/relayed=0 → 服务端中继)。暴露并修复 5 个真实缺陷:
1. 打洞失败 reissue 无限重入(等待连接 10s TTL 过期):NatCoordinator 增加
   `mark_attempted`((session_id, user_conn_id) 60s 去重),仅首次进入打洞,
   失败即回退 issue_relay_pair;
2. relay_pairs 按裸 request_id 键控,两会话各自 rid=1 互相覆盖 → 中继一侧静默
   丢流:改键为 (session_id, request_id)(STCP e2e 单会话故未暴露);
3. punch_listen 无超时,每次失败打洞泄漏任务+socket:2×punch_secs 超时;
4. multiplex=false 裸模式全坏("bad magic"):服务端 peek 前 4 字节自动识别
   raw(RHTP)/yamux,peek 字节经 PrependedIo 转发;
5. 恢复期对固定端口也重分配(违反 §4.6):重分配收敛到 port_auto 规则。

### 韧性 E2E(tests/e2e-resilience.sh,19/19 ×3)
新增 §六.C 五场景独立套件(目标拒连/DB EXCLUSIVE 锁故障回滚/仅数据通道断开
(tcpkill-mm.py)/迟到连接/旧 ACK 迟到(冻结跨版本)/重复上线)。两个关键发现:
- **测试侧教训**:echo 服务器未显式 `finally: c.close()` 时,CPython 线程对象
  `_args` 延迟释放 socket,FIN 不达 → 转发"超时"。曾误判为产品缺陷并以
  yamux 帧级追踪排查数小时;e2e.sh 通过恰因它的 echo 显式关闭。测试基建已修正
  并在脚本内注明。
- **真实缺陷(重复上线)**:会话替换后 on_session_down 对被替换会话早退保护新
  会话(正确),但池内旧会话就绪通道无人驱逐,tcp_refill 见 ready≥want 不补建
  → 用户连接弹出到冻结旧通道形成黑洞。修复:池条目携带建立会话 epoch,
  get_channel/tcp_refill 以单调 epoch **前向认领**并驱逐旧条目,迟到的旧会话
  通道在 tcp_on_channel 丢弃(state.rs TcpPool + data.rs claim_pool)。

### 传输变体 E2E(tests/e2e-transports.sh,12/12 ×3)
六协议(含 tcp 对照)独立环境全链路(上线+池+转发+半关闭)。暴露真实缺陷:
TLS/WSS/QUIC 自动自签证书落盘前未创建 `<配置目录>/certs/` → 监听 ENOENT 失败;
tls.rs auto_cert 先 create_dir_all。矩阵 B2/B3/B4/C9 由 partial 转 done。

### 前端 lint + vitest(§六.B)
- 恢复裁剪时丢失的上游根级 lint 配置(eslint/stylelint/oxfmt/oxlint/.stylelintignore
  均按上游 d27a5b8 原样);修复 pnpm 11 设置位置(neverBuiltDependencies→
  workspace yaml;allowBuilds 恢复但剔除 lefthook,防向父仓库写 git hooks);
  element-plus 精确 2.14.5 回归 catalog + form-ui devDependency(上游测试深路径
  依赖);清理 16 项无消费者 catalog 条目;全仓 eslint/oxfmt/oxlint --fix 收敛
  (发现 oxfmt format-mode 与 eslint sort-keys 在 package.json 上互改,以
  oxfmt.config.ts sortPackageJson:false 为准的上游方案恢复后不再冲突)。
- 结果:`pnpm run lint` exit 0(oxlint 0/0、oxfmt/stylelint 全绿、eslint 0 错误,
  余 16 警告均为上游框架测试文件);`pnpm run test:unit` **575/575**;
  typecheck 0 错误;build:antd 通过 + dist 经 rh-server 冒烟(index/深路由 200,
  /api 未授权 401)。

### 回归(全部真实现场执行)
cargo fmt --check 通过 / clippy --release --all-targets 0 警告 / test 43/43 /
release 构建 0 错误;tests/e2e.sh **46/46**;e2e-resilience **19/19**×3;
xtcp-isolated **9/9**;e2e-transports **12/12**×3。副产物:relay_pairs 复杂类型
抽别名消 clippy type_complexity;nathole_cli 超时代码 fmt 对齐。

## R11(2026-10-09)插件/vhost 进阶/OIDC 三套件 E2E:关闭全部 partial 行 + 三个真实缺陷

### tests/e2e-plugins.sh(15/15 ×3)
七插件(http_proxy/socks5/https2http/https2https/http2https/http2http/tls2raw)
经 API 创建插件规则全链路验证:改写类在上游侧逐头断言(Host 重写、set、del、
响应头注入);TLS 类用自签证书(curl -k / python TLS);代理类本地辅助服务
断言握手细节(CONNECT 目标与 Basic 凭据、SOCKS5 方法协商/用户密码/域名目标)
及隧道回显,含错误凭据 407 负向。教训:插件 JSON kind 为 serde 变体名
(https2_http 等),与 PluginKind::as_str 展示名不同。

### tests/e2e-vhost.sh(10/10 ×3)与 peek_sni 真实缺陷
自签证书 + curl --resolve 验证 HTTPS SNI 双域名分流/未知 SNI 拒绝、HTTP 头改写、
Basic 鉴权、自定义 404。**暴露真实缺陷(该路径此前零 E2E)**:peek_sni 用
try_read(消费 ClientHello,后续 copy_bidirectional 丢失握手字节)且把
WouldBlock 当错误直接断连。修复:改用 async peek(不消费)+ WouldBlock 容忍
等待 + 5s 截止。修后双域名分流与未知 SNI 拒绝均通过。

### tests/e2e-oidc.sh(8/8 ×3)本地 IdP 桩与两个真实缺陷
纯 python + openssl CLI 实现 IdP 桩(发现文档/JWKS RS256//token 可定制
sub/aud/ttl)。**暴露两个真实缺陷**:
1. 服务端 JWKS 获取丢弃 URL 显式端口(`http://127.0.0.1:30901` 连成
   `…:30901:80`,getaddrinfo 失败)——oidc.rs http_get_json 端口解析修复
   (仅纯数字尾巴拆端口,避免误拆 IPv6 字面量);
2. 客户端 hello 首帧仅接受 ERROR/AUTH_CHALLENGE,oidc 路径服务端验签通过
   直接回 HELLO_ACK,客户端报 unexpected frame 退出——OIDC 客户端链路此前
   从未端到端可用。修复:首帧接受 HELLO_ACK(挑战路径不变)。
另补服务端 oidc 拒绝 warn 日志(原因字符串,不含令牌;已验证输出无 JWT 内容)。

### 回归(全部真实现场执行,最终二进制)
cargo fmt --check ✓ / clippy --release --all-targets 0 警告 / test 43/43 /
release 构建 ✓;e2e.sh **46/46**、e2e-resilience **19/19**、xtcp-isolated
**9/9**、e2e-transports **12/12**、e2e-plugins **15/15×3**、e2e-vhost
**10/10×3**、e2e-oidc **8/8×3**。requirements-matrix **partial 清零**
(A4/B9/C11/C12/C17~C24/C27 全部 done),acceptance 新增 §9/§10/§11 并将
已知限制 3~6 改为已关闭。

## R12(2026-10-09)按用户决策移除 OIDC 功能

移除范围(代码/协议/配置/测试/文档全链):
- 删除 crates/rh-server/src/oidc.rs、control.rs auth_oidc 与 hello.auth_method 分支、
  config.rs OidcConfig/[auth] AuthConfig.allow_oidc(该字段无任何读者)、db.rs
  get_client_by_name(仅 oidc 使用)、control.rs NoVerifyTls(仅 JWKS https 用)。
- 协议:Hello 删除 oidc_token 与 auth_method 字段(自有协议,直接改版);
  客户端 hello 流还原为仅挑战路径,config 删除 oidc_token。
- 测试与配置:删除 tests/e2e-oidc.sh;CI 移除对应步骤;deploy 两个示例移除
  oidc 相关键。jsonwebtoken 保留(管理端 JWT 会话使用)。
- 文档:矩阵 B9 → excluded(注明用户决策与移除前验证史);acceptance 删除
  E2E-7 章节并重编号、限制-4 改写为移除记录;design.md P2 清单与遗留句同步。

移除过程事故与恢复(如实记录):脚本化删除 auth_oidc 时括号配平算法误配
(函数签名跨行,首个 `{` 前深度已归零),第一刀切掉了 auth_and_run 的挑战路径
主体。恢复方式:上下文中存有 hello 分发/挑战签发/会话注册尾段的原文本,
缺失的 AUTH_RESPONSE 验证段按协议契约(session.rs 客户端公式
HMAC-SHA256(secret,"rh2:{client_id}:{nonce}") hex + ChallengeTable.consume
一次性消费与归属校验 + Hmac::verify_slice 常量时间比较)重建,并以全量
E2E 验证(e2e.sh 46/46 含错误密钥拒绝/轮换中踢线等认证用例)。
教训:大段删除优先用 Edit 的唯一锚点匹配,不用行号/深度扫描脚本。

回归(移除后,全部真实现场执行):cargo fmt --check 通过、clippy 0 警告、
test 42/42(少的 1 例为 oidc 单测随文件删除)、release 构建 0 错误(仅本机
rust-objcopy/libLLVM 既有剥离告警);e2e.sh 46/46、e2e-resilience 19/19、
xtcp-isolated 9/9、e2e-transports 12/12、e2e-plugins 15/15、e2e-vhost 10/10。

## R13(2026-10-09)首启引导 admin + 控制台改密(用户指令)

用户指令:原流程需手动 `rh-server admin create` 建号,被判定不合理;要求首次
启动自动创建 admin(随机密码),并确认改密能力(当时不存在)。

实现:
- 服务端 `bootstrap_initial_admin`(main.rs):users 表为空时创建 admin,
  密码 = 两个 UUID v4 simple 拼接收窄 24 字符;**仅 stdout 横幅一次性展示**,
  tracing 事件不含密码;已有用户即跳过(重启幂等)。db 新增
  count_users / update_user_password。
- `POST /api/auth/password`(api.rs):require_auth → 验旧密码(argon2,错则
  code=1001 + "password change rejected" warn 审计)→ 新密码 ≥8(code=1002)→
  落库 → **轮换 web_jwt_secret**(所有已签发 JWT 立即失效)→ 审计
  "password changed: {user}"。
- 前端:api changePassword;新增「设置」页(rabbithole/settings)改密表单
  (当前/新/确认,前端校验 ≥8 与一致性),成功后提示并 authStore.logout()
  强制重登;README 快速开始去掉 admin create 步骤。

缺陷与事故(如实记录):
- **e2e.sh 假失败(工具链问题,非产品缺陷)**:新增 boot 用例首跑全挂,经
  $BOOT/binary.sha 指纹定位为脚本默认 BIN_DIR=target/debug——本机 debug 目录
  存在 R12 之前的陈旧二进制(无 bootstrap)。默认改为 target/release(与 README
  一致),另两处 curl JSON 单引号内 `\"` 转义传成字面反斜杠致假登录失败,改
  双引号。
- **设置页空表单误显灰色"两次输入的新密码不一致"**:antd FormItem `help`
  恒渲染(validate-status 只控色),浏览器验收发现;改条件渲染
  (:help/:validate-status 仅 mismatch 时给值),复测空=无提示、不一致=红字
  红框(h=22,color=error)、一致=消失。

浏览器验收(生产构建 + static_dir 托管,同源 /api):横幅随机密码登录 →
侧边栏「设置」→ 提交改密 → toast「密码已修改,所有会话已失效,请重新登录」+
自动登出(保留 redirect)→ **新密码重新登录成功**并回跳设置页。
工具备注:IAB 下 Playwright locator click 的 actionability 等待在本应用挂起
(登录/菜单/提交按钮),fill 与快照正常;改 DOM 级 evaluate(el.click())或
坐标点击(按钮实际位于分栏右半 x≈1013)后全流程通过——此前多轮"点击无效"
实为坐标点在左侧营销面板 + locator 等待挂起的叠加。

回归(改动后全部真实现场执行,fmt 修复触发的全新 release 构建):
- cargo fmt --check 通过(本轮曾发现 db.rs update_user_password 格式偏差,已 fmt 修复);
  clippy --workspace --all-targets 0 警告(rh-server 重编译复核);test 42/42。
- e2e 六套件(全新 release 二进制):e2e.sh **55/55**(含 boot 9 例)、resilience
  19/19、transports 12/12、xtcp-isolated 9/9、plugins 15/15、vhost 10/10,合计 120。
- 前端门禁(设置页修复后):typecheck 0 错、lint exit 0、build 通过、浏览器验收
  生产构建全链路(见上);vitest 全量三连跑 1/1/0 例失败,第三次 **575/575 全绿**
  ——失败均为上游 form-integration.test.ts 时序偶发(文件与上游字节级一致,单文件
  5 连跑 3 次失败且每次用例不同),未屏蔽未跳过,证据见 acceptance §2。
