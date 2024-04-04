//! Helpers for processing requests and building SQL.

use std::collections::BTreeMap;

use ndc_sdk::models;

use super::error::Error;
use query_engine_metadata::metadata;
use query_engine_sql::sql;

#[derive(Debug)]
/// Static information from the query and metadata.
pub struct Env<'request> {
    pub(crate) metadata: &'request metadata::Metadata,
    relationships: BTreeMap<String, models::Relationship>,
    pub(crate) mutations_version: Option<metadata::mutations::MutationsVersion>,
    variables_table: Option<sql::ast::TableReference>,
}

#[derive(Debug)]
/// Stateful information changed throughout the translation process.
pub struct State {
    native_queries: NativeQueries,
    global_table_index: TableAliasIndex,
}

#[derive(Debug)]
pub struct TableAliasIndex(pub u64);

#[derive(Debug)]
/// Store top-level native queries generated throughout the translation process.
///
/// Native queries are implemented as `WITH <native_query_name_<index>> AS (<native_query>) <query>`
struct NativeQueries {
    /// native queries that receive different arguments should result in different CTEs,
    /// and be used via a AliasedTable in the query.
    native_queries: Vec<NativeQueryInfo>,
}

#[derive(Debug)]
/// Information we store about a native query call.
pub struct NativeQueryInfo {
    pub info: metadata::NativeQueryInfo,
    pub arguments: BTreeMap<String, models::Argument>,
    pub alias: sql::ast::TableAlias,
}

/// For the root table in the query, and for the current table we are processing,
/// We'd like to track what is their reference in the query (the name we can use to address them,
/// an alias we generate), and what is their name in the metadata (so we can get
/// their information such as which columns are available for that table).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootAndCurrentTables {
    /// The root (top-most) table in the query.
    pub root_table: TableNameAndReference,
    /// The current table we are processing.
    pub current_table: TableNameAndReference,
}

/// For a table in the query, We'd like to track what is its reference in the query
/// (the name we can use to address them, an alias we generate), and what is their name in the
/// metadata (so we can get their information such as which columns are available for that table).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableNameAndReference {
    /// Table name for column lookup
    pub name: String,
    /// Table alias to query from
    pub reference: sql::ast::TableReference,
}

#[derive(Debug)]
/// Information about columns
pub struct ColumnInfo {
    pub name: sql::ast::ColumnName,
    pub r#type: metadata::Type,
}

#[derive(Debug)]
/// Metadata information about a specific collection.
pub enum CollectionInfo<'env> {
    Table {
        name: &'env str,
        info: &'env metadata::TableInfo,
    },
    NativeQuery {
        name: &'env str,
        info: &'env metadata::NativeQueryInfo,
    },
}

#[derive(Debug)]
/// Metadata information about a specific collection.
pub enum CompositeTypeInfo<'env> {
    CollectionInfo(CollectionInfo<'env>),
    CompositeTypeInfo {
        name: String,
        info: metadata::CompositeType,
    },
}

impl<'request> Env<'request> {
    /// Create a new Env by supplying the metadata and relationships.
    pub fn new(
        metadata: &'request metadata::Metadata,
        relationships: BTreeMap<String, models::Relationship>,
        mutations_version: Option<metadata::mutations::MutationsVersion>,
        variables_table: Option<sql::ast::TableReference>,
    ) -> Self {
        Env {
            metadata,
            relationships,
            mutations_version,
            variables_table,
        }
    }

    /// Lookup a collection's information in the metadata.

    pub fn lookup_composite_type(
        &self,
        type_name: &'request str,
    ) -> Result<CompositeTypeInfo<'request>, Error> {
        let it_is_a_collection = self.lookup_collection(type_name);

        match it_is_a_collection {
            Ok(collection_info) => Ok(CompositeTypeInfo::CollectionInfo(collection_info)),
            Err(Error::CollectionNotFound(_)) => {
                let its_a_type = self.metadata.composite_types.0.get(type_name).map(|t| {
                    CompositeTypeInfo::CompositeTypeInfo {
                        name: t.name.clone(),
                        info: t.clone(),
                    }
                });

                its_a_type.ok_or(Error::CollectionNotFound(type_name.to_string()))
            }
            Err(err) => Err(err),
        }
    }

    pub fn lookup_collection(
        &self,
        collection_name: &'request str,
    ) -> Result<CollectionInfo<'request>, Error> {
        let table = self
            .metadata
            .tables
            .0
            .get(collection_name)
            .map(|t| CollectionInfo::Table {
                name: collection_name,
                info: t,
            });

        match table {
            Some(table) => Ok(table),
            None => self
                .metadata
                .native_queries
                .0
                .get(collection_name)
                .map(|nq| CollectionInfo::NativeQuery {
                    name: collection_name,
                    info: nq,
                })
                .ok_or(Error::CollectionNotFound(collection_name.to_string())),
        }
    }

    /// Lookup a native query's information in the metadata.
    pub fn lookup_native_query(
        &self,
        procedure_name: &str,
    ) -> Result<&metadata::NativeQueryInfo, Error> {
        self.metadata
            .native_queries
            .0
            .get(procedure_name)
            .ok_or(Error::ProcedureNotFound(procedure_name.to_string()))
    }

    pub fn lookup_relationship(&self, name: &str) -> Result<&models::Relationship, Error> {
        self.relationships
            .get(name)
            .ok_or(Error::RelationshipNotFound(name.to_string()))
    }

    /// Looks up the binary comparison operator's PostgreSQL name and arguments' type in the metadata.
    pub fn lookup_comparison_operator(
        &self,
        scalar_type: &metadata::ScalarType,
        name: &str,
    ) -> Result<&'request metadata::ComparisonOperator, Error> {
        self.metadata
            .comparison_operators
            .0
            .get(scalar_type)
            .and_then(|ops| ops.get(name))
            .ok_or(Error::OperatorNotFound {
                operator_name: name.to_string(),
                type_name: scalar_type.clone(),
            })
    }

    /// Try to get the variables table reference. This will fail if no variables were passed
    /// as part of the query request.
    pub fn get_variables_table(&self) -> Result<sql::ast::TableReference, Error> {
        match &self.variables_table {
            None => Err(Error::UnexpectedVariable),
            Some(t) => Ok(t.clone()),
        }
    }
}

impl CollectionInfo<'_> {
    /// Lookup a column in a collection.
    pub fn lookup_column(&self, column_name: &str) -> Result<ColumnInfo, Error> {
        match self {
            CollectionInfo::Table { name, info } => info
                .columns
                .get(column_name)
                .map(|column_info| ColumnInfo {
                    name: sql::ast::ColumnName(column_info.name.clone()),
                    r#type: column_info.r#type.clone(),
                })
                .ok_or(Error::ColumnNotFoundInCollection(
                    column_name.to_string(),
                    name.to_string(),
                )),
            CollectionInfo::NativeQuery { name, info } => info
                .columns
                .get(column_name)
                .map(|column_info| ColumnInfo {
                    name: sql::ast::ColumnName(column_info.name.clone()),
                    r#type: column_info.r#type.clone(),
                })
                .ok_or(Error::ColumnNotFoundInCollection(
                    column_name.to_string(),
                    name.to_string(),
                )),
        }
    }
}

impl CompositeTypeInfo<'_> {
    /// Lookup a column in a collection.
    pub fn lookup_column(&self, column_name: &str) -> Result<ColumnInfo, Error> {
        match self {
            CompositeTypeInfo::CollectionInfo(collection_info) => {
                collection_info.lookup_column(column_name)
            }
            CompositeTypeInfo::CompositeTypeInfo { name, info } => info
                .fields
                .get(column_name)
                .map(|field_info| ColumnInfo {
                    name: sql::ast::ColumnName(field_info.name.clone()),
                    r#type: field_info.r#type.clone(),
                })
                .ok_or(Error::ColumnNotFoundInCollection(
                    column_name.to_string(),
                    name.clone(),
                )),
        }
    }
}

impl Default for State {
    fn default() -> State {
        State {
            native_queries: NativeQueries::new(),
            global_table_index: TableAliasIndex(0),
        }
    }
}

impl State {
    /// Build a new state.
    pub fn new() -> State {
        State::default()
    }

    /// When variables are passed to the query, create an alias for the variables table and
    /// a from clause.
    pub fn make_variables_table(
        &mut self,
        variables: &Option<Vec<BTreeMap<String, serde_json::Value>>>,
    ) -> Option<(sql::ast::From, sql::ast::TableReference)> {
        match variables {
            None => None,
            Some(_variables) => {
                let variables_table_alias = self.make_table_alias("%variables_table".to_string());
                let table_reference =
                    sql::ast::TableReference::AliasedTable(variables_table_alias.clone());
                Some((
                    sql::helpers::from_variables(variables_table_alias),
                    table_reference,
                ))
            }
        }
    }

    /// Introduce a new native query to the generated sql.
    pub fn insert_native_query(
        &mut self,
        name: &str,
        info: metadata::NativeQueryInfo,
        arguments: BTreeMap<String, models::Argument>,
    ) -> sql::ast::TableReference {
        let alias = self.make_native_query_table_alias(name);
        self.native_queries.native_queries.push(NativeQueryInfo {
            info,
            arguments,
            alias: alias.clone(),
        });
        sql::ast::TableReference::AliasedTable(alias)
    }

    /// Fetch the tracked native queries used in the query plan and their table alias.
    pub fn get_native_queries(self) -> Vec<NativeQueryInfo> {
        self.native_queries.native_queries
    }

    /// increment the table index and return the current one.
    fn next_global_table_index(&mut self) -> TableAliasIndex {
        let TableAliasIndex(index) = self.global_table_index;
        self.global_table_index = TableAliasIndex(index + 1);
        TableAliasIndex(index)
    }

    // aliases

    /// Create table aliases using this function so they get a unique index.
    pub fn make_table_alias(&mut self, name: String) -> sql::ast::TableAlias {
        sql::ast::TableAlias {
            unique_index: self.next_global_table_index().0,
            name,
        }
    }

    /// Create a table alias for left outer join lateral part.
    /// Provide an index and a source table name so we avoid name clashes,
    /// and get an alias.
    pub fn make_relationship_table_alias(&mut self, name: &str) -> sql::ast::TableAlias {
        self.make_table_alias(format!("RELATIONSHIP_{}", name))
    }

    /// Create a table alias for order by target part.
    /// Provide an index and a source table name (to disambiguate the table being queried),
    /// and get an alias.
    pub fn make_order_path_part_table_alias(&mut self, table_name: &str) -> sql::ast::TableAlias {
        self.make_table_alias(format!("ORDER_PART_{}", table_name))
    }

    /// Create a table alias for order by column.
    /// Provide an index and a source table name (to point at the table being ordered),
    /// and get an alias.
    pub fn make_order_by_table_alias(&mut self, source_table_name: &str) -> sql::ast::TableAlias {
        self.make_table_alias(format!("ORDER_FOR_{}", source_table_name))
    }

    pub fn make_native_query_table_alias(&mut self, name: &str) -> sql::ast::TableAlias {
        self.make_table_alias(format!("NATIVE_QUERY_{}", name))
    }

    /// Create a table alias for boolean expressions.
    /// Provide state for fresh names and a source table name (to point at the table
    /// being filtered), and get an alias.
    pub fn make_boolean_expression_table_alias(
        &mut self,
        source_table_name: &str,
    ) -> sql::ast::TableAlias {
        self.make_table_alias(format!("BOOLEXP_{}", source_table_name))
    }
}

impl NativeQueries {
    fn new() -> NativeQueries {
        NativeQueries {
            native_queries: vec![],
        }
    }
}
