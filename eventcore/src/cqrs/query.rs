//! Query abstractions for CQRS read models.
//!
//! This module provides a type-safe query builder and filtering system
//! for querying read models in a CQRS system.

use serde::{Deserialize, Serialize};
use std::marker::PhantomData;

/// Trait for queries that can be executed against read models.
pub trait Query: Send + Sync {
    /// The type of model this query operates on
    type Model;

    /// Check if a model matches this query's criteria.
    fn matches(&self, model: &Self::Model) -> bool;

    /// Apply ordering and limits to a collection of models.
    fn apply_ordering_and_limits(&self, models: Vec<Self::Model>) -> Vec<Self::Model>;
}

/// A type-safe query builder for read models.
#[derive(Debug, Clone)]
pub struct QueryBuilder<M> {
    filters: Vec<Filter>,
    ordering: Option<Ordering>,
    limit: Option<usize>,
    offset: Option<usize>,
    _phantom: PhantomData<M>,
}

impl<M> QueryBuilder<M> {
    /// Creates a new query builder.
    pub const fn new() -> Self {
        Self {
            filters: Vec::new(),
            ordering: None,
            limit: None,
            offset: None,
            _phantom: PhantomData,
        }
    }

    /// Add a filter to the query.
    #[must_use]
    pub fn filter(mut self, filter: Filter) -> Self {
        self.filters.push(filter);
        self
    }

    /// Add a field filter using a convenient builder pattern.
    #[must_use]
    pub fn where_field(self, field: impl Into<String>) -> FieldFilterBuilder<M> {
        FieldFilterBuilder {
            query: self,
            field: field.into(),
        }
    }

    /// Set the ordering for results.
    #[must_use]
    pub fn order_by(mut self, field: impl Into<String>, direction: Direction) -> Self {
        self.ordering = Some(Ordering {
            field: field.into(),
            direction,
        });
        self
    }

    /// Limit the number of results.
    #[must_use]
    pub const fn limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Skip a number of results.
    #[must_use]
    pub const fn offset(mut self, offset: usize) -> Self {
        self.offset = Some(offset);
        self
    }

    /// Get the filters for this query.
    pub fn filters(&self) -> &[Filter] {
        &self.filters
    }

    /// Get the ordering for this query.
    pub const fn ordering(&self) -> Option<&Ordering> {
        self.ordering.as_ref()
    }

    /// Get the limit for this query.
    pub const fn get_limit(&self) -> Option<usize> {
        self.limit
    }

    /// Get the offset for this query.
    pub const fn get_offset(&self) -> Option<usize> {
        self.offset
    }
}

impl<M> Default for QueryBuilder<M> {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for field-specific filters.
pub struct FieldFilterBuilder<M> {
    query: QueryBuilder<M>,
    field: String,
}

impl<M> FieldFilterBuilder<M> {
    /// Filter for equality.
    #[must_use]
    pub fn eq(self, value: impl Into<Value>) -> QueryBuilder<M> {
        self.query.filter(Filter {
            field: self.field,
            op: FilterOp::Eq,
            value: value.into(),
        })
    }

    /// Filter for inequality.
    #[must_use]
    pub fn ne(self, value: impl Into<Value>) -> QueryBuilder<M> {
        self.query.filter(Filter {
            field: self.field,
            op: FilterOp::Ne,
            value: value.into(),
        })
    }

    /// Filter for greater than.
    #[must_use]
    pub fn gt(self, value: impl Into<Value>) -> QueryBuilder<M> {
        self.query.filter(Filter {
            field: self.field,
            op: FilterOp::Gt,
            value: value.into(),
        })
    }

    /// Filter for greater than or equal.
    #[must_use]
    pub fn gte(self, value: impl Into<Value>) -> QueryBuilder<M> {
        self.query.filter(Filter {
            field: self.field,
            op: FilterOp::Gte,
            value: value.into(),
        })
    }

    /// Filter for less than.
    #[must_use]
    pub fn lt(self, value: impl Into<Value>) -> QueryBuilder<M> {
        self.query.filter(Filter {
            field: self.field,
            op: FilterOp::Lt,
            value: value.into(),
        })
    }

    /// Filter for less than or equal.
    #[must_use]
    pub fn lte(self, value: impl Into<Value>) -> QueryBuilder<M> {
        self.query.filter(Filter {
            field: self.field,
            op: FilterOp::Lte,
            value: value.into(),
        })
    }

    /// Filter for values in a list.
    #[must_use]
    pub fn in_list(self, values: Vec<Value>) -> QueryBuilder<M> {
        self.query.filter(Filter {
            field: self.field,
            op: FilterOp::In,
            value: Value::List(values),
        })
    }

    /// Filter for values containing a substring.
    #[must_use]
    pub fn contains(self, value: impl Into<String>) -> QueryBuilder<M> {
        self.query.filter(Filter {
            field: self.field,
            op: FilterOp::Contains,
            value: Value::String(value.into()),
        })
    }

    /// Filter for values starting with a prefix.
    #[must_use]
    pub fn starts_with(self, value: impl Into<String>) -> QueryBuilder<M> {
        self.query.filter(Filter {
            field: self.field,
            op: FilterOp::StartsWith,
            value: Value::String(value.into()),
        })
    }
}

/// A filter condition for queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Filter {
    /// The field to filter on
    pub field: String,
    /// The filter operation
    pub op: FilterOp,
    /// The value to compare against
    pub value: Value,
}

/// Filter operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilterOp {
    /// Equals
    Eq,
    /// Not equals
    Ne,
    /// Greater than
    Gt,
    /// Greater than or equal
    Gte,
    /// Less than
    Lt,
    /// Less than or equal
    Lte,
    /// In a list of values
    In,
    /// Contains substring
    Contains,
    /// Starts with prefix
    StartsWith,
}

/// Query values that can be used in filters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    /// Null value
    Null,
    /// Boolean value
    Bool(bool),
    /// Integer value
    Int(i64),
    /// Floating point value
    Float(f64),
    /// String value
    String(String),
    /// List of values
    List(Vec<Value>),
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}

impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Self::Int(i64::from(v))
    }
}

impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Self::Int(v)
    }
}

impl From<u32> for Value {
    fn from(v: u32) -> Self {
        Self::Int(i64::from(v))
    }
}

impl From<u64> for Value {
    #[allow(clippy::cast_possible_wrap)]
    fn from(v: u64) -> Self {
        Self::Int(v as i64)
    }
}

impl From<f32> for Value {
    fn from(v: f32) -> Self {
        Self::Float(f64::from(v))
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Self::Float(v)
    }
}

impl From<String> for Value {
    fn from(v: String) -> Self {
        Self::String(v)
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Self::String(v.to_string())
    }
}

impl<T: Into<Self>> From<Option<T>> for Value {
    fn from(v: Option<T>) -> Self {
        v.map_or(Self::Null, Into::into)
    }
}

/// Ordering specification for query results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ordering {
    /// The field to order by
    pub field: String,
    /// The sort direction
    pub direction: Direction,
}

/// Sort direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    /// Ascending order
    Asc,
    /// Descending order
    Desc,
}

/// Consistency level for read operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsistencyLevel {
    /// Read from latest snapshot (fastest, may be stale)
    Eventual,
    /// Wait for specific event to be processed
    CausallyConsistent {
        /// The event ID to wait for
        after_event: crate::types::EventId,
    },
    /// Force synchronous update before read
    Strong,
}

impl<M> Query for QueryBuilder<M>
where
    M: Send + Sync,
{
    type Model = M;

    fn matches(&self, _model: &Self::Model) -> bool {
        // Default implementation always matches
        // Actual filtering logic would need to be implemented
        // based on the model's structure
        true
    }

    fn apply_ordering_and_limits(&self, mut models: Vec<Self::Model>) -> Vec<Self::Model> {
        // Apply offset
        if let Some(offset) = self.offset {
            if offset < models.len() {
                models.drain(..offset);
            } else {
                models.clear();
            }
        }

        // Apply limit
        if let Some(limit) = self.limit {
            models.truncate(limit);
        }

        models
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct TestModel {
        id: String,
        name: String,
        value: i32,
    }

    #[test]
    fn query_builder_construction() {
        let query = QueryBuilder::<TestModel>::new()
            .where_field("name")
            .eq("test")
            .where_field("value")
            .gt(10)
            .order_by("value", Direction::Desc)
            .limit(10)
            .offset(5);

        assert_eq!(query.filters().len(), 2);
        assert_eq!(query.get_limit(), Some(10));
        assert_eq!(query.get_offset(), Some(5));
        assert!(query.ordering().is_some());
    }

    #[test]
    fn filter_operations() {
        let _query = QueryBuilder::<TestModel>::new()
            .where_field("name")
            .contains("test")
            .where_field("value")
            .in_list(vec![Value::Int(1), Value::Int(2), Value::Int(3)])
            .where_field("active")
            .eq(true);
    }

    #[test]
    fn value_conversions() {
        assert_eq!(Value::from(42i32), Value::Int(42));
        assert_eq!(Value::from(true), Value::Bool(true));
        assert_eq!(Value::from("test"), Value::String("test".to_string()));
        assert_eq!(Value::from(123.456f64), Value::Float(123.456));
        assert_eq!(Value::from(None::<i32>), Value::Null);
        assert_eq!(Value::from(Some(42)), Value::Int(42));
    }
}
