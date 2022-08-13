use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use scraper::element_ref::ElementRef;
use scraper::selector::Selector;
use scraper::Html;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Posting {
    pub identifier: String,
    pub position: u32,
    pub comment_count: u32,
    pub date_created: DateTime<Utc>,
    pub date_published: DateTime<Utc>,
    pub url: String,
    pub author: Person,
    pub is_part_of: Option<String>,
    pub interaction_counters: Vec<InteractionCounter>,
    pub article_body: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Person {
    pub identifier: String,
    pub additional_name: String,
    pub given_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InteractionCounter {
    pub interaction_type: String,
    pub count: u32,
    pub url: String,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("I/O error")]
    Io(#[from] std::io::Error),
    #[error("Missing property")]
    MissingProperty(String),
    #[error("Invalid property")]
    InvalidProperty(String),
    #[error("Duplicate property")]
    DuplicateProperty(String),
    #[error("Missing child")]
    MissingChild(String),
    #[error("Invalid child")]
    InvalidChild(String),
    #[error("Duplicate child")]
    DuplicateChild(String),
}

// Example: "2022-05-25T09:25:12.000Z"
const DATE_TIME_FORMAT: &str = "%Y-%m-%dT%H:%M:%S%.3fZ";

lazy_static::lazy_static! {
    static ref COLLECTION_SEL: Selector =
        Selector::parse("div[itemtype='https://schema.org/Collection']").unwrap();
    static ref POSTING_SEL: Selector =
        Selector::parse("div[itemtype='https://schema.org/SocialMediaPosting']").unwrap();
    static ref IDENTIFIER_SEL: Selector = Selector::parse("meta[itemprop='identifier']").unwrap();
    static ref POSITION_SEL: Selector = Selector::parse("meta[itemprop='position']").unwrap();
    static ref COMMENT_COUNT_SEL: Selector =
        Selector::parse("meta[itemprop='commentCount']").unwrap();
    static ref DATE_CREATED_SEL: Selector =
        Selector::parse("meta[itemprop='dateCreated']").unwrap();
    static ref DATE_PUBLISHED_SEL: Selector =
        Selector::parse("meta[itemprop='datePublished']").unwrap();
    static ref URL_SEL: Selector = Selector::parse("meta[itemprop='url']").unwrap();
    static ref AUTHOR_SEL: Selector =
        Selector::parse("div[itemprop='author'][itemtype='https://schema.org/Person']").unwrap();
    static ref ADDITIONAL_NAME_SEL: Selector =
        Selector::parse("meta[itemprop='additionalName']").unwrap();
    static ref GIVEN_NAME_SEL: Selector = Selector::parse("meta[itemprop='givenName']").unwrap();
    static ref IS_PART_OF_SEL: Selector = Selector::parse("meta[itemprop='isPartOf']").unwrap();
    static ref INTERACTION_COUNTER_SEL: Selector = Selector::parse(
        "div[itemprop='interactionStatistic'][itemtype='https://schema.org/InteractionCounter']"
    )
    .unwrap();
    static ref NAME_SEL: Selector = Selector::parse("meta[itemprop='name']").unwrap();
    static ref USER_INTERACTION_COUNT_SEL: Selector =
        Selector::parse("meta[itemprop='userInteractionCount']").unwrap();
    static ref ARTICLE_BODY_SEL: Selector = Selector::parse("div[itemprop='articleBody']").unwrap();
}

pub fn extract_postings(doc: &Html) -> Result<Option<Vec<Posting>>, Error> {
    if is_collection(doc) {
        let mut postings = doc
            .select(&POSTING_SEL)
            .map(|element| parse_posting(&element))
            .collect::<Result<Vec<_>, Error>>()?;

        postings.sort_by_key(|posting| posting.position);

        Ok(Some(postings))
    } else {
        Ok(None)
    }
}

fn is_collection(doc: &Html) -> bool {
    doc.select(&COLLECTION_SEL).next().is_some()
}

fn parse_posting(element: &ElementRef) -> Result<Posting, Error> {
    let identifier = get_property_value(element, &IDENTIFIER_SEL, "identifier", false)?;
    let position = get_property_value_u32(element, &POSITION_SEL, "position")?;
    let comment_count = get_property_value_u32(element, &COMMENT_COUNT_SEL, "commentCount")?;
    let date_created = get_property_value_date_time(element, &DATE_CREATED_SEL, "dateCreated")?;
    let date_published =
        get_property_value_date_time(element, &DATE_PUBLISHED_SEL, "datePublished")?;
    let url = get_property_value(element, &URL_SEL, "url", false)?;
    let author_element = get_child(element, &AUTHOR_SEL, "author")?;
    let author = parse_person(&author_element)?;
    let is_part_of = get_optional_property_value(element, &IS_PART_OF_SEL, "isPartOf", true)?;
    let article_body = get_text_descendents(element, &ARTICLE_BODY_SEL).join("");

    Ok(Posting {
        identifier: identifier.to_string(),
        position,
        comment_count,
        date_created,
        date_published,
        url: url.to_string(),
        author,
        is_part_of: is_part_of.map(|value| value.to_string()),
        interaction_counters: Vec::new(),
        article_body,
    })
}

fn parse_person(element: &ElementRef) -> Result<Person, Error> {
    let identifier = get_property_value(element, &IDENTIFIER_SEL, "identifier", true)?;
    let additional_name =
        get_property_value(element, &ADDITIONAL_NAME_SEL, "additionalName", true)?;
    let given_name = get_property_value(element, &GIVEN_NAME_SEL, "givenName", true)?;

    Ok(Person {
        identifier: identifier.to_string(),
        additional_name: additional_name.to_string(),
        given_name: given_name.to_string(),
    })
}

fn get_child<'a>(
    element: &'a ElementRef,
    selector: &Selector,
    name: &str,
) -> Result<ElementRef<'a>, Error> {
    let mut selected = element.select(selector);
    let first = selected
        .next()
        .ok_or_else(|| Error::MissingChild(name.to_string()))?;

    if selected.next().is_none() {
        Ok(first)
    } else {
        Err(Error::DuplicateChild(name.to_string()))
    }
}

fn get_text_descendents<'a>(element: &'a ElementRef, selector: &Selector) -> Vec<&'a str> {
    element
        .select(selector)
        .flat_map(|element| element.text())
        .filter(|value| value != &"…")
        .collect()
}

fn get_property_value<'a>(
    element: &'a ElementRef,
    selector: &Selector,
    name: &str,
    unique: bool,
) -> Result<&'a str, Error> {
    let mut selected = element.select(selector);
    let first = selected
        .next()
        .ok_or_else(|| Error::MissingProperty(name.to_string()))?;

    let content = first
        .value()
        .attr("content")
        .ok_or_else(|| Error::InvalidProperty(name.to_string()))?;

    if !unique || selected.next().is_none() {
        Ok(content)
    } else {
        Err(Error::DuplicateProperty(name.to_string()))
    }
}

fn get_optional_property_value<'a>(
    element: &'a ElementRef,
    selector: &Selector,
    name: &str,
    unique: bool,
) -> Result<Option<&'a str>, Error> {
    let mut selected = element.select(selector);
    if let Some(first) = selected.next() {
        let content = first
            .value()
            .attr("content")
            .ok_or_else(|| Error::InvalidProperty(name.to_string()))?;

        if !unique || selected.next().is_none() {
            Ok(Some(content))
        } else {
            Err(Error::DuplicateProperty(name.to_string()))
        }
    } else {
        Ok(None)
    }
}

fn get_property_value_u32<'a>(
    element: &'a ElementRef,
    selector: &Selector,
    name: &str,
) -> Result<u32, Error> {
    let content = get_property_value(element, selector, name, true)?;

    content
        .parse::<u32>()
        .map_err(|_| Error::InvalidProperty(name.to_string()))
}

fn get_property_value_date_time<'a>(
    element: &'a ElementRef,
    selector: &Selector,
    name: &str,
) -> Result<DateTime<Utc>, Error> {
    let content = get_property_value(element, selector, name, true)?;

    Ok(Utc.from_utc_datetime(
        &NaiveDateTime::parse_from_str(content, DATE_TIME_FORMAT)
            .map_err(|_| Error::InvalidProperty(name.to_string()))?,
    ))
}

#[cfg(test)]
mod tests {
    use html5ever::driver::ParseOpts;
    use html5ever::tendril::TendrilSink;
    use scraper::Html;
    use std::fs::File;
    use std::io::Read;

    fn extract<R: Read>(input: &mut R) -> Result<Vec<super::Posting>, super::Error> {
        let parser = html5ever::driver::parse_document(Html::new_document(), ParseOpts::default())
            .from_utf8();

        let doc = parser.read_from(input)?;

        Ok(super::extract_postings(&doc)?.unwrap_or_default())
    }

    #[test]
    fn parse_postings() {
        let mut file = File::open("examples/wayback/1529393316344758279.html").unwrap();
        let postings = extract(&mut file).unwrap();
        assert_eq!(postings.len(), 11);

        let last = postings.last().unwrap();
        let expected = "If you keep doing this I will block you (again).";

        assert_eq!(last.article_body, expected);
    }
}
