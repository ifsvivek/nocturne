//! The modules of this build, in start order. One line per module (see `crate::module` for how to add one).
use crate::module::Module;
use std::sync::Arc;

pub mod discovery;
pub mod gateway;
pub mod sources;
pub mod tunnel;

pub fn all() -> Vec<Arc<dyn Module>> {
    vec![
        Arc::new(sources::Sources::default()),
        Arc::new(gateway::Gateway::default()),
        Arc::new(tunnel::Tunnel),
        Arc::new(discovery::Discovery),
    ]
}
