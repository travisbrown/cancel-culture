use chrono::NaiveDateTime;
use reqwest::Result as RResult;
use std::collections::HashMap;

const DATE_FMT: &str = "%Y%m%d%H%M%S";
const CDX_BASE: &str = "https://web.archive.org/cdx/search/cdx";
const CDX_OPTIONS: &str = "&output=json&fl=original,timestamp,mimetype,statuscode";

pub struct Item {
    pub url: String,
    pub timestamp: String,
    pub datetime: NaiveDateTime,
    pub mime_type: String,
    pub status: u16,
}

pub async fn search(query: String) -> RResult<(HashMap<String, Vec<Item>>, Vec<Vec<String>>)> {
    let query_url = format!("{}?url={}{}", CDX_BASE, query, CDX_OPTIONS);
    let rows = reqwest::get(&query_url)
        .await?
        .json::<Vec<Vec<String>>>()
        .await?;

    let mut res: HashMap<String, Vec<Item>> = HashMap::new();
    let mut failed = vec![];

    for row in rows.into_iter() {
        match parse_row(&row) {
            Some(item) => match res.get_mut(&item.url) {
                Some(items) => {
                    items.push(item);
                }
                None => {
                    res.insert(item.url.clone(), vec![item]);
                }
            },
            None => {
                failed.push(row);
            }
        }
    }

    Ok((res, failed))
}

fn parse_row(row: &[String]) -> Option<Item> {
    row.get(0)
        .zip(row.get(1))
        .zip(row.get(2))
        .zip(row.get(3))
        .and_then(|(((u, t), m), s)| {
            NaiveDateTime::parse_from_str(t, DATE_FMT)
                .ok()
                .zip(s.parse::<u16>().ok())
                .map(|(d, c)| Item {
                    url: u.to_string(),
                    timestamp: t.to_string(),
                    datetime: d,
                    mime_type: m.to_string(),
                    status: c,
                })
        })
}
