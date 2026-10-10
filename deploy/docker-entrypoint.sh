#!/bin/sh
# rabbithole 容器入口:数据卷无配置时写入默认配置,再启动 rh-server。
# 数据库(rabbithole.db)与自动生成的 TLS 证书都落在配置同目录(即数据卷)。
set -e

DATA_DIR=/var/lib/rabbithole
mkdir -p "$DATA_DIR"

if [ ! -f "$DATA_DIR/server.toml" ]; then
  cp /etc/rabbithole/server.toml "$DATA_DIR/server.toml"
  echo "[entrypoint] 已初始化默认配置:$DATA_DIR/server.toml"
fi

exec rh-server --config "$DATA_DIR/server.toml"
