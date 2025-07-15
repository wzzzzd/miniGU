use std::sync::Arc;
use std::time::{Duration, Instant};

use arrow::array::create_array;
use gql_parser::ast::{
    Procedure, Program, ProgramActivity, SchemaRef as AstSchemaRef, SessionActivity, SessionReset,
    SessionResetArgs, SessionSet, TransactionActivity,
};
use gql_parser::parse_gql;
use itertools::Itertools;
use minigu_binder::binder::Binder;
use minigu_catalog::memory::MemoryCatalog;
use minigu_catalog::memory::schema::MemorySchemaCatalog;
use minigu_catalog::provider::SchemaRef;
use minigu_common::data_chunk::DataChunk;
use minigu_common::data_type::{DataField, DataSchema, LogicalType};
use minigu_common::error::not_implemented;
use minigu_context::database::DatabaseContext;
use minigu_context::session::SessionContext;
use minigu_execution::builder::ExecutorBuilder;
use minigu_execution::executor::Executor;
use minigu_ir::plan::PlanData;
use minigu_planner::logical_planner::LogicalPlanner;
use minigu_planner::optimizer::Optimizer;

use crate::error::{Error, Result};
use crate::metrics::QueryMetrics;
use crate::result::QueryResult;

pub struct Session {
    context: SessionContext,
    closed: bool,
}

impl Session {
    pub(crate) fn new(
        database: Arc<DatabaseContext>,
        default_schema: Arc<MemorySchemaCatalog>,
    ) -> Result<Self> {
        let mut context = SessionContext::new(database);
        context.home_schema = Some(default_schema.clone());
        context.current_schema = Some(default_schema);
        Ok(Self {
            context,
            closed: false,
        })
    }

    pub fn query(&mut self, query: &str) -> Result<QueryResult> {
        if self.closed {
            return Err(Error::SessionClosed);
        }
        let start = Instant::now();
        let program = parse_gql(query)?;
        let parsing_time = start.elapsed();
        let mut result = program
            .value()
            .activity
            .as_ref()
            .map(|activity| match activity.value() {
                ProgramActivity::Session(activity) => self.handle_session_activity(activity),
                ProgramActivity::Transaction(activity) => {
                    self.handle_transaction_activity(activity)
                }
            })
            .transpose()?
            .unwrap_or_default();
        result.metrics.parsing_time = parsing_time;
        if program.value().session_close {
            self.closed = true;
        }
        Ok(result)
    }

    fn handle_session_activity(&mut self, activity: &SessionActivity) -> Result<QueryResult> {
        let mut metrics = QueryMetrics::default();
        let start = Instant::now();
        for set in &activity.set {
            match set.value() {
                SessionSet::Schema(schema) => return not_implemented("session set schema", None),
                SessionSet::Graph(graph) => {
                    let binder = self.build_binder();
                    let graph = binder.bind_graph_expr(graph.value())?;
                    self.context.current_graph = Some(graph);
                }
                SessionSet::TimeZone(_) => return not_implemented("session set timezone", None),
                SessionSet::Parameter(_) => return not_implemented("session set parameter", None),
            }
        }
        for reset in &activity.reset {
            if let Some(reset) = &reset.value().0 {
                match reset.value() {
                    SessionResetArgs::AllCharacteristics => self.reset_session(),
                    SessionResetArgs::AllParameters => {
                        return not_implemented("session reset all parameters", None);
                    }
                    SessionResetArgs::Schema => {
                        self.context.current_schema = self.context.home_schema.clone();
                    }
                    SessionResetArgs::Graph => {
                        self.context.current_graph = self.context.home_graph.clone();
                    }
                    SessionResetArgs::TimeZone => {
                        return not_implemented("session reset timezone", None);
                    }
                    SessionResetArgs::Parameter(spanned) => {
                        return not_implemented("session reset parameter", None);
                    }
                }
            } else {
                self.reset_session();
            }
        }
        metrics.execution_time = start.elapsed();
        Ok(QueryResult::new(None, metrics, vec![]))
    }

    fn handle_transaction_activity(&self, activity: &TransactionActivity) -> Result<QueryResult> {
        if activity.start.is_some() {
            return not_implemented("start transaction", None);
        }
        if activity.end.is_some() {
            return not_implemented("end transaction", None);
        }
        let result = activity
            .procedure
            .as_ref()
            .map(|procedure| self.handle_procedure(procedure.value()))
            .transpose()?
            .unwrap_or_default();
        Ok(result)
    }

    fn handle_procedure(&self, procedure: &Procedure) -> Result<QueryResult> {
        let mut metrics = QueryMetrics::default();

        let start = Instant::now();
        let binder = self.build_binder();
        let bound = binder.bind(procedure)?;
        metrics.binding_time = start.elapsed();

        let start = Instant::now();
        let logical_plan = LogicalPlanner::new().create_logical_plan(bound)?;
        let physical_plan = Optimizer::new().create_physical_plan(&logical_plan)?;
        metrics.planning_time = start.elapsed();

        let data_schema = physical_plan.schema().cloned();
        let start = Instant::now();
        let chunks: Vec<_> = self.context.database().runtime().scope(|_| {
            let mut executor = ExecutorBuilder::new(self.context.clone()).build(&physical_plan);
            executor.into_iter().try_collect()
        })?;
        metrics.execution_time = start.elapsed();

        Ok(QueryResult::new(data_schema, metrics, chunks))
    }

    fn build_binder(&self) -> Binder {
        Binder::new(
            self.context.database().catalog(),
            self.context.current_schema.clone().map(|s| s as _),
            self.context.home_schema.clone().map(|s| s as _),
            self.context.current_graph.clone(),
            self.context.home_graph.clone(),
        )
    }

    fn reset_session(&mut self) {
        self.context.current_schema = self.context.home_schema.clone();
        self.context.current_graph = self.context.home_graph.clone();
    }
}
