use cancel_culture::browser;
use chrono::NaiveDate;
use clap::{crate_authors, crate_version, Clap};
use fantoccini::{Client, Element, Locator};
use std::time::Duration;
use tokio::time::delay_for;

/// Low-tech way to look up URLs on the Wayback Machine! The CDX API is better (when it works).
#[tokio::main]
async fn main() -> Result<(), fantoccini::error::CmdError> {
    let opts: Opts = Opts::parse();

    let mut client = browser::make_client_or_panic(
        &opts.browser,
        !opts.disable_headless,
        opts.host.as_deref(),
        opts.port,
    )
    .await;
    client.goto(&mk_wayback_search_url(&opts.query)).await?;

    let mut summary = client.wait_for_find(SUMMARY_LOC).await?;

    // This shouldn't really be necessary, but is for some reason?
    delay_for(Duration::from_millis(500)).await;

    let expected_count = parse_summary_text(&summary.text().await?.trim_start());
    let mut seen_count = 0;

    let sort = client.find(SORT_LOC).await?;
    sort.click().await?;

    let sort = client.find(SORT_LOC).await?;
    sort.click().await?;

    loop {
        let res = extract_links(&mut client).await?;
        seen_count += res.len();

        for link in res {
            let date_from_str = link
                .date_from
                .as_ref()
                .map_or(String::new(), |d| d.to_string());
            let date_to_str = link
                .date_to
                .as_ref()
                .map_or(String::new(), |d| d.to_string());

            println!(
                "{} {} {} {}",
                link.url, link.mimetype, date_from_str, date_to_str
            );
        }

        match get_next_link(&mut client).await? {
            Some(element) => {
                element.click().await?;
            }
            None => break,
        }
    }

    if seen_count == expected_count {
        eprintln!(
            "Success! (seen: {}; expected: {})",
            seen_count, expected_count
        );
    } else {
        eprintln!(
            "There was an issue! (seen: {}; expected: {})",
            seen_count, expected_count
        );
    }

    Ok(())
}

async fn get_next_link(
    client: &mut Client,
) -> Result<Option<Element>, fantoccini::error::CmdError> {
    let mut next = client.wait_for_find(NEXT_LOC).await?;
    match next.attr("class").await? {
        Some(class_value) => {
            if class_value.contains("disabled") {
                Ok(None)
            } else {
                Ok(Some(next))
            }
        }
        None => Ok(Some(next)),
    }
}

async fn extract_links(
    client: &mut Client,
) -> Result<Vec<WaybackLink>, fantoccini::error::CmdError> {
    let mut table = client.wait_for_find(TABLE_LOC).await?;
    let mut rows = table.find_all(ROW_LOC).await?;
    let mut res = vec![];

    for row in &mut rows {
        let url = row.find(URL_LOC).await?.text().await?;
        let mimetype = row.find(MIME_TYPE_LOC).await?.text().await?;
        let date_from = parse_wayback_date(&row.find(DATE_FROM_LOC).await?.text().await?);
        let date_to = parse_wayback_date(&row.find(DATE_TO_LOC).await?.text().await?);

        res.push(WaybackLink {
            url,
            mimetype,
            date_from,
            date_to,
        });
    }

    Ok(res)
}

fn parse_wayback_date(input: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(input, DATE_FMT).ok()
}

fn parse_summary_text(input: &str) -> usize {
    input
        .chars()
        .take_while(|c| c.is_digit(10) || *c == ',')
        .filter(|c| *c != ',')
        .collect::<String>()
        .parse::<usize>()
        .unwrap_or(0)
}

fn mk_wayback_search_url(query: &str) -> String {
    format!("https://web.archive.org/web/*/{}", query)
}

#[derive(Clap)]
#[clap(version = crate_version!(), author = crate_authors!())]
struct Opts {
    query: String,
    #[clap(short, long)]
    host: Option<String>,
    #[clap(short, long)]
    port: Option<u16>,
    #[clap(short, long)]
    disable_headless: bool,
    #[clap(short, long, default_value = "chrome")]
    browser: String,
}

#[derive(Debug)]
pub struct WaybackLink {
    pub url: String,
    pub mimetype: String,
    pub date_from: Option<NaiveDate>,
    pub date_to: Option<NaiveDate>,
}

const DATE_FMT: &str = "%B %d, %Y";

const SUMMARY_LOC: Locator = Locator::XPath(
    "//h2[@id='query-summary'][contains(@style, 'visible')][contains(text(), 'URLs')]",
);
const SORT_LOC: Locator = Locator::Css("th.dateTo");
const TABLE_LOC: Locator = Locator::Id("resultsUrl");
const ROW_LOC: Locator = Locator::XPath("tbody/tr");
const NEXT_LOC: Locator = Locator::Id("resultsUrl_next");

const URL_LOC: Locator = Locator::Css("td.url");
const MIME_TYPE_LOC: Locator = Locator::Css("td.mimetype");
const DATE_FROM_LOC: Locator = Locator::Css("td.dateFrom");
const DATE_TO_LOC: Locator = Locator::Css("td.dateTo");
