pub mod cdx;
mod error;
mod store;
pub mod web;

pub use error::Error;
pub use store::Store;

pub type Result<T> = std::result::Result<T, Error>;
