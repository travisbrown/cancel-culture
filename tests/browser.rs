use cancel_culture::browser::make_client;

#[ignore]
#[tokio::test]
async fn test_make_client() {
    let client = make_client("chrome", true, None, None).await;

    assert!(client.is_ok());
}
