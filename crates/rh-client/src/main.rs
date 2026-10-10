//! rh-client 入口:配置加载 + 会话主循环(断线退避重连)。

mod config;
mod dnsquery;
mod forward;
mod health;
mod nathole_cli;
mod plugins;
mod session;
mod udpchan;
mod visitor_run;

use clap::Parser;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

#[derive(Parser)]
#[command(name = "rh-client", version)]
struct Cli {
    #[arg(short, long, default_value = "rh-client.toml")]
    config: std::path::PathBuf,
    /// 覆盖服务器地址(host:port)。
    #[arg(long)]
    server: Option<String>,
    /// 覆盖客户端 ID。
    #[arg(long)]
    id: Option<String>,
    /// 覆盖共享密钥。
    #[arg(long)]
    secret: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let mut cfg = config::ClientConfig::load(&cli.config)?;
    if let Some(s) = cli.server {
        cfg.server_addr = s;
    }
    if let Some(i) = cli.id {
        cfg.client_id = i;
    }
    if let Some(s) = cli.secret {
        cfg.secret = s;
    }
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&cfg.log_level));
    tracing_subscriber::fmt().with_env_filter(filter).init();
    anyhow::ensure!(!cfg.client_id.is_empty(), "client_id required");
    anyhow::ensure!(!cfg.secret.is_empty(), "secret required");

    let cfg = Arc::new(cfg);
    dnsquery::set_server(cfg.dns_server.clone());
    let shutdown = CancellationToken::new();
    {
        let sd = shutdown.clone();
        tokio::spawn(async move {
            let ctrl_c = tokio::signal::ctrl_c();
            #[cfg(unix)]
            {
                let mut term =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                        .expect("SIGTERM handler");
                tokio::select! {
                    _ = ctrl_c => {}
                    _ = term.recv() => {}
                }
            }
            #[cfg(not(unix))]
            {
                let _ = ctrl_c.await;
            }
            info!("shutting down");
            sd.cancel();
        });
    }

    let mut backoff_secs = 1u64;
    loop {
        if shutdown.is_cancelled() {
            return Ok(());
        }
        let started = std::time::Instant::now();
        match session::connect_and_run(cfg.clone(), shutdown.clone()).await {
            Ok(()) => {
                info!("session ended");
            }
            Err(e) => {
                warn!(error = %e, "session error");
            }
        }
        // 会话存活超过 30s 视为健康周期:退避重置,避免长期运行后
        // 任意一次断线都要等满 60s 才重连
        if started.elapsed() >= std::time::Duration::from_secs(30) {
            backoff_secs = 1;
        }
        if shutdown.is_cancelled() {
            return Ok(());
        }
        let wait = std::time::Duration::from_secs(backoff_secs);
        info!(retry_in = backoff_secs, "reconnecting");
        tokio::select! {
            _ = tokio::time::sleep(wait) => {}
            _ = shutdown.cancelled() => return Ok(()),
        }
        backoff_secs = (backoff_secs * 2).min(60);
    }
}
