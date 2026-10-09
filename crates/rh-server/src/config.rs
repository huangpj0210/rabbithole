//! rh-server 配置(TOML)。示例见 deploy/rh-server.toml。

use rh_core::net::PortsRange;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct ServerConfig {
    pub bind_addr: String,
    /// 主控制监听协议:tcp | tls | ws | wss | quic | kcp(默认 tcp)。
    pub bind_protocol: String,
    pub bind_port: u16,
    /// 配置文件路径(运行期定位 DB/证书)。
    #[serde(skip)]
    pub config_path: std::path::PathBuf,
    /// 附加传输监听(quic/kcp/ws/tls 等多协议并存)。
    pub extra_listeners: Vec<ListenerConfig>,
    /// TCP 传输是否启用 yamux 多路复用。
    pub multiplex: bool,
    pub tls: Option<TlsServerFileConfig>,
    pub web: WebConfig,
    pub vhost: VhostConfig,
    pub ports: PortsConfig,
    pub limits: LimitsConfig,
    pub ssh_gateway: SshGatewayConfig,
    pub nathole: NatholeConfig,
    pub metrics: MetricsConfig,
    pub log: LogConfig,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct ListenerConfig {
    /// tcp | tls | ws | wss | quic | kcp
    pub protocol: String,
    pub bind_port: u16,
    pub bind_addr: Option<String>,
    pub multiplex: Option<bool>,
    pub tls: Option<TlsServerFileConfig>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct TlsServerFileConfig {
    pub cert_file: Option<PathBuf>,
    pub key_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct WebConfig {
    /// 管理 API 监听;默认仅本机。
    pub bind_addr: String,
    pub port: u16,
    /// 前端静态资源目录(可选)。
    pub static_dir: Option<PathBuf>,
    /// JWT 签名密钥;空则启动时生成(重启后 token 失效)。
    pub jwt_secret: Option<String>,
    pub jwt_ttl_hours: u64,
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            bind_addr: "127.0.0.1".into(),
            port: 8080,
            static_dir: None,
            jwt_secret: None,
            jwt_ttl_hours: 12,
            tls_cert: None,
            tls_key: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct VhostConfig {
    /// HTTP vhost 共享监听端口;0 = 关闭。
    pub http_port: u16,
    /// HTTPS vhost(SNI)共享监听端口;0 = 关闭。
    pub https_port: u16,
    /// tcpmux HTTP CONNECT 共享监听端口;0 = 关闭。
    pub tcpmux_port: u16,
    /// tcpmux passthrough 模式。
    pub tcpmux_passthrough: bool,
    /// 子域名后缀(subdomain 路由)。
    pub subdomain_host: String,
    /// vhost HTTP 空闲超时(秒)。
    pub http_timeout_s: u64,
    pub custom_404_page: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
#[derive(Default)]
pub struct PortsConfig {
    /// 自动分配范围(空 = 10000-25000)。
    pub auto_range: Option<String>,
    /// 白名单(空 = 全部允许)。
    pub allow_ports: Vec<String>,
    /// 每客户端最大规则数;0 = 不限。
    pub max_services_per_client: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct LimitsConfig {
    pub max_pool_size: u8,
    pub default_pool_size: u8,
    /// 每客户端并发数据连接上限(池+在途+转发中)。
    pub max_data_conns_per_client: usize,
    /// 每客户端 UDP 会话映射上限。
    pub max_udp_sessions_per_proxy: usize,
    /// 控制连接发送队列上限。
    pub control_send_queue: usize,
    /// 每 IP 未认证并发连接上限。
    pub unauth_conns_per_ip: usize,
    /// UDP 报文上限。
    pub udp_packet_size: usize,
    /// 用户连接等待池补拨总额(毫秒)。
    pub pool_wait_timeout_ms: u64,
}

impl Default for LimitsConfig {
    fn default() -> Self {
        Self {
            max_pool_size: 16,
            default_pool_size: 2,
            max_data_conns_per_client: 256,
            max_udp_sessions_per_proxy: 1024,
            control_send_queue: 256,
            unauth_conns_per_ip: 64,
            udp_packet_size: 64 * 1024,
            pool_wait_timeout_ms: 10_000,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct SshGatewayConfig {
    pub enabled: bool,
    pub bind_port: u16,
    pub bind_addr: String,
    pub authorized_keys_file: Option<PathBuf>,
    pub auto_gen_key_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct NatholeConfig {
    /// 打洞 STUN 服务器(host:port);客户端用。
    pub stun_server: String,
    pub punch_timeout_s: u64,
    pub prepare_timeout_s: u64,
}

impl Default for NatholeConfig {
    fn default() -> Self {
        Self {
            stun_server: String::new(),
            punch_timeout_s: 5,
            prepare_timeout_s: 5,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct MetricsConfig {
    pub enable_prometheus: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct LogConfig {
    pub level: String,
    pub file: Option<PathBuf>,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: "info".into(),
            file: None,
        }
    }
}

impl ServerConfig {
    pub fn load(path: &std::path::Path) -> anyhow::Result<Self> {
        let mut cfg: ServerConfig = toml::from_str(&std::fs::read_to_string(path)?)?;
        cfg.config_path = path.to_path_buf();
        Ok(cfg)
    }

    pub fn auto_port_ranges(&self) -> Vec<PortsRange> {
        match &self.ports.auto_range {
            Some(s) => PortsRange::parse(s).map(|r| vec![r]).unwrap_or_default(),
            None => vec![PortsRange {
                start: 10000,
                end: 25000,
            }],
        }
    }

    pub fn allow_ports(&self) -> Vec<PortsRange> {
        self.ports
            .allow_ports
            .iter()
            .filter_map(|s| PortsRange::parse(s))
            .collect()
    }
}
