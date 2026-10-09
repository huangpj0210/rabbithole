//! 事件审计:落库(供 API 查询)。

use crate::db::Db;

#[derive(Clone)]
pub struct EventBus {
    db: Db,
}

impl EventBus {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    pub async fn emit(&self, level: &str, source: &str, message: &str) {
        self.emit_detail(level, source, message, "").await;
    }

    pub async fn emit_detail(&self, level: &str, source: &str, message: &str, detail: &str) {
        tracing::info!(level, source, message, "event");
        let _ = self.db.insert_event(level, source, message, detail).await;
    }
}
