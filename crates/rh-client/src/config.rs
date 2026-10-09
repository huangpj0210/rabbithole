//! 客户端配置(TOML)。

use rh_transport::Protocol;
use serde::Deserialize;
use std::net::SocketAddr;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ClientConfig {
    pub server_addr: String,
    /// tcp | tls | ws | wss | quic | kcp。
    pub protocol: String,
    /// tcp/tls/ws/wss 是否启用 yamux 多路复用(与服务端一致)。
    pub multiplex: bool,
    pub client_id: String,
    pub secret: String,
    /// TLS。
    pub trusted_ca_file: Option<String>,
    pub server_name: Option<String>,
    pub tls_insecure_skip_verify: bool,
    pub tls_pin_sha256: Option<String>,
    /// HTTP/SOCKS5 代理拨号(http://user:pass@host:port 或 socks5://…)。
    pub proxy_url: Option<String>,
    /// XTCP 打洞 STUN 服务器;空 = 以本机地址作候选。
    pub stun_server: Option<String>,
    /// 自定义 DNS 服务器(host:port);空 = 系统解析。
    pub dns_server: Option<String>,
    pub log_level: String,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            server_addr: "127.0.0.1:7100".into(),
            protocol: "tcp".into(),
            multiplex: true,
            client_id: String::new(),
            secret: String::new(),
            trusted_ca_file: None,
            server_name: None,
            tls_insecure_skip_verify: false,
            tls_pin_sha256: None,
            proxy_url: None,
            stun_server: None,
            dns_server: None,
            log_level: "info".into(),
        }
    }
}

impl ClientConfig {
    pub fn load(path: &std::path::Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("read config {path:?}: {e}"))?;
        Ok(toml::from_str(&text)?)
    }

    pub fn transport_cfg(&self) -> anyhow::Result<rh_transport::ClientTransportConfig> {
        let protocol = Protocol::parse(&self.protocol)
            .ok_or_else(|| anyhow::anyhow!("bad protocol {}", self.protocol))?;
        let server_addr: SocketAddr = self
            .server_addr
            .parse()
            .map_err(|_| anyhow::anyhow!("bad server_addr {}", self.server_addr))?;
        Ok(rh_transport::ClientTransportConfig {
            protocol,
            server_addr,
            multiplex: self.multiplex,
            tls: rh_transport::TlsClientOptions {
                trusted_ca: self
                    .trusted_ca_file
                    .as_ref()
                    .map(std::fs::read)
                    .transpose()?,
                server_name: self.server_name.clone(),
                insecure_skip_verify: self.tls_insecure_skip_verify,
                pin_sha256: self.tls_pin_sha256.clone(),
            },
            proxy_url: self.proxy_url.clone(),
            alpn: vec!["rhtp".into()],
            connect_timeout: std::time::Duration::from_secs(10),
        })
    }
}
