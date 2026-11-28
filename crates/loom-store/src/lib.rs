//! `loom-store`: PostgreSQL through `sqlx`, with migrations by `sqlx migrate`.
//!
//! Holds the raw run streams (T7) and the downloaded price list (T14).

mod error;
mod migrate;
mod price_list;
mod raw_stream;
pub mod testing;

pub use error::StoreError;
pub use migrate::migrate;
pub use price_list::PgPriceListStore;
pub use raw_stream::{PgRawStreamStore, prune_raw_streams};
pub use sqlx::PgPool;
