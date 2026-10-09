//! visitor 配置类型:STCP/SUDP/XTCP 的访问端规则,运行在指定的客户端上。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VisitorKind {
    Stcp,
    Sudp,
    Xtcp,
}

impl VisitorKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            VisitorKind::Stcp => "stcp",
            VisitorKind::Sudp => "sudp",
            VisitorKind::Xtcp => "xtcp",
        }
    }
}

impl std::fmt::Display for VisitorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// visitor 规格:客户端本地监听并把连接(或 UDP)经服务端中继到目标代理。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VisitorSpec {
    /// 创建时缺省,由服务端分配。
    #[serde(default)]
    pub id: u64,
    pub name: String,
    pub kind: VisitorKind,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 客户端本地监听地址(stcp/xtcp 的 TCP listener;sudp 的 UDP bind)。
    #[serde(default = "default_visitor_bind")]
    pub bind_addr: String,
    pub bind_port: u16,
    /// 目标代理名(全局唯一目标代理,由服务端解析归属客户端)。
    pub target_proxy_name: String,
    /// 访问密钥;与目标代理 secret_key 匹配才允许中继。
    #[serde(default)]
    pub secret_key: String,
    /// visitor 侧通道选项(压缩与代理级传输一致语义)。
    #[serde(default)]
    pub use_compression: bool,
}

fn default_true() -> bool {
    true
}
fn default_visitor_bind() -> String {
    "127.0.0.1".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let v = VisitorSpec {
            id: 3,
            name: "v".into(),
            kind: VisitorKind::Stcp,
            enabled: true,
            bind_addr: "127.0.0.1".into(),
            bind_port: 6000,
            target_proxy_name: "svc".into(),
            secret_key: "k".into(),
            use_compression: false,
        };
        let s = serde_json::to_string(&v).unwrap();
        let back: VisitorSpec = serde_json::from_str(&s).unwrap();
        assert_eq!(v, back);
    }
}
