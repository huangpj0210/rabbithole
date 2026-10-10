//! 构建脚本:检测前端产物目录是否存在,存在则启用 `embed_ui` cfg,
//! 由 rust-embed 在编译期把 Web 控制台嵌进二进制。
//!
//! 双路径择一:新 React 控制台(`console/dist`)优先,旧 vben UI
//! (`web/apps/web-antd/dist`)兜底;选中哪个就置对应的 `embed_ui_console` /
//! `embed_ui_legacy` cfg,`embedded_ui.rs` 据此选择 rust-embed 的 folder。
//! 这样干净检出(未构建前端)时纯 Rust 构建依然可用(控制台需 static_dir
//! 或先构建前端再重编);dist 目录内容变化会触发本 crate 重编。
//! CUT 阶段移除旧路径与 legacy cfg。

use std::path::PathBuf;

fn main() {
    println!("cargo:rustc-check-cfg=cfg(embed_ui)");
    println!("cargo:rustc-check-cfg=cfg(embed_ui_console)");
    println!("cargo:rustc-check-cfg=cfg(embed_ui_legacy)");
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let candidates: [(&str, &str); 2] = [
        ("../../console/dist", "embed_ui_console"),
        ("../../web/apps/web-antd/dist", "embed_ui_legacy"),
    ];
    for (rel, _) in &candidates {
        println!("cargo:rerun-if-changed={}", manifest.join(rel).display());
    }
    for (rel, cfg) in candidates {
        if manifest.join(rel).join("index.html").is_file() {
            println!("cargo:rustc-cfg=embed_ui");
            println!("cargo:rustc-cfg={cfg}");
            println!(
                "cargo:warning=rh-server: embedding web console from {}",
                manifest.join(rel).display()
            );
            break;
        }
    }
}
