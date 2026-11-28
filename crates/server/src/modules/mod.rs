//! The modules of this build, in start order. One line per module (see `crate::module` for how to add one).
pub mod storage;
pub mod storage_agent;

use crate::module::Module;
use std::sync::Arc;

pub fn all() -> Vec<Arc<dyn Module>> {
    vec![Arc::new(storage::Storage::default())]
}
