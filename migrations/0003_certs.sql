-- F6/M3 HTTPS 证书管理(SNI 选证书)。
CREATE TABLE certs (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  name        TEXT NOT NULL,
  domains     TEXT NOT NULL DEFAULT '[]',   -- JSON 数组;空则按证书 SAN/CN 自动提取
  cert_pem    TEXT NOT NULL,
  key_pem     TEXT NOT NULL,                -- 任何 API 响应不回显
  fingerprint TEXT NOT NULL DEFAULT '',     -- sha256(DER) 前 32 hex
  not_after   INTEGER NOT NULL DEFAULT 0,   -- unix ms
  created_at  INTEGER NOT NULL
);
