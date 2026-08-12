//! SimpleExecutor：基础的 SQL 执行器
//!
//! 串联 [`ParameterHandler`]（绑定）→ sqlx 执行 → [`ResultSetHandler`]（映射）。
//! 所有方法以泛型 `E: sqlx::Executor` 接收执行目标，因此同一套逻辑可作用于
//! 连接池（`&AnyPool`）或事务连接（`&mut AnyConnection`）。

use std::sync::Arc;

use hirust_mapper_core::BoundSql;
use serde::de::DeserializeOwned;
use sqlx::any::{AnyQueryResult, AnyRow};
use sqlx::Executor;

use crate::error::Result;
use crate::handler::parameter::ParameterHandler;
use crate::handler::result_set::ResultSetHandler;
use crate::type_handler::TypeHandlerRegistry;

/// 基础执行器
///
/// 无状态（除类型处理器注册表），可被多个 Session 共享。
pub struct SimpleExecutor {
    type_handler_registry: Arc<TypeHandlerRegistry>,
}

impl SimpleExecutor {
    /// 使用指定类型处理器注册表创建
    pub fn new(type_handler_registry: Arc<TypeHandlerRegistry>) -> Self {
        Self { type_handler_registry }
    }

    /// 使用默认内置类型处理器创建
    pub fn with_defaults() -> Self {
        Self::new(Arc::new(TypeHandlerRegistry::with_defaults()))
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
        sqlx::query_with(sqlx::AssertSqlSafe(&*bound.sql), args)
            .fetch_all(executor)
            .await
            .map_err(Into::into)
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
        sqlx::query_with(sqlx::AssertSqlSafe(&*bound.sql), args)
            .execute(executor)
            .await
            .map_err(Into::into)
    }
}

/// 独立辅助函数：执行绑定并返回受影响行数（无需 SimpleExecutor 实例）
pub async fn execute_rows_affected<'q, E>(bound: &'q BoundSql, executor: E) -> Result<u64>
where
    E: Executor<'q, Database = sqlx::Any>,
{
    let args = ParameterHandler::bind_arguments(bound)?;
    let result = sqlx::query_with(sqlx::AssertSqlSafe(&*bound.sql), args)
        .execute(executor)
        .await?;
    Ok(result.rows_affected())
}
