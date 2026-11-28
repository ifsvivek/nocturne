//! The modules of this build, in start order. One line per module (see `crate::module` for how to add one).
use crate::module::Module;
use std::sync::Arc;

pub fn all() -> Vec<Arc<dyn Module>> {
    vec![]
}
