//! Module `requests`: the native request list. A request names a title; approved (at once with `auto_approve`, else by an admin) it becomes a monitored title of the library module, and its status follows that title: downloading while a grab is active, available once every monitored, released unit has a file. Seerr and external Radarr / Sonarr connections stay in the app for now (follow-up in 0.7.5).
use crate::api::{ApiErr, ApiResult, ok, saved};
use crate::auth::Caller;
use crate::decide;
use crate::library::{Engine, today};
use crate::module::{ApiRouter, BoxFut, Module};
use crate::modules::library::LibSlot;
use crate::state::{Shared, now};
use axum::extract::{Path, State};
use axum::{Extension, Json};
use nocturne_core::server::{
    Event, Grab, GrabState, Library, MediaRequest, Monitor, NewRequest, NewTitle, Profile,
    RequestStatus, Title,
};
use serde_json::Value;

pub struct Requests;

impl Module for Requests {
    fn name(&self) -> &'static str {
        "requests"
    }

    fn routes(&self, r: ApiRouter) -> ApiRouter {
        r.get("", list)
            .post("", create)
            .delete("/{id}", cancel)
            .post("/{id}/approve", approve)
    }

    fn start<'a>(&'a self, _: &'a Shared) -> BoxFut<'a, Result<(), String>> {
        Box::pin(async { Ok(()) })
    }

    fn stop<'a>(&'a self, _: &'a Shared) -> BoxFut<'a, ()> {
        Box::pin(async {})
    }
}

/// The status a request shows: its own for pending or declined ones, else what its title says.
pub fn status(sh: &Shared, r: &MediaRequest) -> RequestStatus {
    let Some(t) = r.title else { return r.status };
    let Ok(Some(title)) = sh.db.get::<Title>(t) else {
        return r.status;
    };
    let grabs = sh.db.all::<Grab>().unwrap_or_default();
    if grabs
        .iter()
        .any(|g| g.title == t && g.state == GrabState::Downloading)
    {
        return RequestStatus::Downloading;
    }
    let units = Engine::units_of(sh, t).unwrap_or_default();
    let today = today();
    let wanted: Vec<_> = units
        .iter()
        .filter(|u| u.monitored && decide::released(u, &today))
        .collect();
    if !wanted.is_empty()
        && wanted
            .iter()
            .all(|u| u.file.is_some() || crate::library::on_source(sh, &title, u))
    {
        RequestStatus::Available
    } else {
        RequestStatus::Approved
    }
}

async fn list(State(sh): State<Shared>) -> ApiResult<Vec<MediaRequest>> {
    let mut all: Vec<MediaRequest> = sh.db.all()?;
    for r in &mut all {
        r.status = status(&sh, r);
    }
    all.sort_by_key(|r| std::cmp::Reverse(r.id));
    Ok(Json(all))
}

/// Make the request's title in the library module.
async fn to_title(
    sh: &Shared,
    r: &mut MediaRequest,
    lib: Option<u64>,
    prof: Option<u64>,
) -> Result<(), ApiErr> {
    let e = sh.ext::<LibSlot>().get()?;
    let lib = match lib {
        Some(l) => l,
        None => {
            sh.db
                .all::<Library>()?
                .into_iter()
                .find(|l| l.kind == r.kind)
                .ok_or_else(|| {
                    ApiErr::conflict("add a library folder for this kind of media first")
                })?
                .id
        }
    };
    let prof = match prof {
        Some(p) => p,
        None => {
            sh.db
                .all::<Profile>()?
                .into_iter()
                .find(|p| p.kind == r.kind)
                .ok_or_else(|| {
                    ApiErr::conflict("add a quality profile for this kind of media first")
                })?
                .id
        }
    };
    let monitor = if r.seasons.is_empty() {
        Monitor::All
    } else {
        Monitor::Seasons(r.seasons.clone())
    };
    let id = e
        .add_title(
            sh,
            NewTitle {
                kind: r.kind,
                name: r.name.clone(),
                year: r.year,
                ids: r.ids.clone(),
                profile: prof,
                library: lib,
                monitor,
                search: true,
            },
        )
        .await;
    let id = match id {
        Ok(id) => id,
        // Already monitored: link the request to that title.
        Err(m) if m.starts_with("already monitored (title ") => m
            .trim_start_matches("already monitored (title ")
            .trim_end_matches(')')
            .parse()
            .map_err(|_| ApiErr::bad(m.clone()))?,
        Err(m) => return Err(ApiErr::bad(m)),
    };
    r.title = Some(id);
    r.status = RequestStatus::Approved;
    Ok(())
}

async fn create(
    State(sh): State<Shared>,
    Extension(c): Extension<Caller>,
    Json(n): Json<NewRequest>,
) -> ApiResult<Value> {
    if n.name.trim().is_empty() {
        return Err(ApiErr::bad("name the title"));
    }
    let mut r = MediaRequest {
        id: 0,
        kind: n.kind,
        name: n.name.trim().into(),
        year: n.year,
        ids: n.ids,
        seasons: n.seasons,
        requested_by: c.name().into(),
        created: now(),
        status: RequestStatus::Pending,
        title: None,
    };
    if sh.cfg().requests.auto_approve {
        to_title(&sh, &mut r, n.library, n.profile).await?;
    }
    let id = sh.db.put(&mut r)?;
    sh.publish(Event::Request(id));
    saved(id)
}

async fn cancel(State(sh): State<Shared>, Path(id): Path<u64>) -> ApiResult<Value> {
    sh.db
        .get::<MediaRequest>(id)?
        .ok_or_else(ApiErr::not_found)?;
    sh.db.delete::<MediaRequest>(&[id])?;
    sh.publish(Event::Request(id));
    ok()
}

async fn approve(
    State(sh): State<Shared>,
    Extension(c): Extension<Caller>,
    Path(id): Path<u64>,
) -> ApiResult<Value> {
    if !c.admin() {
        return Err(ApiErr::forbidden("only an admin approves requests"));
    }
    let mut r = sh
        .db
        .get::<MediaRequest>(id)?
        .ok_or_else(ApiErr::not_found)?;
    if r.title.is_none() {
        to_title(&sh, &mut r, None, None).await?;
        sh.db.put(&mut r)?;
        sh.publish(Event::Request(id));
    }
    ok()
}
