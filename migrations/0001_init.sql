-- rabbithole 初始 schema(SQLite)
-- 外键与约束真实生效:PRAGMA foreign_keys=ON(sqlx 连接选项设置)。

CREATE TABLE users (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  username      TEXT NOT NULL UNIQUE,
  password_hash TEXT NOT NULL,
  role          TEXT NOT NULL DEFAULT 'admin' CHECK (role IN ('admin','viewer')),
  created_at    INTEGER NOT NULL
);

CREATE TABLE clients (
  id              TEXT PRIMARY KEY,             -- uuid v4
  name            TEXT NOT NULL UNIQUE,
  secret          TEXT NOT NULL,                -- 64 hex,HMAC 校验需原文
  enabled         INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0,1)),
  remark          TEXT NOT NULL DEFAULT '',
  config_revision INTEGER NOT NULL DEFAULT 0,   -- 每客户端期望配置版本,与变更同事务推进
  created_at      INTEGER NOT NULL,
  last_online_at  INTEGER,
  client_version  TEXT NOT NULL DEFAULT '',
  hostname        TEXT NOT NULL DEFAULT '',
  os              TEXT NOT NULL DEFAULT ''
);

CREATE TABLE proxies (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  client_id   TEXT NOT NULL REFERENCES clients(id) ON DELETE CASCADE,
  name        TEXT NOT NULL,
  kind        TEXT NOT NULL CHECK (kind IN ('tcp','udp','http','https','stcp','sudp','xtcp','tcpmux')),
  enabled     INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0,1)),
  bind_ip     TEXT NOT NULL DEFAULT '0.0.0.0',
  bind_port   INTEGER NOT NULL DEFAULT 0 CHECK (bind_port BETWEEN 0 AND 65535),
  port_auto   INTEGER NOT NULL DEFAULT 0 CHECK (port_auto IN (0,1)),
  target_host TEXT NOT NULL DEFAULT '127.0.0.1',
  target_port INTEGER NOT NULL DEFAULT 0 CHECK (target_port BETWEEN 0 AND 65535),
  pool_size   INTEGER NOT NULL DEFAULT 0,
  group_name  TEXT NOT NULL DEFAULT '',
  group_key   TEXT NOT NULL DEFAULT '',
  source      TEXT NOT NULL DEFAULT 'console' CHECK (source IN ('console','ssh')),
  spec        TEXT NOT NULL DEFAULT '{}',       -- 完整 ProxySpec JSON(transport/插件/健康检查/kind 配置)
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL,
  UNIQUE (client_id, name)
);
CREATE INDEX idx_proxies_client ON proxies(client_id);
CREATE INDEX idx_proxies_bind ON proxies(kind, bind_port) WHERE bind_port > 0;

CREATE TABLE visitors (
  id                INTEGER PRIMARY KEY AUTOINCREMENT,
  client_id         TEXT NOT NULL REFERENCES clients(id) ON DELETE CASCADE,
  name              TEXT NOT NULL,
  kind              TEXT NOT NULL CHECK (kind IN ('stcp','sudp','xtcp')),
  enabled           INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0,1)),
  bind_addr         TEXT NOT NULL DEFAULT '127.0.0.1',
  bind_port         INTEGER NOT NULL CHECK (bind_port BETWEEN 0 AND 65535),
  target_proxy_name TEXT NOT NULL,
  target_client_id  TEXT NOT NULL,              -- 目标代理归属客户端(创建时解析固定)
  secret_key        TEXT NOT NULL DEFAULT '',
  use_compression   INTEGER NOT NULL DEFAULT 0,
  created_at        INTEGER NOT NULL,
  updated_at        INTEGER NOT NULL,
  UNIQUE (client_id, name)
);
CREATE INDEX idx_visitors_client ON visitors(client_id);

CREATE TABLE events (
  id      INTEGER PRIMARY KEY AUTOINCREMENT,
  ts      INTEGER NOT NULL,
  level   TEXT NOT NULL CHECK (level IN ('info','warn','error')),
  source  TEXT NOT NULL,
  message TEXT NOT NULL,
  detail  TEXT NOT NULL DEFAULT ''
);
CREATE INDEX idx_events_ts ON events(ts);
