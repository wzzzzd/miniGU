use std::sync::Arc;

use gql_parser::ast::{
    ElementPattern, GraphPattern, GraphPatternBindingTable, MatchMode, PathMode, PathPattern,
    PathPatternExpr, PathPatternPrefix,
};
use itertools::Itertools;
use minigu_common::data_type::{DataField, LogicalType};
use minigu_common::error::not_implemented;
use minigu_ir::bound::{
    BoundElementPattern, BoundGraphPattern, BoundMatchMode, BoundPathMode, BoundPathPattern,
    BoundPathPatternExpr,
};

use crate::binder::Binder;
use crate::error::{BindError, BindResult};

impl Binder<'_> {
    pub fn bind_graph_pattern_binding_table(
        &mut self,
        table: &GraphPatternBindingTable,
    ) -> BindResult<BoundGraphPattern> {
        if !table.yield_clause.is_empty() {
            return not_implemented("graph pattern yield clause", None);
        }
        self.bind_graph_pattern(table.pattern.value())
    }

    pub fn bind_graph_pattern(&mut self, pattern: &GraphPattern) -> BindResult<BoundGraphPattern> {
        if pattern.keep.is_some() {
            return not_implemented("keep clause in graph pattern", None);
        }
        let match_mode = pattern
            .match_mode
            .as_ref()
            .map(|m| bind_match_mode(m.value()));
        let paths = pattern
            .patterns
            .iter()
            .map(|p| self.bind_path_pattern(p.value()))
            .try_collect()?;
        let predicate = pattern
            .where_clause
            .as_ref()
            .map(|e| self.bind_value_expression(e.value()))
            .transpose()?;
        let schema = Arc::new(
            self.active_data_schema
                .as_ref()
                .expect("there should be an active data schema when binding graph pattern")
                .clone(),
        );
        Ok(BoundGraphPattern {
            match_mode,
            paths,
            predicate,
            schema,
        })
    }

    pub fn bind_path_pattern(&mut self, pattern: &PathPattern) -> BindResult<BoundPathPattern> {
        if let Some(variable) = &pattern.variable {
            let schema = self.active_data_schema.get_or_insert_default();
            if schema.get_field_by_name(variable.value()).is_some() {
                return Err(BindError::VariableAlreadyDeclared(variable.value().clone()));
            }
            let field = DataField::new(variable.value().to_string(), LogicalType::Path, true);
            schema.push(field);
        }
        let mode = pattern
            .prefix
            .as_ref()
            .map(|p| bind_path_pattern_prefix(p.value()))
            .transpose()?;
        let expr = self.bind_path_pattern_expr(pattern.expr.value())?;
        self.validate_bound_path_pattern_expr(&expr)?;
        Ok(BoundPathPattern { mode, expr })
    }

    pub fn bind_path_pattern_expr(
        &mut self,
        expr: &PathPatternExpr,
    ) -> BindResult<BoundPathPatternExpr> {
        match expr {
            PathPatternExpr::Union(_) => not_implemented("union path pattern", None),
            PathPatternExpr::Alternation(_) => {
                not_implemented("multiset alternation path pattern", None)
            }
            PathPatternExpr::Concat(paths) => paths
                .iter()
                .map(|p| self.bind_path_pattern_expr(p.value()))
                .try_collect()
                .map(BoundPathPatternExpr::Concat),
            PathPatternExpr::Quantified { .. } => not_implemented("quantified path pattern", None),
            PathPatternExpr::Optional(_) => not_implemented("optional path pattern", None),
            PathPatternExpr::Grouped(_) => not_implemented("sub-path pattern", None),
            PathPatternExpr::Pattern(element) => self
                .bind_element_pattern(element)
                .map(BoundPathPatternExpr::Pattern),
        }
    }

    pub fn validate_bound_path_pattern_expr(&self, expr: &BoundPathPatternExpr) -> BindResult<()> {
        Ok(())
    }

    pub fn bind_element_pattern(
        &mut self,
        pattern: &ElementPattern,
    ) -> BindResult<BoundElementPattern> {
        match pattern {
            ElementPattern::Node(filler) => {
                todo!()
            }
            ElementPattern::Edge { kind, filler } => {
                todo!()
            }
        }
    }
}

pub fn bind_path_pattern_prefix(prefix: &PathPatternPrefix) -> BindResult<BoundPathMode> {
    match prefix {
        PathPatternPrefix::PathMode(mode) => Ok(bind_path_mode(mode)),
        PathPatternPrefix::PathSearch(_) => not_implemented("path search prefix", None),
    }
}

pub fn bind_path_mode(mode: &PathMode) -> BoundPathMode {
    match mode {
        PathMode::Walk => BoundPathMode::Walk,
        PathMode::Trail => BoundPathMode::Trail,
        PathMode::Simple => BoundPathMode::Simple,
        PathMode::Acyclic => BoundPathMode::Acyclic,
    }
}

pub fn bind_match_mode(mode: &MatchMode) -> BoundMatchMode {
    match mode {
        MatchMode::Repeatable => BoundMatchMode::Repeatable,
        MatchMode::Different => BoundMatchMode::Different,
    }
}
