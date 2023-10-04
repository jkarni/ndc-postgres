use ndc_postgres::configuration::RawConfiguration;
use schemars::{gen::SchemaSettings, schema::RootSchema};

fn main() {
    let schema: RootSchema = SchemaSettings::draft2019_09()
        .into_generator()
        .into_root_schema_for::<RawConfiguration>();

    println!("{}", serde_json::to_string_pretty(&schema).unwrap());
}
