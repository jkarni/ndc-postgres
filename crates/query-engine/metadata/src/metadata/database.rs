//! Metadata information regarding the database and tracked information.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Map of all known composite types.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CompositeTypes(pub BTreeMap<String, CompositeType>);

/// A Scalar Type.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScalarType(pub String);

/// The type of values that a column, field, or argument may take.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Type {
    ScalarType(ScalarType),
    CompositeType(String),
    ArrayType(Box<Type>),
}

/// Information about a composite type. These are very similar to tables, but with the crucial
/// difference that composite types do not support constraints (such as NOT NULL).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CompositeType {
    pub name: String,
    pub fields: BTreeMap<String, FieldInfo>,
    #[serde(default)]
    pub description: Option<String>,
}

/// Information about a composite type field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FieldInfo {
    pub name: String,
    pub r#type: Type,
    #[serde(default)]
    pub description: Option<String>,
}

/// The complete list of supported binary operators for scalar types.
/// Not all of these are supported for every type.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ComparisonOperators(pub BTreeMap<ScalarType, BTreeMap<String, ComparisonOperator>>);

/// Represents a postgres binary comparison operator
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ComparisonOperator {
    pub operator_name: String,
    pub operator_kind: OperatorKind,
    pub argument_type: ScalarType,

    #[serde(default = "default_true")]
    pub is_infix: bool,
}

/// Is it a built-in operator, or a custom operator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum OperatorKind {
    Equal,
    In,
    Custom,
}

/// This is quite unfortunate: https://github.com/serde-rs/serde/issues/368
/// TL;DR: we can't set default literals for serde, so if we want 'is_infix' to
/// default to 'true', we have to set its default as a function that returns 'true'.
fn default_true() -> bool {
    true
}

/// Mapping from a "table" name to its information.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TablesInfo(pub BTreeMap<String, TableInfo>);

/// Information about a database table (or any other kind of relation).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TableInfo {
    pub schema_name: String,
    pub table_name: String,
    pub columns: BTreeMap<String, ColumnInfo>,
    #[serde(default)]
    pub uniqueness_constraints: UniquenessConstraints,
    #[serde(default)]
    pub foreign_relations: ForeignRelations,
    #[serde(default)]
    pub description: Option<String>,
}

/// Can this column contain null values
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Nullable {
    #[default]
    Nullable,
    NonNullable,
}

/// Does this column have a default value.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum HasDefault {
    #[default]
    NoDefault,
    HasDefault,
}

/// Is this column an identity column.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum IsIdentity {
    #[default]
    NotIdentity,
    IdentityByDefault,
    IdentityAlways,
}

/// Is this column a generated column.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum IsGenerated {
    #[default]
    NotGenerated,
    Stored,
}

/// Information about a database column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ColumnInfo {
    pub name: String,
    pub r#type: Type,
    #[serde(default)]
    pub nullable: Nullable,
    #[serde(skip_serializing_if = "does_not_have_default")]
    #[serde(default)]
    pub has_default: HasDefault,
    #[serde(skip_serializing_if = "is_not_identity")]
    #[serde(default)]
    pub is_identity: IsIdentity,
    #[serde(skip_serializing_if = "is_not_generated")]
    #[serde(default)]
    pub is_generated: IsGenerated,
    #[serde(default)]
    pub description: Option<String>,
}

fn does_not_have_default(has_default: &HasDefault) -> bool {
    matches!(has_default, HasDefault::NoDefault)
}

fn is_not_identity(is_identity: &IsIdentity) -> bool {
    matches!(is_identity, IsIdentity::NotIdentity)
}

fn is_not_generated(is_generated: &IsGenerated) -> bool {
    matches!(is_generated, IsGenerated::NotGenerated)
}

/// A mapping from the name of a unique constraint to its value.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UniquenessConstraints(pub BTreeMap<String, UniquenessConstraint>);

/// The set of columns that make up a uniqueness constraint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UniquenessConstraint(pub BTreeSet<String>);

/// A mapping from the name of a foreign key constraint to its value.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ForeignRelations(pub BTreeMap<String, ForeignRelation>);

/// A foreign key constraint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ForeignRelation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub foreign_schema: Option<String>,
    pub foreign_table: String,
    pub column_mapping: BTreeMap<String, String>,
}

/// All supported aggregate functions, grouped by type.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AggregateFunctions(pub BTreeMap<ScalarType, BTreeMap<String, AggregateFunction>>);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AggregateFunction {
    pub return_type: ScalarType,
}

/// Type representation of scalar types, grouped by type.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TypeRepresentations(pub BTreeMap<ScalarType, TypeRepresentation>);

/// Type representation of a scalar type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum TypeRepresentation {
    /// JSON booleans
    Boolean,
    /// Any JSON string
    String,
    /// Any JSON number
    Number,
    /// Any JSON number, with no decimal part
    Integer,
    /// One of the specified string values
    Enum(Vec<String>),
}

// tests

#[cfg(test)]
mod tests {
    use super::{ScalarType, TypeRepresentation, TypeRepresentations};

    #[test]
    fn parse_type_representations() {
        assert_eq!(
            serde_json::from_str::<TypeRepresentations>(
                r#"{"card_suit": {"enum": ["hearts", "clubs", "diamonds", "spades"]}}"#
            )
            .unwrap(),
            TypeRepresentations(
                [(
                    ScalarType("card_suit".to_string()),
                    TypeRepresentation::Enum(vec![
                        "hearts".into(),
                        "clubs".into(),
                        "diamonds".into(),
                        "spades".into()
                    ])
                )]
                .into()
            )
        );
    }
}
