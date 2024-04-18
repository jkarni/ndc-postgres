//! Handle the translation of literal values.

use crate::translation::{error::Error, helpers::State};
use query_engine_metadata::metadata::database;
use query_engine_sql::sql::{self, ast::ColumnReference, helpers::simple_select};
use sql::ast::{Expression, Value};

/// Convert a JSON value into a SQL value.
pub fn translate_json_value(
    state: &mut State,
    value: &serde_json::Value,
    r#type: &database::Type,
) -> Result<sql::ast::Expression, Error> {
    match (value, r#type) {
        (serde_json::Value::Null, _) => Ok(Expression::Cast {
            expression: Box::new(Expression::Value(Value::Null)),
            r#type: type_to_ast_scalar_type(r#type),
        }),
        (serde_json::Value::Bool(b), _) => Ok(Expression::Value(Value::Bool(*b))),
        (serde_json::Value::Number(n), _) => {
            let lit = n
                .as_f64()
                .ok_or(Error::UnableToDeserializeNumberAsF64(n.clone()))?;
            Ok(Expression::Value(Value::Float8(lit)))
        }
        (serde_json::Value::String(str), _) => Ok(Expression::Cast {
            expression: Box::new(Expression::Value(Value::String(str.clone()))),
            r#type: type_to_ast_scalar_type(r#type),
        }),
        (serde_json::Value::Array(_), database::Type::ArrayType(_)) => {
            let value_expression =
                sql::ast::Expression::Value(sql::ast::Value::JsonValue(value.clone()));
            Ok(translate_projected_variable(
                state,
                r#type,
                value_expression,
            ))
        }
        (serde_json::Value::Object(_obj), database::Type::CompositeType(_type_name)) => {
            let value_expression =
                sql::ast::Expression::Value(sql::ast::Value::JsonValue(value.clone()));
            Ok(translate_projected_variable(
                state,
                r#type,
                value_expression,
            ))
        }
        // If the type is not congruent with the value constructor we simply pass the json value
        // raw and cast to the specified type. This allows users to consume any json values,
        // treating them either as actual json or as any type that has a cast from json defined.
        _ => Ok(sql::ast::Expression::Cast {
            expression: Box::new(sql::ast::Expression::Cast {
                expression: Box::new(Expression::Value(Value::JsonValue(value.clone()))),
                r#type: sql::ast::ScalarTypeName::new_unqualified("jsonb"),
            }),
            r#type: type_to_ast_scalar_type(r#type),
        }),
    }
}

/// Translate a NDC 'Type' to an SQL type name.
fn type_to_ast_scalar_type(typ: &database::Type) -> sql::ast::ScalarTypeName {
    match typ {
        query_engine_metadata::metadata::Type::ArrayType(t) => {
            // This will collapse nested arrays. This is fine since it matches the behavior of
            // Postgres where these are unsupported anyway.
            let mut scalar_type = type_to_ast_scalar_type(t);
            scalar_type.is_array = true;
            scalar_type
        }
        query_engine_metadata::metadata::Type::ScalarType(t) => {
            // TODO: This will need access to a mapping between ndc-type names and db type names
            sql::ast::ScalarTypeName::new_unqualified(&t.0)
        }
        query_engine_metadata::metadata::Type::CompositeType(t) => {
            // TODO: This will need access to a mapping between ndc-type names and db type names
            sql::ast::ScalarTypeName::new_unqualified(t)
        }
    }
}

/// Convert a variable into a SQL value.
pub fn translate_variable(
    state: &mut State,
    variables_table: sql::ast::TableReference,
    variable: &str,
    r#type: &database::Type,
) -> sql::ast::Expression {
    let variables_reference = Expression::ColumnReference(ColumnReference::AliasedColumn {
        table: variables_table,
        column: sql::helpers::make_column_alias(sql::helpers::VARIABLES_FIELD.to_string()),
    });

    // We use the binop '->' to project (as jsonb) the value of a variable from the data column of
    // the variable table.
    let projected_variable_exp = sql::ast::Expression::BinaryOperation {
        left: Box::new(variables_reference),
        operator: sql::ast::BinaryOperator("->".to_string()),
        right: Box::new(sql::ast::Expression::Value(sql::ast::Value::String(
            variable.to_string(),
        ))),
    };

    translate_projected_variable(state, r#type, projected_variable_exp)
}

/// Produce a SQL expression that translates an expression of Postgres type 'jsonb' into a given
/// type.
///
/// For scalar types and object types this is a simple operation, since we can rely on builtin
/// functions.
///
/// Arrays are more complex since there isn't a builtin function that handles array
/// types.
pub fn translate_projected_variable(
    state: &mut State,
    r#type: &database::Type,
    exp: sql::ast::Expression,
) -> sql::ast::Expression {
    match r#type {
        database::Type::CompositeType(_type_name) => sql::ast::Expression::FunctionCall {
            function: sql::ast::Function::JsonbPopulateRecord,
            args: vec![
                sql::ast::Expression::Cast {
                    expression: Box::new(sql::ast::Expression::Value(sql::ast::Value::Null)),
                    r#type: type_to_ast_scalar_type(r#type),
                },
                exp,
            ],
        },
        // We translate projection of array types into the following sql:
        // ```
        // ( SELECT
        //     array_agg(
        //       jsonb_populate_record(cast(null as <type>), "array"."element")
        //     ) AS "element"
        //   FROM
        //    jsonb_array_elements((<variable_table> -> <label>)) AS "array"("element")
        // )
        // ```
        database::Type::ArrayType(type_name) => {
            let array_table = state.make_table_alias("array".to_string());
            let element_column = sql::ast::ColumnAlias {
                name: "element".to_string(),
            };

            let from_arr = sql::ast::From::JsonbArrayElements {
                expression: exp,
                alias: array_table.clone(),
                column: element_column.clone(),
            };

            let element_expression =
                sql::ast::Expression::ColumnReference(ColumnReference::AliasedColumn {
                    table: sql::ast::TableReference::AliasedTable(array_table.clone()),
                    column: element_column.clone(),
                });

            let converted_element_exp =
                translate_projected_variable(state, type_name, element_expression);

            let mut result_select = simple_select(vec![(
                element_column.clone(),
                sql::ast::Expression::FunctionCall {
                    function: sql::ast::Function::Unknown("array_agg".to_string()),
                    args: vec![converted_element_exp],
                },
            )]);

            result_select.from = Some(from_arr);

            sql::ast::Expression::CorrelatedSubSelect(Box::new(result_select))
        }
        database::Type::ScalarType(_) => sql::ast::Expression::Cast {
            expression: Box::new(sql::ast::Expression::BinaryOperation {
                left: Box::new(exp),
                operator: sql::ast::BinaryOperator("#>>".to_string()),
                right: Box::new(sql::ast::Expression::Cast {
                    expression: Box::new(sql::ast::Expression::Value(sql::ast::Value::Array(
                        vec![],
                    ))),
                    r#type: {
                        let mut text_arr = sql::ast::ScalarTypeName::new_unqualified("text");
                        text_arr.is_array = true;
                        text_arr
                    },
                }),
            }),
            r#type: type_to_ast_scalar_type(r#type),
        },
    }
}
