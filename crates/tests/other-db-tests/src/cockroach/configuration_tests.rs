//! Tests that configuration generation has not changed.
//!
//! If you have changed it intentionally, run `just generate-chinook-configuration`.

#[cfg(test)]
mod configuration_tests {
    use super::super::common;
    use tests_common::common_tests;

    #[tokio::test]
    async fn test_configure() {
        common_tests::configuration_tests::test_configure(
            common::CONNECTION_STRING,
            common::CHINOOK_DEPLOYMENT_PATH,
        )
        .await
    }

    #[test]
    fn configuration_conforms_to_the_schema() {
        common_tests::configuration_tests::configuration_conforms_to_the_schema(
            common::CHINOOK_DEPLOYMENT_PATH,
        )
    }
}
