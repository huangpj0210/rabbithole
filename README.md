# rabbithole

自托管的端口转发与远程服务接入平台:Rust 服务端 + 客户端 + Web 管理控制台(Umi Max + antd v6)。
功能基线对齐 frp v0.71.0 的稳定能力(对照矩阵见 `docs/requirements-matrix.md`),
协议为全新自有设计(RHTP),不兼容 frp。

## 功能总览

- **8 种代理类型**:TCP / UDP / HTTP vhost(域名+子域名路由)/ HTTPS vhost(SNI)/
  STCP / SUDP(密钥访问)/ XTCP(P2P 打洞,含 KCP 直连)/ TCPMux(HTTP CONNECT 复用)
- **传输**:tcp / tls(rustls,支持自动自签证书)/ ws / wss / quic(quinn)/ kcp;
  yamux 多路复用;客户端可经 HTTP CONNECT / SOCKS5 代理拨号
- **认证**:客户端密钥 HMAC 挑战应答;
  管理面 argon2 + JWT(首次启动自动创建 `admin`,随机密码仅在服务端 stdout 横幅展示一次;
  控制台「设置」页可改密,改密后所有旧会话令牌立即失效);凭据轮换/撤销/会话替换
- **运行能力**:连接预热池、TCP/HTTP 健康检查、group 负载均衡、带宽限制、
  snappy 压缩、PROXY protocol v1/v2、HTTP 头/Host 改写、端口白名单与自动分配、
  IPv4/IPv6、10 种客户端插件、自定义 DNS、SSH Tunnel Gateway(`ssh -R`)
- **集中管理**:SQLite 存储;规则热更新(期望状态 + 幂等全量同步);
  在线状态/失败原因;全局/分客户端/分规则流量与连接统计;客户端级配额
  (并发/规则数/限速/流量上限);HTTPS 多证书(SNI 服务端终结);事件审计;
  Prometheus 指标;重启恢复
- **Web 控制台**:登录/登出、客户端管理(一次性凭据展示 + 配额)、规则与访问器管理
  (批量启停删、插件、HTTP 增强:自定义 404/静态缓存/头改写)、证书管理、
  总览统计/流量速率图/事件流(级别筛选)、服务器配置热生效
  (SSH 网关/端口策略/日志级别/公钥白名单)、修改密码

## 快速开始

```bash
# 服务端(需 Rust stable)
cargo build --release
./target/release/rh-server --config deploy/server.example.toml
# 首次启动自动创建 admin,随机密码只在 stdout 横幅展示一次(不再需要 admin create);
# 首登后请立即在控制台「设置」页修改密码

# 客户端(凭据在控制台"客户端"页创建,仅展示一次)
./target/release/rh-client --config deploy/client.example.toml
```

Web 控制台默认 `http://127.0.0.1:8080`。完整配置说明见 `deploy/` 与 `docs/design.md`。

控制台两种运行方式:

```bash
# A. 生产模式(推荐,单二进制):构建前端产物后重新编译服务端,
#    dist 会被 rust-embed 自动嵌进 rh-server(build.rs 自动检测),无需任何配置
cd console && pnpm install
pnpm build                            # 产物 console/dist/(Umi Max + antd v6)
cd .. && cargo build --release        # 二进制内嵌控制台
./target/release/rh-server --config deploy/server.example.toml
# 也可改用 [web] static_dir 指向磁盘目录(优先级高于内嵌,便于只更新前端不重编);
# SPA 回退已内置:深层路由 200,未知 /api/* 返回 JSON 404
# 未构建前端时直接 cargo build 也可编译(二进制不含控制台,API 正常,补 UI 需重建)

# B. 开发模式:Umi dev server(/api 代理,端口与后端见 console/config/proxy.ts)
cd console && pnpm dev
```

SSH Tunnel Gateway(可选,`[ssh_gateway]` 开启):

```bash
ssh -R 0:内网目标:端口 <client-id>@<server> -p <ssh_gateway.bind_port>   # 密码=客户端密钥
# 或公钥:authorized_keys_file 中列入公钥,用户名=客户端 ID
```

## Docker

官方镜像(多架构 linux/amd64 + linux/arm64,随版本标签发布):

```bash
docker run -d --name rabbithole \
  -p 7100:7100 -p 8080:8080 -p 8081:8081 -p 8082:8082 -p 8443:8443 \
  -v rabbithole-data:/var/lib/rabbithole \
  ghcr.io/xbclub/rabbithole:latest
# 首次启动:admin 随机密码与自签 TLS 证书一次性打印在容器日志,
# 配置落在数据卷 /var/lib/rabbithole/server.toml,可挂载修改
docker logs rabbithole        # 查看初始密码
```

转发端口段(默认 20000-21000)按需追加 `-p` 映射;生产请置于反向代理 + HTTPS 之后。
也可自行构建:`deploy/Dockerfile`(打包 musl 静态二进制,无 RUN 步骤,本地构建方式见文件头注释)。

## 发布(Release)

推送 `v*` 标签即触发 [release 工作流](.github/workflows/release.yml):

```bash
git tag v0.1.0 && git push origin v0.1.0
```

一次发布产出:

- **二进制归档**:Windows(zip)/ Linux / macOS(tar.gz)× x86_64 / arm64 共 6 目标,
  每个含 rh-server、rh-client、示例配置与 sha256,发布到 GitHub Release
  (Linux 为 musl 静态链接,任意发行版可直接运行)
- **Docker 镜像**:`ghcr.io/xbclub/rabbithole:<tag>` 与 `:latest`(amd64 + arm64)


## 测试与验证

```bash
cargo fmt --all && cargo clippy --workspace --all-targets && cargo test --workspace
bash tests/e2e.sh target/release        # 55 项多进程 E2E(全部代理类型/插件/健康检查/SSH/DNS/轮换/恢复/负载均衡/限速/首启引导/改密)
bash tests/e2e-resilience.sh target/release  # 19 项韧性场景(拒连/DB 故障回滚/数据通道断开/迟到 ACK/重复上线)
bash tests/e2e-transports.sh target/release  # 12 项传输变体(tcp/tls/ws/wss/quic/kcp 全链路)
bash tests/xtcp-isolated.sh target/release   # 9 项 XTCP 隔离网络(用户态 NAT 网关,打洞直连+失败回退)
bash tests/e2e-plugins.sh target/release     # 15 项客户端插件(七插件语义全链路)
bash tests/e2e-vhost.sh target/release       # 10 项 vhost 进阶(HTTPS SNI/头改写/Basic/自定义 404)
bash tests/e2e-embed.sh target/release       # 9 项内嵌控制台(单二进制托管/static_dir 覆盖;需先构建前端并重编)
bash tests/load.sh target/release 600  # 100 并发 TCP × 10 分钟持续
```

历史验证记录见 `docs/acceptance.md`;已知限制与未验证项亦在该文件列明。

## 仓库结构

```
crates/rh-protocol   RHTP 帧协议、控制消息、代理/访问器规格
crates/rh-transport  tcp/tls/ws/quic/kcp、yamux 复用、压缩、代理拨号
crates/rh-core       限速、PROXY protocol、STUN、网络工具
crates/rh-server     控制面/数据面/管理 API/vhost/SSH 网关/SQLite
crates/rh-client     会话与重连、转发、插件、健康检查、visitor、打洞
console/            管理控制台(React 19 + antd v6,Umi Max + ProComponents)
tests/               E2E 与负载测试脚本
docs/                设计文档、需求矩阵、验收记录、评审日志
deploy/              部署示例配置与 systemd 单元
```

## 许可

Apache-2.0(与基线 frp 一致)。

## 鸣谢

- **[frp](https://github.com/fatedier/frp)**(Apache-2.0)—— 功能基线与转发语义的对标项目,
  多处模块设计(插件、visitor、打洞、vhost)参考了其成熟实现
- **[ant-design-admin-v6](https://github.com/arvinaij/ant-design-admin-v6)**(MIT,
  LICENSE 见 `console/`)—— 管理控制台前端基座(Umi Max + React 19 + antd v6 + ProComponents)
- **[vue-vben-admin](https://github.com/vbenjs/vue-vben-admin)**(MIT)—— 早期控制台基座,
  已于 2026-10 移除,其交互惯例影响了首版控制台设计

