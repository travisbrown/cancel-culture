use bytes::Bytes;
use cancel_culture::wayback::{cdx::Client, Item};
use chrono::NaiveDate;
use std::fs;

const ITEM_QUERY: &str = "twitter.com/travisbrown/status/1323554460765925376";

fn item() -> Item {
    Item::new(
        format!("https://{}", ITEM_QUERY),
        NaiveDate::from_ymd(2020, 11, 3).and_hms(9, 16, 10),
        "BHEPEG22C5COEOQD46QEFH4XK5SLN32A".to_string(),
        "text/html".to_string(),
        Some(200),
    )
}

#[tokio::test]
async fn test_search() {
    let client = Client::default();
    let results = client.search(ITEM_QUERY).await.unwrap();

    assert_eq!(results[0], item());
}

#[tokio::test]
async fn test_download() {
    let client = Client::default();

    let result = client.download(&item(), true).await.unwrap();
    let expected = Bytes::from(fs::read("examples/html/1323554460765925376.html").unwrap());

    assert_eq!(result, expected);
}
