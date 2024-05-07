mod common;

use tokio::fs;

use ndc_postgres_cli::*;
use ndc_postgres_configuration as configuration;
use ndc_postgres_configuration::environment::FixedEnvironment;
use ndc_postgres_configuration::ParsedConfiguration;

const CONNECTION_URI: &str = "postgresql://postgres:password@localhost:64002";

#[tokio::test]
async fn test_update_configuration() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;

    let connection_uri = configuration::ConnectionUri(configuration::Secret::FromEnvironment {
        variable: "CONNECTION_URI".into(),
    });

    {
        let connection_settings =
            configuration::version3::connection_settings::DatabaseConnectionSettings {
                connection_uri: connection_uri.clone(),
                ..configuration::version3::connection_settings::DatabaseConnectionSettings::empty()
            };
        let input = ParsedConfiguration::Version3(configuration::version3::RawConfiguration {
            connection_settings,
            ..configuration::version3::RawConfiguration::empty()
        });
        configuration::write_parsed_configuration(input, &dir).await?;
    }

    let environment =
        FixedEnvironment::from([("CONNECTION_URI".into(), CONNECTION_URI.to_string())]);
    let context = Context {
        context_path: dir.path().to_owned(),
        environment,
        release_version: None,
    };
    run(Command::Update, context).await?;

    let configuration_file_path = dir.path().join("configuration.json");
    assert!(configuration_file_path.exists());
    let contents = fs::read_to_string(configuration_file_path).await?;
    common::assert_ends_with_newline(&contents);
    let output: ParsedConfiguration = configuration::parse_configuration(&dir).await?;
    match output {
        ParsedConfiguration::Version3(configuration::version3::RawConfiguration {
            connection_settings,
            metadata,
            ..
        }) => {
            assert_eq!(connection_settings.connection_uri, connection_uri);
            let some_table_metadata = metadata.tables.0.get("Artist");
            assert!(some_table_metadata.is_some());
        }
        ParsedConfiguration::Version4(_) => panic!("Expected version 3"),
    }

    Ok(())
}
