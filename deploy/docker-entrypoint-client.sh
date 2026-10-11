#!/bin/sh
# rabbithole 客户端容器入口:两种接入方式,挂载文件优先。
# 1) 挂载配置文件(高级项齐全,模板见仓库 deploy/client.example.toml):
#      -v $PWD/client.toml:/etc/rabbithole/client.toml:ro
# 2) 纯环境变量(推荐,规则由服务端集中下发,3 个字符串即可接入):
#      -e RH_SERVER=example.com:7100 -e RH_CLIENT_ID=<id> -e RH_SECRET=<secret>
# 自定义配置路径用 RH_CONFIG 指定;额外参数透传给 rh-client。
set -e

if [ -n "$RH_CONFIG" ] && [ -f "$RH_CONFIG" ]; then
  exec rh-client --config "$RH_CONFIG" "$@"
fi
if [ -f /etc/rabbithole/client.toml ]; then
  exec rh-client --config /etc/rabbithole/client.toml "$@"
fi
if [ -z "$RH_CLIENT_ID" ] || [ -z "$RH_SECRET" ]; then
  echo "[entrypoint] 未挂载配置文件,且缺少 RH_CLIENT_ID / RH_SECRET 环境变量"
  echo "[entrypoint] 最简用法:docker run -d --restart unless-stopped \\"
  echo "[entrypoint]   -e RH_SERVER=<服务端地址:端口> -e RH_CLIENT_ID=<客户端ID> -e RH_SECRET=<密钥> \\"
  echo "[entrypoint]   ghcr.io/xbclub/rabbithole-client:latest"
  echo "[entrypoint] 凭据在服务端控制台「客户端」页创建(仅展示一次);高级项(CA/代理/STUN)挂载 toml 配置"
  exit 1
fi
exec rh-client "$@"
