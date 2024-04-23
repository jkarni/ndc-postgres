//! Handle 'rows' and 'aggregates' translation.

use std::collections::BTreeMap;

use indexmap::IndexMap;

use ndc_sdk::models;

use super::aggregates;
use super::fields::translate_fields;
use super::filtering;
use super::relationships;
use super::sorting;
use crate::translation::error::Error;
use crate::translation::helpers::{
    CollectionInfo, Env, RootAndCurrentTables, State, TableNameAndReference,
};
use query_engine_sql::sql;

/// Translate aggregates query to sql ast.
pub fn translate_aggregate_query(
    env: &Env,
    state: &mut State,
    current_table: &TableNameAndReference,
    from_clause: &sql::ast::From,
    query: &models::Query,
) -> Result<Option<sql::ast::Select>, Error> {
    // fail if no aggregates defined at all
    match &query.aggregates {
        None => Ok(None),
        Some(aggregate_fields) => {
            // create all aggregate columns
            let aggregate_columns =
                aggregates::translate(&current_table.reference, aggregate_fields)?;

            // construct a simple select with the table name, alias, and selected columns.
            let columns_select = sql::helpers::simple_select(aggregate_columns);

            // create the select clause and the joins, order by, where clauses.
            // We don't add the limit afterwards.
            let mut select =
                translate_query_part(env, state, current_table, query, columns_select, vec![])?;
            // we remove the order by part though because it is only relevant for group by clauses,
            // which we don't support at the moment.
            select.order_by = sql::helpers::empty_order_by();

            select.from = Some(from_clause.clone());

            Ok(Some(select))
        }
    }
}

/// Whether this rows query returns fields or not.
pub enum ReturnsFields {
    FieldsWereRequested,
    NoFieldsWereRequested,
}

/// Translate rows part of query to sql ast.
pub fn translate_rows_query(
    env: &Env,
    state: &mut State,
    current_table: &TableNameAndReference,
    from_clause: &sql::ast::From,
    query: &models::Query,
) -> Result<(ReturnsFields, sql::ast::Select), Error> {
    // join aliases
    let mut join_relationship_fields: Vec<relationships::JoinFieldInfo> = vec![];

    // translate fields to select list
    let fields = query.fields.clone().unwrap_or_default();

    // remember whether we fields were requested or not.
    // The case were fields were not requested, and also no aggregates were requested,
    // can be used for `__typename` queries.
    let returns_fields = if IndexMap::is_empty(&fields) {
        ReturnsFields::NoFieldsWereRequested
    } else {
        ReturnsFields::FieldsWereRequested
    };

    // translate fields to columns or relationships.
    let fields_select = translate_fields(
        env,
        state,
        fields,
        current_table,
        &mut join_relationship_fields,
    )?;

    // create the select clause and the joins, order by, where clauses.
    // We'll add the limit afterwards.
    let mut select = translate_query_part(
        env,
        state,
        current_table,
        query,
        fields_select,
        join_relationship_fields,
    )?;

    select.from = Some(from_clause.clone());

    // Add the limit.
    select.limit = sql::ast::Limit {
        limit: query.limit,
        offset: query.offset,
    };
    Ok((returns_fields, select))
}

/// Translate the lion (or common) part of 'rows' or 'aggregates' part of a query.
/// Specifically, from, joins, order bys, and where clauses.
///
/// This expects to get the relevant information about tables, relationships, the root table,
/// and the query, as well as the columns and join fields after processing.
///
/// One thing that this doesn't do that you want to do for 'rows' and not 'aggregates' is
/// set the limit and offset so you want to do that after calling this function.
fn translate_query_part(
    env: &Env,
    state: &mut State,
    current_table: &TableNameAndReference,
    query: &models::Query,
    mut select: sql::ast::Select,
    join_relationship_fields: Vec<relationships::JoinFieldInfo>,
) -> Result<sql::ast::Select, Error> {
    let root_table = current_table.clone();

    // the root table and the current table are the same at this point
    let root_and_current_tables = RootAndCurrentTables {
        root_table,
        current_table: current_table.clone(),
    };

    // translate order_by
    let (order_by, order_by_joins) =
        sorting::translate_order_by(env, state, &root_and_current_tables, &query.order_by)?;

    select.joins.extend(order_by_joins);

    // translate where
    let (filter, filter_joins) = match &query.predicate {
        None => Ok((sql::helpers::true_expr(), vec![])),
        Some(predicate) => {
            filtering::translate_expression(env, state, &root_and_current_tables, predicate)
        }
    }?;

    select.where_ = sql::ast::Where(filter);

    // collect any joins for relationships
    let relationship_joins = relationships::translate_joins(
        env,
        state,
        &root_and_current_tables,
        join_relationship_fields,
    )?;

    select.joins.extend(relationship_joins);

    select.joins.extend(filter_joins);

    select.order_by = order_by;

    Ok(select)
}

/// Create a from clause from a collection name and its reference.
pub fn make_from_clause_and_reference(
    collection_name: &str,
    arguments: &BTreeMap<String, models::Argument>,
    env: &Env,
    state: &mut State,
    collection_alias: Option<sql::ast::TableAlias>,
) -> Result<(TableNameAndReference, sql::ast::From), Error> {
    let collection_alias = match collection_alias {
        None => state.make_table_alias(collection_name.to_string()),
        Some(alias) => alias,
    };
    let collection_alias_name = sql::ast::TableReference::AliasedTable(collection_alias.clone());

    // find the table according to the metadata.
    let collection_info = env.lookup_collection(collection_name)?;
    let from_clause = make_from_clause(state, &collection_alias, &collection_info, arguments);

    let current_table = TableNameAndReference {
        name: collection_name.to_string(),
        reference: collection_alias_name.clone(),
    };
    Ok((current_table, from_clause))
}

/// Build a FROM clause from a collection info and an alias.
/// Will add a Native Query to the 'State' if the collection is a native query.
fn make_from_clause(
    state: &mut State,
    current_table_alias: &sql::ast::TableAlias,
    collection_info: &CollectionInfo,
    arguments: &BTreeMap<String, models::Argument>,
) -> sql::ast::From {
    match collection_info {
        CollectionInfo::Table { info, .. } => {
            let db_table = sql::ast::TableReference::DBTable {
                schema: sql::ast::SchemaName(info.schema_name.clone()),
                table: sql::ast::TableName(info.table_name.clone()),
            };
            sql::ast::From::Table {
                reference: db_table,
                alias: current_table_alias.clone(),
            }
        }
        CollectionInfo::NativeQuery { name, info } => {
            let aliased_table = state.insert_native_query(name, (*info).clone(), arguments.clone());
            sql::ast::From::Table {
                reference: aliased_table,
                alias: current_table_alias.clone(),
            }
        }
    }
}
