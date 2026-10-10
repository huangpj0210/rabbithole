//! 构建脚本:检测前端产物目录是否存在,存在则启用 `embed_ui` cfg,
//! 由 rust-embed 在编译期把 Web 控制台嵌进二进制。
//!
//! 产物来源:顶层 `console/`(React + antd v6,Umi Max)。
//! 这样干净检出(未构建前端)时纯 Rust 构建依然可用(控制台需 static_dir 或
//! 先构建前端再重编);`pnpm build`(console/)产出/更新 dist 后重新 cargo build
//! 即自动内嵌。dist 目录内容变化会触发本 crate 重编(rerun-if-changed)。

use std::path::PathBuf;

fn main() {
    println!("cargo:rustc-check-cfg=cfg(embed_ui)");
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let dist = manifest.join("../../console/dist");
    println!("cargo:rerun-if-changed={}", dist.display());
    if dist.join("index.html").is_file() {
        println!("cargo:rustc-cfg=embed_ui");
        println!(
            "cargo:warning=rh-server: embedding web console from {}",
            dist.display()
        );
    }
}
