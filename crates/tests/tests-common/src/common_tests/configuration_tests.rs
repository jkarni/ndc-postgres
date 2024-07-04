//! Tests the configuration generation has not changed.

use ndc_postgres_configuration::version4;
use ndc_postgres_configuration::ParsedConfiguration;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Test native query introspection.
pub async fn test_native_operation_create(
    connection_string: &str,
    ndc_metadata_path: impl AsRef<Path> + Sync,
    sql: String,
    kind: version4::native_operations::Kind,
) -> anyhow::Result<version4::metadata::NativeQueryInfo> {
    let configuration = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(ndc_metadata_path);

    let parsed_configuration =
        ndc_postgres_configuration::parse_configuration(configuration).await?;

    match parsed_configuration {
        ParsedConfiguration::Version3(_) => anyhow::bail!("version3"),
        ParsedConfiguration::Version4(parsed_configuration) => {
            let result = version4::native_operations::create(
                &parsed_configuration,
                connection_string,
                &PathBuf::from("test.sql"),
                &sql,
                kind,
            )
            .await?;

            Ok(result)
        }
    }
}

/// Tests that configuration generation has not changed.
///
/// This test does not use insta snapshots because it checks the NDC metadata file that is shared
/// with other tests.
///
/// If you have changed it intentionally, run `just generate-configuration`.
pub async fn introspection_is_idempotent(
    connection_string: &str,
    ndc_metadata_path: impl AsRef<Path> + Sync,
) -> anyhow::Result<()> {
    let parsed_configuration = ndc_postgres_configuration::parse_configuration(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .join(ndc_metadata_path),
    )
    .await?;
    let environment = HashMap::from([(
        ndc_postgres_configuration::DEFAULT_CONNECTION_URI_VARIABLE.into(),
        connection_string.into(),
    )]);

    let introspected_configuration =
        ndc_postgres_configuration::introspect(parsed_configuration.clone(), environment).await?;

    assert_eq!(parsed_configuration, introspected_configuration);
    Ok(())
}
