//! Auto-generate insert mutations and translate them into sql ast.

use crate::translation::error::Error;
use crate::translation::helpers::{self, TableNameAndReference};
use crate::translation::query::filtering;
use crate::translation::query::values::translate_json_value;
use ndc_sdk::models;
use query_engine_metadata::metadata;
use query_engine_metadata::metadata::database;
use query_engine_sql::sql;
use std::collections::{BTreeMap, BTreeSet};

/// A representation of an auto-generated insert mutation.
///
/// This can get us `INSERT INTO <table>(<columns>) VALUES (<values>)`.
#[derive(Debug, Clone)]
pub struct InsertMutation {
    pub collection_name: String,
    pub description: String,
    pub schema_name: sql::ast::SchemaName,
    pub table_name: sql::ast::TableName,
    pub columns: BTreeMap<String, metadata::database::ColumnInfo>,
    pub constraint: Constraint,
}

/// The name and description of the constraint input argument.
#[derive(Debug, Clone)]
pub struct Constraint {
    pub argument_name: String,
    pub description: String,
}

/// generate an insert mutation.
pub fn generate(
    collection_name: &str,
    table_info: &database::TableInfo,
) -> (String, InsertMutation) {
    let name = format!("experimental_insert_{collection_name}");

    let description = format!("Insert into the {collection_name} table");

    let insert_mutation = InsertMutation {
        collection_name: collection_name.to_string(),
        description,
        schema_name: sql::ast::SchemaName(table_info.schema_name.clone()),
        table_name: sql::ast::TableName(table_info.table_name.clone()),
        columns: table_info.columns.clone(),
        constraint: Constraint {
            argument_name: "constraint".to_string(),
            description: format!(
                "Insert permission predicate over the '{collection_name}' collection"
            ),
        },
    };

    (name, insert_mutation)
}

/// Translate a single insert object into a mapping from column names to values.
fn translate_object_into_columns_and_values(
    env: &crate::translation::helpers::Env,
    state: &mut crate::translation::helpers::State,
    mutation: &InsertMutation,
    object: &serde_json::Value,
) -> Result<BTreeMap<sql::ast::ColumnName, sql::ast::InsertExpression>, Error> {
    let mut columns_to_values = BTreeMap::new();
    match object {
        serde_json::Value::Object(object) => {
            // For each field, look up the column name in the table and insert it and the value into the map.
            for (name, value) in object {
                let column_info =
                    mutation
                        .columns
                        .get(name)
                        .ok_or(Error::ColumnNotFoundInCollection(
                            name.clone(),
                            mutation.collection_name.clone(),
                        ))?;

                columns_to_values.insert(
                    sql::ast::ColumnName(column_info.name.clone()),
                    sql::ast::InsertExpression::Expression(translate_json_value(
                        env,
                        state,
                        value,
                        &column_info.r#type,
                    )?),
                );
            }
            Ok(())
        }
        serde_json::Value::Array(_) => Err(Error::UnexpectedStructure(
            "array of arrays structure in insert _objects argument. Expecting an array of objects."
                .to_string(),
        )),
        _ => Err(Error::UnexpectedStructure(
            "array of values structure in insert _objects argument. Expecting an array of objects."
                .to_string(),
        )),
    }?;
    Ok(columns_to_values)
}

/// We parse the objects that the user sent to us and we translate them to a list of columns
/// to insert and a vector of vector of values, each vector of values represents an object/row.
fn translate_objects_to_columns_and_values(
    env: &crate::translation::helpers::Env,
    state: &mut crate::translation::helpers::State,
    mutation: &InsertMutation,
    value: &serde_json::Value,
) -> Result<
    (
        Vec<sql::ast::ColumnName>,
        Vec<Vec<sql::ast::InsertExpression>>,
    ),
    Error,
> {
    match value {
        serde_json::Value::Array(array) => {
            let mut all_columns_and_values: Vec<
                BTreeMap<sql::ast::ColumnName, sql::ast::InsertExpression>,
            > = vec![];
            // We fetch the column names and values for each user specified object in the _objects array.
            for object in array {
                all_columns_and_values.push(translate_object_into_columns_and_values(
                    env, state, mutation, object,
                )?);
            }

            // Some objects might have missing columns, which indicate that they want the default value to be inserted.
            // To handle this, we take the union of column names in all objects, and then traverse each object
            // to check if it is missing a column. If it does, we add the column to its mapping with a DEFAULT expression.

            // Here we get the union of the column names.
            let union_of_columns: BTreeSet<sql::ast::ColumnName> = all_columns_and_values
                .iter()
                .map(|cols_and_vals| cols_and_vals.keys().cloned().collect::<BTreeSet<_>>())
                .fold(BTreeSet::new(), |acc, cols| {
                    acc.union(&cols).cloned().collect()
                });

            // Here we add missing column names with DEFAULT.
            for columns_and_values in &mut all_columns_and_values {
                for column_name in &union_of_columns {
                    if !columns_and_values.contains_key(column_name) {
                        columns_and_values
                            .insert(column_name.clone(), sql::ast::InsertExpression::Default);
                    }
                }

                // Finally, check that the final form of the object is fine according to the schema.
                check_columns(
                    &mutation.columns,
                    columns_and_values,
                    &mutation.collection_name,
                )?;
            }

            Ok((
                // We return an ordered vector of column names
                union_of_columns.into_iter().collect(),
                // and a vector of rows
                all_columns_and_values
                    .into_iter()
                    .map(|columns_and_values| columns_and_values.into_values().collect())
                    .collect(),
            ))
        }
        serde_json::Value::Object(_) => Err(Error::UnexpectedStructure(
            "object structure in insert _objects argument. Expecting an array of objects."
                .to_string(),
        )),
        _ => Err(Error::UnexpectedStructure(
            "value structure in insert _objects argument. Expecting an array of objects."
                .to_string(),
        )),
    }
}

/// Given the description of an insert mutation (ie, `InsertMutation`),
/// and the arguments, output the SQL AST.
pub fn translate(
    env: &crate::translation::helpers::Env,
    state: &mut crate::translation::helpers::State,
    mutation: &InsertMutation,
    arguments: &BTreeMap<String, serde_json::Value>,
) -> Result<(sql::ast::Insert, sql::ast::ColumnAlias), Error> {
    let object = arguments
        .get("_objects")
        .ok_or(Error::ArgumentNotFound("_objects".to_string()))?;

    let (columns, values) = translate_objects_to_columns_and_values(env, state, mutation, object)?;

    let table_name_and_reference = TableNameAndReference {
        name: mutation.collection_name.clone(),
        reference: sql::ast::TableReference::DBTable {
            schema: mutation.schema_name.clone(),
            table: mutation.table_name.clone(),
        },
    };

    // Build the `constraint` argument boolean expression.
    let predicate_json =
        arguments
            .get(&mutation.constraint.argument_name)
            .ok_or(Error::ArgumentNotFound(
                mutation.constraint.argument_name.clone(),
            ))?;

    let predicate: models::Expression = serde_json::from_value(predicate_json.clone())
        .map_err(|_| Error::ArgumentNotFound(mutation.constraint.argument_name.clone()))?;

    let predicate_expression = filtering::translate_expression(
        env,
        state,
        &helpers::RootAndCurrentTables {
            root_table: table_name_and_reference.clone(),
            current_table: table_name_and_reference.clone(),
        },
        &predicate,
    )?;

    let check_constraint_alias =
        sql::helpers::make_column_alias(sql::helpers::CHECK_CONSTRAINT_FIELD.to_string());

    let insert = sql::ast::Insert {
        schema: mutation.schema_name.clone(),
        table: mutation.table_name.clone(),
        columns,
        values,
        returning: sql::ast::Returning::Returning(sql::ast::SelectList::SelectListComposite(
            Box::new(sql::ast::SelectList::SelectStar),
            Box::new(sql::ast::SelectList::SelectList(vec![(
                check_constraint_alias.clone(),
                predicate_expression,
            )])),
        )),
    };

    Ok((insert, check_constraint_alias))
}

/// Check that no columns are missing, and that columns cannot be inserted to
/// are not inserted.
fn check_columns(
    columns: &BTreeMap<String, database::ColumnInfo>,
    inserted_columns: &BTreeMap<sql::ast::ColumnName, sql::ast::InsertExpression>,
    insert_name: &str,
) -> Result<(), Error> {
    for (name, column) in columns {
        match column {
            // nullable, default, and identity by default columns can be inserted into or omitted.
            database::ColumnInfo {
                nullable: database::Nullable::Nullable,
                ..
            }
            | database::ColumnInfo {
                has_default: database::HasDefault::HasDefault,
                ..
            }
            | database::ColumnInfo {
                is_identity: database::IsIdentity::IdentityByDefault,
                ..
            } => Ok(()),
            // generated columns must not be inserted into.
            database::ColumnInfo {
                is_generated: database::IsGenerated::Stored,
                ..
            } => {
                let value = inserted_columns.get(&sql::ast::ColumnName(column.name.clone()));
                match value {
                    Some(expr) if *expr != sql::ast::InsertExpression::Default => {
                        Err(Error::ColumnIsGenerated(name.clone()))
                    }
                    _ => Ok(()),
                }
            }
            // identity always columns must not be inserted into.
            database::ColumnInfo {
                is_identity: database::IsIdentity::IdentityAlways,
                ..
            } => {
                let value = inserted_columns.get(&sql::ast::ColumnName(column.name.clone()));
                match value {
                    Some(expr) if *expr != sql::ast::InsertExpression::Default => {
                        Err(Error::ColumnIsIdentityAlways(name.clone()))
                    }
                    _ => Ok(()),
                }
            }
            // regular columns must be inserted into.
            _ => {
                let value = inserted_columns.get(&sql::ast::ColumnName(column.name.clone()));
                match value {
                    Some(sql::ast::InsertExpression::Expression(_)) => Ok(()),
                    Some(sql::ast::InsertExpression::Default) | None => Err(
                        Error::MissingColumnInInsert(name.clone(), insert_name.to_owned()),
                    ),
                }
            }
        }?;
    }
    Ok(())
}
