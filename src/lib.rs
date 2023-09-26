pub extern crate multiaddr;

pub mod key;
pub mod node;
pub mod pool;
pub mod query;
pub mod republish;
pub mod routing;
pub mod store;
pub mod transport;

pub const K_VALUE: usize = 20;

pub const ALPHA_VALUE: usize = 3;

pub const MAX_QUERIES: usize = 100;

pub const JOB_MAX_NEW_QUERIES: usize = 10;

pub const MAX_RECORD_SIZE: usize = 65 * 1024;

pub const MAX_RECORDS: usize = 1024;
