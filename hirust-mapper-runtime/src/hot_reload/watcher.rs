//! Mapper 热重载监视器
//!
//! 基于 [`notify::RecommendedWatcher`] 监控文件系统事件，专用线程收集变更并去抖，
//! 安静期（默认 200ms，可配）后批量重新解析变更的 XML 文件，通过
//! [`MapperRegistry::insert_mapper`] 原子替换。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};

use crate::error::{MapperRuntimeError, Result};
use crate::registry::MapperRegistry;

/// 最小去抖间隔（毫秒），防止过快刷新
const MIN_DEBOUNCE_MS: u64 = 50;

/// Mapper 热重载监视器
///
/// 持有底层 notify watcher 与去抖 worker 线程。drop 时优雅关闭两者。
pub struct MapperWatcher {
    /// notify 文件监视器（None 表示已关闭）
    watcher: Option<RecommendedWatcher>,
    /// 去抖 worker 线程句柄
    worker: Option<std::thread::JoinHandle<()>>,
    /// 向 worker 发送停止信号
    stop_tx: mpsc::Sender<()>,
}

impl MapperWatcher {
    /// 启动热重载监视器
    ///
    /// - `registry`：共享的 Mapper 注册表（变更后调用 `register_from_file` 重新解析）
    /// - `watch_dirs`：待监视的目录列表（递归监视）
    /// - `debounce_ms`：去抖间隔（安静期后触发重解析）
    pub fn start(
        registry: MapperRegistry,
        watch_dirs: Vec<PathBuf>,
        debounce_ms: u64,
    ) -> Result<Self> {
        if watch_dirs.is_empty() {
            return Err(MapperRuntimeError::HotReload(
                "无可监视的目录，热重载未启动".to_string(),
            ));
        }

        let debounce = Duration::from_millis(debounce_ms.max(MIN_DEBOUNCE_MS));

        // 事件通道：notify 回调（同步）→ worker 线程
        let (event_tx, event_rx) = mpsc::channel::<PathBuf>();

        let mut watcher = RecommendedWatcher::new(
            move |res: std::result::Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    for path in &event.paths {
                        // 忽略发送错误（worker 已退出）
                        let _ = event_tx.send(path.clone());
                    }
                }
            },
            Config::default(),
        )
        .map_err(|e| MapperRuntimeError::HotReload(format!("创建 watcher 失败: {}", e)))?;

        // 递归监视各目录
        let mut watched = 0;
        for dir in &watch_dirs {
            if dir.is_dir() {
                watcher
                    .watch(dir, RecursiveMode::Recursive)
                    .map_err(|e| {
                        MapperRuntimeError::HotReload(format!(
                            "监视目录 {} 失败: {}",
                            dir.display(),
                            e
                        ))
                    })?;
                watched += 1;
            }
        }
        if watched == 0 {
            return Err(MapperRuntimeError::HotReload(format!(
                "目录均不存在，热重载未启动: {:?}",
                watch_dirs
            )));
        }

        // 去抖 worker 线程
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let worker = std::thread::Builder::new()
            .name("hirust-mapper-hot-reload".into())
            .spawn(move || {
                let mut pending: HashSet<PathBuf> = HashSet::new();
                loop {
                    match event_rx.recv_timeout(debounce) {
                        Ok(path) => {
                            // 仅关心 XML 文件
                            if is_xml(&path) {
                                pending.insert(path);
                            }
                        }
                        Err(RecvTimeoutError::Timeout) => {
                            // 安静期结束：批量重解析
                            if !pending.is_empty() {
                                let paths: Vec<PathBuf> = pending.drain().collect();
                                for path in paths {
                                    reload_mapper(&registry, &path);
                                }
                            }
                            // 检查停止信号
                            if stop_rx.try_recv().is_ok() {
                                break;
                            }
                        }
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
            })
            .map_err(|e| MapperRuntimeError::HotReload(format!("启动热重载线程失败: {}", e)))?;

        Ok(Self {
            watcher: Some(watcher),
            worker: Some(worker),
            stop_tx,
        })
    }

    /// 是否正在运行
    pub fn is_running(&self) -> bool {
        self.watcher.is_some()
    }
}

impl Drop for MapperWatcher {
    fn drop(&mut self) {
        // 1. 先关闭 watcher，断开事件通道 → worker 的 recv 返回 Disconnected
        self.watcher.take();
        // 2. 发送停止信号（belt-and-suspenders）
        let _ = self.stop_tx.send(());
        // 3. 等待 worker 退出
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// 重新解析单个 mapper 文件并原子替换注册表中的 Mapper
fn reload_mapper(registry: &MapperRegistry, path: &Path) {
    match registry.register_from_file(path) {
        Ok(namespace) => {
            // 热重载成功（生产环境可接入 tracing/log）
            log_info(&format!("热重载成功: {} ({})", namespace, path.display()));
        }
        Err(e) => {
            log_warn(&format!("热重载失败 {}: {}", path.display(), e));
        }
    }
}

fn is_xml(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).map(|s| s.eq_ignore_ascii_case("xml")).unwrap_or(false)
}

/// 轻量日志：当前用 eprintln（避免引入 log 依赖；P10 可替换为 tracing）
fn log_info(msg: &str) {
    eprintln!("[hirust-mapper] {}", msg);
}
fn log_warn(msg: &str) {
    eprintln!("[hirust-mapper][WARN] {}", msg);
}

/// 从 mapper glob 模式列表推导出需监视的目录
///
/// 规则：取每个模式中第一个通配符之前的静态前缀作为路径，相对 `base_dir` 解析；
/// 若该路径指向文件（有扩展名），取其父目录。结果去重。
pub fn extract_watch_dirs(mapper_paths: &[String], base_dir: &Path) -> Vec<PathBuf> {
    let mut dirs: HashSet<PathBuf> = HashSet::new();
    for pattern in mapper_paths {
        // 截取首个通配符 (* ? [ {) 之前的静态前缀
        let static_prefix: String = pattern
            .chars()
            .take_while(|c| !matches!(c, '*' | '?' | '[' | '{'))
            .collect();

        let trimmed = static_prefix.trim_end_matches('/');
        if trimmed.is_empty() {
            // 模式以通配符开头 → 监视 base_dir
            dirs.insert(base_dir.to_path_buf());
            continue;
        }

        let prefix_path = Path::new(trimmed);
        let resolved = if prefix_path.is_absolute() {
            prefix_path.to_path_buf()
        } else {
            base_dir.join(prefix_path)
        };

        // 若解析结果像文件（有扩展名），取父目录
        let dir = if resolved.extension().is_some() {
            resolved.parent().map(|p| p.to_path_buf()).unwrap_or(resolved)
        } else {
            resolved
        };

        dirs.insert(dir);
    }
    dirs.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_watch_dirs_glob_recursive() {
        let base = Path::new("/app");
        let dirs = extract_watch_dirs(&["mappers/**/*.xml".to_string()], base);
        assert_eq!(dirs.len(), 1);
        assert!(dirs.contains(&PathBuf::from("/app/mappers")));
    }

    #[test]
    fn test_extract_watch_dirs_specific_file() {
        let base = Path::new("/app");
        let dirs = extract_watch_dirs(&["mappers/User.xml".to_string()], base);
        // 文件模式 → 取父目录 mappers
        assert!(dirs.contains(&PathBuf::from("/app/mappers")));
    }

    #[test]
    fn test_extract_watch_dirs_multiple_dedup() {
        let base = Path::new("/app");
        let dirs = extract_watch_dirs(
            &["mappers/**/*.xml".to_string(), "mappers/Order.xml".to_string()],
            base,
        );
        // 两者都解析到 /app/mappers → 去重为 1
        assert_eq!(dirs.len(), 1);
    }

    #[test]
    fn test_extract_watch_dirs_wildcard_start() {
        let base = Path::new("/app");
        let dirs = extract_watch_dirs(&["**/*.xml".to_string()], base);
        // 模式以通配符开头 → 监视 base_dir
        assert!(dirs.contains(&PathBuf::from("/app")));
    }

    #[test]
    fn test_is_xml() {
        assert!(is_xml(Path::new("a.xml")));
        assert!(is_xml(Path::new("a.XML"))); // 大小写不敏感
        assert!(!is_xml(Path::new("a.txt")));
        assert!(!is_xml(Path::new("noext")));
    }
}
