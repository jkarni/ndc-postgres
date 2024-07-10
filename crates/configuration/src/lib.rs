mod configuration;
mod values;

pub mod environment;
pub mod error;
pub mod metrics;

pub mod version3;
pub mod version4;
pub mod version5;

pub use configuration::{
    generate_latest_schema, introspect, make_runtime_configuration, parse_configuration,
    upgrade_to_latest_version, write_parsed_configuration, Configuration, ParsedConfiguration,
    DEFAULT_CONNECTION_URI_VARIABLE,
};
pub use values::{ConnectionUri, IsolationLevel, PoolSettings, Secret};

pub use metrics::Metrics;

#[derive(Debug, Copy, Clone)]
pub enum VersionTag {
    Version3,
    Version4,
    Version5,
}

#[cfg(test)]
pub mod common {
    use std::fmt::Write;
    use std::path::{Path, PathBuf};

    /// Find the project root via the crate root provided by `cargo test`,
    /// and get our single static configuration file.
    /// This depends on the convention that all our crates live in `/crates/<name>`
    /// and will break in the unlikely case that we change this
    pub fn get_path_from_project_root(ndc_metadata_path: impl AsRef<Path>) -> PathBuf {
        let mut d = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        d.push("../../");
        d.push(ndc_metadata_path);

        d
    }
    /// Checks that a given value conforms to the schema generated by `schemars`.
    ///
    /// Panics with a human-readable error if the value does not conform, or if the
    /// schema could not be compiled.
    pub fn check_value_conforms_to_schema<T: schemars::JsonSchema>(value: &serde_json::Value) {
        let schema_json = serde_json::to_value(schemars::schema_for!(T))
            .expect("the schema could not be converted to JSON");
        let schema = jsonschema::JSONSchema::options()
            .with_draft(jsonschema::Draft::Draft7)
            .compile(&schema_json)
            .expect("the schema could not be compiled");

        let result = schema.validate(value);

        match result {
            Ok(()) => (),
            Err(errors) => {
                panic!(
                    "The configuration does not conform to the schema.\n{}",
                    errors.fold(String::new(), |mut str, error| {
                        let _ = write!(
                            str,
                            "{}\ninstance path: {}\nschema path:   {}\n\n",
                            error, error.instance_path, error.schema_path
                        );
                        str
                    })
                )
            }
        }
    }
}
