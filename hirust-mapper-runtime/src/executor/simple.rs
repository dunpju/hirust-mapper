//! SimpleExecutor：基础的 SQL 执行器
//!
//! 串联 [`ParameterHandler`]（绑定）→ sqlx 执行 → [`ResultSetHandler`]（映射）。
//! 所有方法以泛型 `E: sqlx::Executor` 接收执行目标，因此同一套逻辑可作用于
//! 连接池（`&AnyPool`）或事务连接（`&mut AnyConnection`）。

use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use futures_util::{Stream, StreamExt};
use hirust_mapper_core::BoundSql;
use serde::de::DeserializeOwned;
use sqlx::any::{AnyQueryResult, AnyRow};
use sqlx::Executor;

use crate::error::Result;
use crate::event::lifecycle::{classify_sql, AfterSqlEvent, BeforeSqlEvent, SqlKind, SqlOutcome};
use crate::event::EventBus;
use crate::handler::parameter::ParameterHandler;
use crate::handler::result_set::ResultSetHandler;
use crate::sql_log::SqlLogConfig;
use crate::type_handler::TypeHandlerRegistry;

/// 基础执行器
///
/// 无状态（除类型处理器注册表、SQL 日志配置与事件总线），可被多个 Session 共享。
pub struct SimpleExecutor {
    type_handler_registry: Arc<TypeHandlerRegistry>,
    sql_log: Arc<SqlLogConfig>,
    event_bus: Arc<EventBus>,
}

impl SimpleExecutor {
    /// 使用指定类型处理器注册表创建
    pub fn new(type_handler_registry: Arc<TypeHandlerRegistry>) -> Self {
        Self {
            type_handler_registry,
            sql_log: Arc::new(SqlLogConfig::default()),
            event_bus: Arc::new(EventBus::new()),
        }
    }

    /// 使用默认内置类型处理器创建
    pub fn with_defaults() -> Self {
        Self::new(Arc::new(TypeHandlerRegistry::with_defaults()))
    }

    /// 设置 SQL 日志配置（开启执行日志 / 慢查询阈值）
    pub fn with_sql_log(mut self, sql_log: Arc<SqlLogConfig>) -> Self {
        self.sql_log = sql_log;
        self
    }

    /// 设置事件总线（用于派发 SQL 执行前/后生命周期事件）
    pub fn with_event_bus(mut self, event_bus: Arc<EventBus>) -> Self {
        self.event_bus = event_bus;
        self
    }

    /// 事件总线引用
    pub fn event_bus(&self) -> &EventBus {
        &self.event_bus
    }

    /// 类型处理器注册表
    pub fn type_handler_registry(&self) -> &TypeHandlerRegistry {
        &self.type_handler_registry
    }

    /// 执行查询，返回原始行（未映射）
    pub async fn query_rows<'q, E>(&self, bound: &'q BoundSql, executor: E) -> Result<Vec<AnyRow>>
    where
        E: Executor<'q, Database = sqlx::Any>,
    {
        let args = ParameterHandler::bind_arguments(bound)?;
        let bus = &self.event_bus;
        bus.dispatch_if(|| BeforeSqlEvent {
            raw_sql: bound.sql.clone(),
            params: bound.parameters.clone(),
            kind: SqlKind::Select,
        });
        let start = Instant::now();
        let result = sqlx::query_with(sqlx::AssertSqlSafe(&*bound.sql), args)
            .fetch_all(executor)
            .await
            .map_err(crate::error::MapperRuntimeError::from);
        let elapsed = start.elapsed();
        crate::sql_log::log_execution(&self.sql_log, bound, elapsed);
        bus.dispatch_if(|| AfterSqlEvent {
            raw_sql: bound.sql.clone(),
            params: bound.parameters.clone(),
            kind: SqlKind::Select,
            elapsed,
            outcome: match &result {
                Ok(rows) => SqlOutcome::Fetched(rows.len()),
                Err(e) => SqlOutcome::Failed(e.to_string()),
            },
        });
        result
    }

    /// 流式查询：返回逐行的行流（按需拉取，避免 [`query_rows`](Self::query_rows) 的 `fetch_all`
    /// 一次性物化整表）。适用于大结果集、低内存峰值场景。
    ///
    /// 流借用调用方提供的 `bound` 与 `executor`。调用方配合 `futures_util::StreamExt` 消费：
    /// ```ignore
    /// use futures_util::StreamExt;
    /// let mut stream = executor.query_rows_stream(&bound, &pool);
    /// while let Some(row) = stream.next().await {
    ///     let row = row?;
    ///     // ...
    /// }
    /// ```
    pub fn query_rows_stream<'q, E>(
        &self,
        bound: &'q BoundSql,
        executor: E,
    ) -> Pin<Box<dyn Stream<Item = Result<AnyRow>> + Send + 'q>>
    where
        E: Executor<'q, Database = sqlx::Any> + Send + 'q,
    {
        let args = match ParameterHandler::bind_arguments(bound) {
            Ok(a) => a,
            Err(e) => return Box::pin(futures_util::stream::once(async move { Err(e) })),
        };
        let s = sqlx::query_with(sqlx::AssertSqlSafe(&*bound.sql), args)
            .fetch(executor)
            .map(|res| res.map_err(crate::error::MapperRuntimeError::from));
        Box::pin(s)
    }

    /// 流式查询并逐行映射为 `T`（[`query`](Self::query) 的流式版本）
    pub fn query_stream<'q, E, T>(
        &self,
        bound: &'q BoundSql,
        executor: E,
    ) -> Pin<Box<dyn Stream<Item = Result<T>> + Send + 'q>>
    where
        E: Executor<'q, Database = sqlx::Any> + Send + 'q,
        T: DeserializeOwned + Send + 'q,
    {
        let row_stream = self.query_rows_stream(bound, executor);
        Box::pin(row_stream.map(|r| r.and_then(|row| ResultSetHandler::map_row::<T>(&row))))
    }

    /// 执行查询，将每行映射为 `T`
    pub async fn query<'q, E, T>(&self, bound: &'q BoundSql, executor: E) -> Result<Vec<T>>
    where
        E: Executor<'q, Database = sqlx::Any>,
        T: DeserializeOwned + Send,
    {
        let rows = self.query_rows(bound, executor).await?;
        // map_rows 是无状态的关联函数（列类型按 AnyTypeInfoKind 静态分派）
        ResultSetHandler::map_rows(rows)
    }

    /// 执行单行查询，返回映射后的 `Option<T>`（0 或 1 行；多于 1 行报错）
    pub async fn query_one<'q, E, T>(
        &self,
        bound: &'q BoundSql,
        executor: E,
    ) -> Result<Option<T>>
    where
        E: Executor<'q, Database = sqlx::Any>,
        T: DeserializeOwned + Send,
    {
        let rows = self.query_rows(bound, executor).await?;
        match rows.len() {
            0 => Ok(None),
            1 => Ok(Some(ResultSetHandler::map_row(&rows[0])?)),
            n => Err(crate::error::MapperRuntimeError::TooManyRows { actual: n }),
        }
    }

    /// 执行更新（INSERT/UPDATE/DELETE），返回查询结果（含受影响行数与生成主键）
    pub async fn execute<'q, E>(&self, bound: &'q BoundSql, executor: E) -> Result<AnyQueryResult>
    where
        E: Executor<'q, Database = sqlx::Any>,
    {
        let args = ParameterHandler::bind_arguments(bound)?;
        let kind = classify_sql(&bound.sql);
        let bus = &self.event_bus;
        bus.dispatch_if(|| BeforeSqlEvent {
            raw_sql: bound.sql.clone(),
            params: bound.parameters.clone(),
            kind,
        });
        let start = Instant::now();
        let result = sqlx::query_with(sqlx::AssertSqlSafe(&*bound.sql), args)
            .execute(executor)
            .await
            .map_err(crate::error::MapperRuntimeError::from);
        let elapsed = start.elapsed();
        crate::sql_log::log_execution(&self.sql_log, bound, elapsed);
        bus.dispatch_if(|| AfterSqlEvent {
            raw_sql: bound.sql.clone(),
            params: bound.parameters.clone(),
            kind,
            elapsed,
            outcome: match &result {
                Ok(r) => SqlOutcome::Affected(r.rows_affected()),
                Err(e) => SqlOutcome::Failed(e.to_string()),
            },
        });
        result
    }
}

/// 独立辅助函数：执行绑定并返回受影响行数（无需 SimpleExecutor 实例）。
///
/// `sql_log` 控制 SQL 执行日志；`event_bus` 用于派发执行前/后生命周期事件。
/// 传入 [`SqlLogConfig::default`]（关闭）则不记录，传入空 [`EventBus`] 则不派发。
pub async fn execute_rows_affected<'q, E>(
    bound: &'q BoundSql,
    executor: E,
    sql_log: &SqlLogConfig,
    event_bus: &EventBus,
) -> Result<u64>
where
    E: Executor<'q, Database = sqlx::Any>,
{
    let args = ParameterHandler::bind_arguments(bound)?;
    let kind = classify_sql(&bound.sql);
    event_bus.dispatch_if(|| BeforeSqlEvent {
        raw_sql: bound.sql.clone(),
        params: bound.parameters.clone(),
        kind,
    });
    let start = Instant::now();
    let result = sqlx::query_with(sqlx::AssertSqlSafe(&*bound.sql), args)
        .execute(executor)
        .await;
    let elapsed = start.elapsed();
    crate::sql_log::log_execution(sql_log, bound, elapsed);
    let outcome = match &result {
        Ok(r) => SqlOutcome::Affected(r.rows_affected()),
        Err(e) => SqlOutcome::Failed(e.to_string()),
    };
    event_bus.dispatch_if(|| AfterSqlEvent {
        raw_sql: bound.sql.clone(),
        params: bound.parameters.clone(),
        kind,
        elapsed,
        outcome,
    });
    let result = result?;
    Ok(result.rows_affected())
}
