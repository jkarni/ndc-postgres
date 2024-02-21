//! Runs the tests provided by the ndc-spec.

#![cfg(test)]

use super::common;
use tests_common::common_tests::ndc_tests;

#[tokio::test]
async fn test_connector() -> ndc_tests::Result {
    let router = common::create_router().await;
    ndc_tests::test_connector(router).await
}
