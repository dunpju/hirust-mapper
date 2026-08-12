//! SqlSession：请求级会话
//!
//! 每个请求（或逻辑工作单元）持有一个 Session，共享工厂的连接池与注册表。
//! 提供完整的 CRUD 接口与事务管理（begin/commit/rollback）。
//!
//! 执行流程：查找 Mapper → 生成 [`BoundSql`] → [`SimpleExecutor`] 执行 → [`ResultSetHandler`] 映射。
//!
//! # 关于 `&mut self`
//!
//! 数据库执行方法以 `&mut self` 接收，因为事务模式下需对内部事务连接独占访问。
//! Session 是请求级对象（单线程顺序使用），`&mut self` 是惯用且正确的设计。

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use hirust_mapper_core::{BoundSql, Mapper, ResultMap};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

use crate::environment::Environment;
use crate::error::{MapperRuntimeError, Result};
use crate::executor::SimpleExecutor;
use crate::registry::{MapperRegistry, TypeAliasRegistry};
use crate::type_handler::TypeHandlerRegistry;

/// SqlSession（请求级）
pub struct SqlSession {
    environment: Environment,
    mapper_registry: Arc<RwLock<MapperRegistry>>,
    type_alias_registry: Arc<TypeAliasRegistry>,
    type_handler_registry: Arc<TypeHandlerRegistry>,
    executor: SimpleExecutor,
    transaction: Option<sqlx::Transaction<'static, sqlx::Any>>,
    closed: bool,
}

impl std::fmt::Debug for SqlSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqlSession")
            .field("driver", &self.environment.driver())
            .field("in_transaction", &self.transaction.is_some())
            .field("closed", &self.closed)
            .finish()
    }
}

impl SqlSession {
    /// 由 SqlSessionFactory 调用的构造函数
    pub(crate) fn new(
        environment: Environment,
        mapper_registry: Arc<RwLock<MapperRegistry>>,
        type_alias_registry: Arc<TypeAliasRegistry>,
        type_handler_registry: Arc<TypeHandlerRegistry>,
    ) -> Self {
        let executor = SimpleExecutor::new(Arc::clone(&type_handler_registry));
        Self {
            environment,
            mapper_registry,
            type_alias_registry,
            type_handler_registry,
            executor,
            transaction: None,
            closed: false,
        }
    }

    // ─── 访问器 ────────────────────────────────────────────────────

    /// 数据库环境引用
    pub fn environment(&self) -> &Environment {
        &self.environment
    }

    /// 连接池引用
    pub fn pool(&self) -> &sqlx::AnyPool {
        self.environment.pool()
    }

    /// Mapper 注册表的只读锁
    pub fn mapper_registry(&self) -> std::sync::RwLockReadGuard<'_, MapperRegistry> {
        self.mapper_registry.read().expect("MapperRegistry 锁中毒")
    }

    /// 类型别名注册表引用
    pub fn type_alias_registry(&self) -> &TypeAliasRegistry {
        &self.type_alias_registry
    }

    /// 类型处理器注册表引用
    pub fn type_handler_registry(&self) -> &TypeHandlerRegistry {
        &self.type_handler_registry
    }

    /// 执行器引用
    pub fn executor(&self) -> &SimpleExecutor {
        &self.executor
    }

    /// 是否处于事务中
    pub fn in_transaction(&self) -> bool {
        self.transaction.is_some()
    }

    // ─── Mapper 查找与 SQL 生成 ────────────────────────────────────

    /// 按 namespace 查找 Mapper（返回廉价的 `Arc<Mapper>`，不深克隆）
    pub fn get_mapper(&self, namespace: &str) -> Result<Arc<Mapper>> {
        self.mapper_registry()
            .get_mapper(namespace)
            .ok_or_else(|| MapperRuntimeError::MapperNotFound(namespace.to_string()))
    }

    /// 两阶段绑定：生成 BoundSql（`#{}` → `?`，`${}` → 内联）
    pub fn build_bound_sql(
        &self,
        namespace: &str,
        statement_id: &str,
        params: &HashMap<String, Value>,
    ) -> Result<BoundSql> {
        let mapper = self.get_mapper(namespace)?;
        mapper
            .build_bound_sql(statement_id, params)
            .map_err(MapperRuntimeError::from)
    }

    /// 将任意 `Serialize` 参数转为 `HashMap<String, Value>`
    fn params_to_map<T: Serialize>(params: &T) -> Result<HashMap<String, Value>> {
        let value = serde_json::to_value(params)
            .map_err(|e| MapperRuntimeError::TypeConversion(format!("参数序列化失败: {}", e)))?;
        match value {
            Value::Object(map) => Ok(map.into_iter().collect()),
            // 非对象值包装在 "_param" 键下，供 #{_param} 引用
            other => {
                let mut m = HashMap::new();
                m.insert("_param".to_string(), other);
                Ok(m)
            }
        }
    }

    /// 查询语句关联的 ResultMap（若声明了 resultMap 且存在）
    fn get_result_map(&self, namespace: &str, statement_id: &str) -> Result<Option<ResultMap>> {
        let mapper = self.get_mapper(namespace)?;
        match mapper.statements.get(statement_id) {
            Some(stmt) => {
                if let Some(rm_id) = &stmt.result_map {
                    Ok(mapper.result_maps.get(rm_id).cloned())
                } else {
                    Ok(None)
                }
            }
            None => Err(hirust_mapper_core::MapperError::StatementNotFound {
                id: statement_id.to_string(),
            }
            .into()),
        }
    }

    /// 内部：按事务状态选择执行目标并取回原始行
    async fn fetch_rows(&mut self, bound: &BoundSql) -> Result<Vec<sqlx::any::AnyRow>> {
        let executor = &self.executor;
        match self.transaction.as_mut() {
            Some(tx) => executor.query_rows(bound, &mut **tx).await,
            None => executor.query_rows(bound, self.environment.pool()).await,
        }
    }

    // ─── 查询接口 ──────────────────────────────────────────────────

    /// 查询单行（期望 0 或 1 行；多于 1 行报 `TooManyRows` 错误）
    pub async fn select_one<T: DeserializeOwned + Send>(
        &mut self,
        namespace: &str,
        statement_id: &str,
        params: &HashMap<String, Value>,
    ) -> Result<Option<T>> {
        let result_map = self.get_result_map(namespace, statement_id)?;
        let bound = self.build_bound_sql(namespace, statement_id, params)?;
        let rows = self.fetch_rows(&bound).await?;
        match result_map {
            Some(rm) => crate::handler::result_set::ResultSetHandler::map_row_with_result_map::<T>(rows, &rm),
            None => match rows.len() {
                0 => Ok(None),
                1 => Ok(Some(crate::handler::result_set::ResultSetHandler::map_row(&rows[0])?)),
                n => Err(MapperRuntimeError::TooManyRows { actual: n }),
            },
        }
    }

    /// 查询多行
    pub async fn select_list<T: DeserializeOwned + Send>(
        &mut self,
        namespace: &str,
        statement_id: &str,
        params: &HashMap<String, Value>,
    ) -> Result<Vec<T>> {
        let result_map = self.get_result_map(namespace, statement_id)?;
        let bound = self.build_bound_sql(namespace, statement_id, params)?;
        let rows = self.fetch_rows(&bound).await?;
        match result_map {
            Some(rm) => crate::handler::result_set::ResultSetHandler::map_rows_with_result_map::<T>(rows, &rm),
            None => crate::handler::result_set::ResultSetHandler::map_rows::<T>(rows),
        }
    }

    // ─── 写入接口 ──────────────────────────────────────────────────

    /// 插入（返回生成的主键，若驱动支持）
    ///
    /// 注意：sqlx 的 `Any` 驱动不透传 `last_insert_id`（sqlite 后端硬编码为 None），
    /// 因此对 sqlite 额外在**同一连接**上执行 `SELECT last_insert_rowid()` 取回主键。
    /// 这要求 INSERT 与 SELECT 复用同一连接，故 insert 不走 SimpleExecutor 的无状态路径，
    /// 而是显式持有连接（事务连接或从池获取的连接）。
    pub async fn insert<T: Serialize>(
        &mut self,
        namespace: &str,
        statement_id: &str,
        params: &T,
    ) -> Result<Option<i64>> {
        let params = Self::params_to_map(params)?;
        let bound = self.build_bound_sql(namespace, statement_id, &params)?;
        let args = crate::handler::parameter::ParameterHandler::bind_arguments(&bound)?;
        let driver = self.environment.driver();

        if let Some(tx) = self.transaction.as_mut() {
            let conn: &mut sqlx::AnyConnection = tx; // deref coercion: &mut Transaction → &mut AnyConnection
            sqlx::query_with(sqlx::AssertSqlSafe(&*bound.sql), args)
                .execute(&mut *conn)
                .await
                .map_err(MapperRuntimeError::Database)?;
            Ok(Self::fetch_last_insert_id(conn, driver).await?)
        } else {
            let mut conn = self
                .environment
                .pool()
                .acquire()
                .await
                .map_err(MapperRuntimeError::Database)?;
            sqlx::query_with(sqlx::AssertSqlSafe(&*bound.sql), args)
                .execute(&mut *conn)
                .await
                .map_err(MapperRuntimeError::Database)?;
            Ok(Self::fetch_last_insert_id(&mut conn, driver).await?)
        }
    }

    /// 按驱动取回最近一次 INSERT 生成的主键
    async fn fetch_last_insert_id(
        conn: &mut sqlx::AnyConnection,
        driver: &str,
    ) -> Result<Option<i64>> {
        match driver {
            "sqlite" => {
                let id: i64 = sqlx::query_scalar("SELECT last_insert_rowid()")
                    .fetch_one(conn)
                    .await
                    .map_err(MapperRuntimeError::Database)?;
                Ok(Some(id))
            }
            _ => {
                // mysql/postgres 等的生成主键获取留待 P8（selectKey / RETURNING）
                Ok(None)
            }
        }
    }

    /// 更新（返回受影响行数）
    pub async fn update<T: Serialize>(
        &mut self,
        namespace: &str,
        statement_id: &str,
        params: &T,
    ) -> Result<u64> {
        let params = Self::params_to_map(params)?;
        let bound = self.build_bound_sql(namespace, statement_id, &params)?;
        let executor = &self.executor;
        let result = match self.transaction.as_mut() {
            Some(tx) => executor.execute(&bound, &mut **tx).await,
            None => executor.execute(&bound, self.environment.pool()).await,
        }?;
        Ok(result.rows_affected())
    }

    /// 删除（返回受影响行数）
    pub async fn delete<T: Serialize>(
        &mut self,
        namespace: &str,
        statement_id: &str,
        params: &T,
    ) -> Result<u64> {
        let params = Self::params_to_map(params)?;
        let bound = self.build_bound_sql(namespace, statement_id, &params)?;
        let executor = &self.executor;
        let result = match self.transaction.as_mut() {
            Some(tx) => executor.execute(&bound, &mut **tx).await,
            None => executor.execute(&bound, self.environment.pool()).await,
        }?;
        Ok(result.rows_affected())
    }

    // ─── 事务管理 ──────────────────────────────────────────────────

    /// 开启事务
    pub async fn begin(&mut self) -> Result<()> {
        if self.transaction.is_some() {
            return Err(MapperRuntimeError::Transaction(
                "事务已开启，请先 commit 或 rollback".to_string(),
            ));
        }
        let tx = self
            .environment
            .pool()
            .begin()
            .await
            .map_err(MapperRuntimeError::Database)?;
        self.transaction = Some(tx);
        Ok(())
    }

    /// 提交事务并消费 session
    pub async fn commit(mut self) -> Result<()> {
        if let Some(tx) = self.transaction.take() {
            tx.commit().await.map_err(|e| {
                MapperRuntimeError::Transaction(format!("提交失败: {}", e))
            })?;
        }
        self.closed = true;
        Ok(())
    }

    /// 回滚事务并消费 session
    pub async fn rollback(mut self) -> Result<()> {
        if let Some(tx) = self.transaction.take() {
            tx.rollback().await.map_err(|e| {
                MapperRuntimeError::Transaction(format!("回滚失败: {}", e))
            })?;
        }
        self.closed = true;
        Ok(())
    }

    // ─── 生命周期 ──────────────────────────────────────────────────

    /// 关闭 session（未提交的事务将回滚）
    pub async fn close(&mut self) -> Result<()> {
        if let Some(tx) = self.transaction.take() {
            let _ = tx.rollback().await; // 关闭时回滚未提交事务
        }
        self.closed = true;
        Ok(())
    }

    /// session 是否已关闭
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// 获取命名空间代理（链式调用风格，省去每次传入 namespace）
    pub fn mapper(&mut self, namespace: &str) -> Result<MapperProxy<'_>> {
        let _ = self.get_mapper(namespace)?; // 验证 namespace 存在
        Ok(MapperProxy {
            session: self,
            namespace: namespace.to_string(),
        })
    }
}

// ─── MapperProxy：命名空间代理 ─────────────────────────────────────

/// 命名空间代理：绑定到特定 namespace
pub struct MapperProxy<'s> {
    session: &'s mut SqlSession,
    namespace: String,
}

impl<'s> MapperProxy<'s> {
    /// 查询单行
    pub async fn select_one<T: DeserializeOwned + Send>(
        &mut self,
        statement_id: &str,
        params: &HashMap<String, Value>,
    ) -> Result<Option<T>> {
        self.session.select_one(&self.namespace, statement_id, params).await
    }

    /// 查询多行
    pub async fn select_list<T: DeserializeOwned + Send>(
        &mut self,
        statement_id: &str,
        params: &HashMap<String, Value>,
    ) -> Result<Vec<T>> {
        self.session.select_list(&self.namespace, statement_id, params).await
    }

    /// 插入
    pub async fn insert<T: Serialize>(&mut self, statement_id: &str, params: &T) -> Result<Option<i64>> {
        self.session.insert(&self.namespace, statement_id, params).await
    }

    /// 更新
    pub async fn update<T: Serialize>(&mut self, statement_id: &str, params: &T) -> Result<u64> {
        self.session.update(&self.namespace, statement_id, params).await
    }

    /// 删除
    pub async fn delete<T: Serialize>(&mut self, statement_id: &str, params: &T) -> Result<u64> {
        self.session.delete(&self.namespace, statement_id, params).await
    }

    /// namespace
    pub fn namespace(&self) -> &str {
        &self.namespace
    }
}
