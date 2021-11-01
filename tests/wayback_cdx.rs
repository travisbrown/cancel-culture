use cancel_culture::wayback::cdx::Client;
use chrono::NaiveDate;
use std::fs::File;
use std::io::{BufRead, BufReader, Error};
use wayback_rs::Item;

const EXAMPLE_ITEM_QUERY: &str = "twitter.com/travisbrown/status/1323554460765925376";

fn example_item() -> Item {
    Item::new(
        format!("https://{}", EXAMPLE_ITEM_QUERY),
        NaiveDate::from_ymd(2020, 11, 3).and_hms(9, 16, 10),
        "BHEPEG22C5COEOQD46QEFH4XK5SLN32A".to_string(),
        "text/html".to_string(),
        0,
        Some(200),
    )
}

fn example_lines() -> Vec<String> {
    let file = File::open("examples/html/1323554460765925376.html").unwrap();
    let reader = BufReader::new(file);

    reader
        .lines()
        .collect::<Result<Vec<String>, Error>>()
        .unwrap()
}

#[tokio::test]
async fn test_search() {
    let client = Client::default();
    let results = client.search(EXAMPLE_ITEM_QUERY).await.unwrap();

    assert_eq!(results[0], example_item());
}

#[tokio::test]
async fn test_download() {
    let client = Client::default();
    let result = client.download(&example_item(), true).await.unwrap();
    let result_lines = result
        .lines()
        .collect::<Result<Vec<String>, Error>>()
        .unwrap();

    assert_eq!(result_lines, example_lines());
}
