pub mod deleted_tweets;

use serde::Serialize;
use tinytemplate::{error::Result, TinyTemplate};

pub trait Report: Serialize {
    fn title() -> &'static str;
    fn template() -> &'static str;

    fn generate(&self) -> Result<String> {
        let mut report = TinyTemplate::new();
        report.add_template("report", Self::template())?;
        report.render("report", &self)
    }

    fn render(&self) -> String {
        self.generate()
            .unwrap_or_else(|err| panic!("Cannot render {}: {}", Self::title(), err))
    }
}
