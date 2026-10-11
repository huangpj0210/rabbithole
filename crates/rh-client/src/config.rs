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

    /// 环境变量覆盖层(RH_*):必填三样 + 常用开关,未设置的变量跳过。
    /// 与配置文件的关系:文件承载全部字段(含 CA/代理/STUN 等高级项),
    /// env 只做薄覆盖——Docker / systemd / k8s 场景传 3 个字符串即可接入,
    /// 无需为客户端准备配置文件(规则本就由服务端集中下发)。
    pub fn apply_env(&mut self) -> anyhow::Result<()> {
        let s = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        if let Some(v) = s("RH_SERVER") {
            self.server_addr = v;
        }
        if let Some(v) = s("RH_CLIENT_ID") {
            self.client_id = v;
        }
        if let Some(v) = s("RH_SECRET") {
            self.secret = v;
        }
        if let Some(v) = s("RH_PROTOCOL") {
            self.protocol = v;
        }
        if let Some(v) = s("RH_MULTIPLEX") {
            self.multiplex = parse_env_bool("RH_MULTIPLEX", &v)?;
        }
        if let Some(v) = s("RH_TLS_INSECURE") {
            self.tls_insecure_skip_verify = parse_env_bool("RH_TLS_INSECURE", &v)?;
        }
        if let Some(v) = s("RH_TLS_PIN_SHA256") {
            self.tls_pin_sha256 = Some(v);
        }
        if let Some(v) = s("RH_TRUSTED_CA_FILE") {
            self.trusted_ca_file = Some(v);
        }
        if let Some(v) = s("RH_SERVER_NAME") {
            self.server_name = Some(v);
        }
        if let Some(v) = s("RH_PROXY_URL") {
            self.proxy_url = Some(v);
        }
        if let Some(v) = s("RH_STUN_SERVER") {
            self.stun_server = Some(v);
        }
        if let Some(v) = s("RH_DNS_SERVER") {
            self.dns_server = Some(v);
        }
        if let Some(v) = s("RH_LOG_LEVEL") {
            self.log_level = v;
        }
        Ok(())
    }
}

/// env 布尔值:1/true/yes/on 与 0/false/no/off(大小写不敏感),其余报错。
fn parse_env_bool(name: &str, v: &str) -> anyhow::Result<bool> {
    match v.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(anyhow::anyhow!(
            "环境变量 {name} 取值非法(应为 true/false):{v}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENV_KEYS: &[&str] = &[
        "RH_SERVER",
        "RH_CLIENT_ID",
        "RH_SECRET",
        "RH_PROTOCOL",
        "RH_MULTIPLEX",
        "RH_TLS_INSECURE",
    ];

    /// 单测试函数串行执行:std::env 是进程级全局,避免并行测试互相踩。
    #[test]
    fn env_overrides_layering() {
        for k in ENV_KEYS {
            std::env::remove_var(k);
        }
        // 无 env:保持默认
        let mut c = ClientConfig::default();
        c.apply_env().unwrap();
        assert_eq!(c.server_addr, "127.0.0.1:7100");
        assert!(c.multiplex);

        // 有 env:覆盖默认;布尔解析大小写不敏感
        std::env::set_var("RH_SERVER", "example.com:7100");
        std::env::set_var("RH_CLIENT_ID", "cid-1");
        std::env::set_var("RH_SECRET", "sec-1");
        std::env::set_var("RH_MULTIPLEX", "off");
        std::env::set_var("RH_TLS_INSECURE", "TRUE");
        c.apply_env().unwrap();
        assert_eq!(c.server_addr, "example.com:7100");
        assert_eq!(c.client_id, "cid-1");
        assert_eq!(c.secret, "sec-1");
        assert!(!c.multiplex);
        assert!(c.tls_insecure_skip_verify);

        // 非法布尔值:报错且带变量名
        std::env::set_var("RH_MULTIPLEX", "maybe");
        let err = c.apply_env().unwrap_err().to_string();
        assert!(err.contains("RH_MULTIPLEX"), "err: {err}");

        for k in ENV_KEYS {
            std::env::remove_var(k);
        }
    }
}
