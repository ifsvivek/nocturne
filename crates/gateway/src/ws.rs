//! the /socket WebSocket (worker C). Owned by the worker named above; worker A only provides this seam.
//! Add routes with `r.add(path, Access::..., get(handler))` (or `add_stream` for media and WebSocket routes).
use crate::server::Routes;

pub fn routes(r: Routes) -> Routes {
    r
}
