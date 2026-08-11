//! 热重载模块
//!
//! [`MapperWatcher`] 监控 mapper XML 文件变更，经去抖后在后台线程重新解析并替换
//! 注册表中对应的 `Mapper`（通过 `MapperRegistry::insert_mapper`，线程安全）。
//!
//! 由 [`crate::session_factory::SqlSessionFactory`] 在 `mapper_refresh_interval_ms > 0`
//! 时启动，生命周期与工厂相同。

pub mod watcher;

pub use watcher::{extract_watch_dirs, MapperWatcher};
