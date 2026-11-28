//! Module `library`: the library manager's routes and lifecycle (engine: `crate::library`). Starting it seeds a standard quality profile per media kind on first use and starts the queue and job loops; stopping it ends them.
use crate::api::{ApiErr, ApiResult, ok, saved};
use crate::library::Engine;
use crate::module::{ApiRouter, BoxFut, Module};
use crate::state::Shared;
use axum::Json;
use axum::extract::{Path, Query, State};
use nocturne_arr_types::{Caps, IndexerDef, MediaKind, Release};
use nocturne_core::server::{
    BlockEntry, Candidate, HistoryEntry, Job, JobInfo, Library as Lib, NewTitle, Profile,
    QueueItem, Title, TitleDetail,
};
use serde::Deserialize;
use serde_json::Value;
use std::sync::{Arc, Mutex, RwLock};

/// The running engine, for other modules (requests): `sh.ext::<LibSlot>()`.
#[derive(Default)]
pub struct LibSlot(pub RwLock<Option<Arc<Engine>>>);

impl LibSlot {
    pub fn get(&self) -> Result<Arc<Engine>, ApiErr> {
        self.0.read().unwrap().clone().ok_or_else(|| {
            ApiErr(
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "the library module is off".into(),
            )
        })
    }
}

pub struct Library {
    engine: Arc<Engine>,
    stop: Mutex<Option<tokio::sync::watch::Sender<bool>>>,
}

impl Library {
    pub fn new(engine: Arc<Engine>) -> Self {
        Self {
            engine,
            stop: Mutex::default(),
        }
    }
}

impl Module for Library {
    fn name(&self) -> &'static str {
        "library"
    }

    fn routes(&self, r: ApiRouter) -> ApiRouter {
        r.get("/libraries", libraries)
            .post("/libraries", add_library)
            .put("/libraries/{id}", put_library)
            .delete("/libraries/{id}", delete_library)
            .get("/profiles", profiles)
            .post("/profiles", add_profile)
            .put("/profiles/{id}", put_profile)
            .delete("/profiles/{id}", delete_profile)
            .get("/indexers", indexers)
            .post("/indexers", add_indexer)
            .put("/indexers/{id}", put_indexer)
            .delete("/indexers/{id}", delete_indexer)
            .post("/indexers/{id}/test", test_indexer)
            .get("/titles", titles)
            .post("/titles", add_title)
            .get("/titles/{id}", title)
            .delete("/titles/{id}", remove_title)
            .post("/titles/{id}/refresh", refresh)
            .put("/titles/{id}/monitor", monitor)
            .post("/titles/{id}/search", search)
            .post("/titles/{id}/releases", releases)
            .post("/titles/{id}/grab", grab)
            .get("/queue", queue)
            .delete("/queue/{id}", remove_grab)
            .post("/queue/{id}/retry", retry)
            .post("/queue/{id}/pause", pause)
            .post("/queue/{id}/resume", resume)
            .get("/history", history)
            .get("/blocklist", blocklist)
            .delete("/blocklist/{id}", unblock)
            .get("/jobs", jobs)
            .post("/jobs/{job}/run", run_job)
    }

    fn start<'a>(&'a self, sh: &'a Shared) -> BoxFut<'a, Result<(), String>> {
        Box::pin(async move {
            if sh.db.all::<Profile>()?.is_empty() {
                let mut ps: Vec<Profile> = [
                    MediaKind::Movie,
                    MediaKind::Series,
                    MediaKind::Music,
                    MediaKind::Book,
                ]
                .into_iter()
                .map(Profile::standard)
                .collect();
                sh.db.put_all(&mut ps)?;
            }
            let (tx, rx) = tokio::sync::watch::channel(false);
            *self.stop.lock().unwrap() = Some(tx);
            *sh.ext::<LibSlot>().0.write().unwrap() = Some(self.engine.clone());
            *sh.ext::<crate::api::JobSlot>().0.lock().unwrap() = self.engine.jobs(sh);
            self.engine.spawn_loops(sh, rx);
            Ok(())
        })
    }

    fn stop<'a>(&'a self, sh: &'a Shared) -> BoxFut<'a, ()> {
        Box::pin(async move {
            if let Some(tx) = self.stop.lock().unwrap().take() {
                tx.send_replace(true);
            }
            *sh.ext::<LibSlot>().0.write().unwrap() = None;
        })
    }

    /// Its section has only `enabled`: nothing to reload.
    fn reload<'a>(&'a self, _: &'a Shared) -> BoxFut<'a, Result<(), String>> {
        Box::pin(async { Ok(()) })
    }
}

fn eng(sh: &Shared) -> Result<Arc<Engine>, ApiErr> {
    sh.ext::<LibSlot>().get()
}

async fn libraries(State(sh): State<Shared>) -> ApiResult<Vec<Lib>> {
    Ok(Json(sh.db.all()?))
}

async fn add_library(State(sh): State<Shared>, Json(mut l): Json<Lib>) -> ApiResult<Value> {
    l.id = 0;
    saved(Engine::save_library(&sh, l)?)
}

async fn put_library(
    State(sh): State<Shared>,
    Path(id): Path<u64>,
    Json(mut l): Json<Lib>,
) -> ApiResult<Value> {
    l.id = id;
    saved(Engine::save_library(&sh, l)?)
}

async fn delete_library(State(sh): State<Shared>, Path(id): Path<u64>) -> ApiResult<Value> {
    Engine::delete_library(&sh, id).map_err(ApiErr::conflict)?;
    ok()
}

async fn profiles(State(sh): State<Shared>) -> ApiResult<Vec<Profile>> {
    Ok(Json(sh.db.all()?))
}

async fn add_profile(State(sh): State<Shared>, Json(mut p): Json<Profile>) -> ApiResult<Value> {
    p.id = 0;
    saved(Engine::save_profile(&sh, p)?)
}

async fn put_profile(
    State(sh): State<Shared>,
    Path(id): Path<u64>,
    Json(mut p): Json<Profile>,
) -> ApiResult<Value> {
    p.id = id;
    saved(Engine::save_profile(&sh, p)?)
}

async fn delete_profile(State(sh): State<Shared>, Path(id): Path<u64>) -> ApiResult<Value> {
    Engine::delete_profile(&sh, id).map_err(ApiErr::conflict)?;
    ok()
}

#[derive(Deserialize)]
struct IndexerSave {
    def: IndexerDef,
    #[serde(default)]
    api_key: Option<String>,
}

async fn indexers(State(sh): State<Shared>) -> ApiResult<Vec<IndexerDef>> {
    Ok(Json(sh.db.all()?))
}

async fn add_indexer(State(sh): State<Shared>, Json(mut s): Json<IndexerSave>) -> ApiResult<Value> {
    s.def.id = 0;
    saved(Engine::save_indexer(&sh, s.def, s.api_key.as_deref())?)
}

async fn put_indexer(
    State(sh): State<Shared>,
    Path(id): Path<u64>,
    Json(mut s): Json<IndexerSave>,
) -> ApiResult<Value> {
    s.def.id = id;
    saved(Engine::save_indexer(&sh, s.def, s.api_key.as_deref())?)
}

async fn delete_indexer(State(sh): State<Shared>, Path(id): Path<u64>) -> ApiResult<Value> {
    Engine::delete_indexer(&sh, id)?;
    ok()
}

async fn test_indexer(State(sh): State<Shared>, Path(id): Path<u64>) -> ApiResult<Caps> {
    Ok(Json(eng(&sh)?.test_indexer(&sh, id).await?))
}

#[derive(Deserialize)]
struct KindQ {
    kind: Option<MediaKind>,
}

async fn titles(State(sh): State<Shared>, Query(q): Query<KindQ>) -> ApiResult<Vec<Title>> {
    let all: Vec<Title> = sh.db.all()?;
    Ok(Json(
        all.into_iter()
            .filter(|t| q.kind.is_none_or(|k| t.kind == k))
            .collect(),
    ))
}

async fn add_title(State(sh): State<Shared>, Json(n): Json<NewTitle>) -> ApiResult<Value> {
    let id = eng(&sh)?.add_title(&sh, n).await.map_err(|e| {
        if e.starts_with("already monitored") {
            ApiErr::conflict(e)
        } else {
            ApiErr::bad(e)
        }
    })?;
    saved(id)
}

async fn title(State(sh): State<Shared>, Path(id): Path<u64>) -> ApiResult<TitleDetail> {
    Engine::detail(&sh, id)?
        .map(Json)
        .ok_or_else(ApiErr::not_found)
}

async fn remove_title(State(sh): State<Shared>, Path(id): Path<u64>) -> ApiResult<Value> {
    eng(&sh)?.remove_title(&sh, id).await?;
    ok()
}

async fn refresh(State(sh): State<Shared>, Path(id): Path<u64>) -> ApiResult<Value> {
    eng(&sh)?.refresh_title(&sh, id).await?;
    ok()
}

#[derive(Deserialize)]
struct MonitorBody {
    #[serde(default)]
    units: Vec<u64>,
    monitored: bool,
}

async fn monitor(
    State(sh): State<Shared>,
    Path(id): Path<u64>,
    Json(b): Json<MonitorBody>,
) -> ApiResult<Value> {
    Engine::monitor(&sh, id, &b.units, b.monitored)?;
    ok()
}

#[derive(Deserialize, Default)]
struct Units {
    #[serde(default)]
    units: Vec<u64>,
}

fn units(b: Option<Json<Units>>) -> Vec<u64> {
    b.map(|Json(u)| u.units).unwrap_or_default()
}

async fn search(
    State(sh): State<Shared>,
    Path(id): Path<u64>,
    b: Option<Json<Units>>,
) -> ApiResult<Value> {
    let e = eng(&sh)?;
    sh.db.get::<Title>(id)?.ok_or_else(ApiErr::not_found)?;
    let (sh2, u) = (sh.clone(), units(b));
    tokio::spawn(async move {
        if let Err(err) = e.search(&sh2, id, u).await {
            eprintln!("nocturne-server: search: {err}");
        }
    });
    ok()
}

async fn releases(
    State(sh): State<Shared>,
    Path(id): Path<u64>,
    b: Option<Json<Units>>,
) -> ApiResult<Vec<Candidate>> {
    Ok(Json(eng(&sh)?.releases(&sh, id, &units(b)).await?))
}

async fn grab(
    State(sh): State<Shared>,
    Path(id): Path<u64>,
    Json(r): Json<Release>,
) -> ApiResult<Value> {
    saved(eng(&sh)?.grab_manual(&sh, id, r).await?)
}

async fn queue(State(sh): State<Shared>) -> ApiResult<Vec<QueueItem>> {
    Ok(Json(Engine::queue_live(&sh).await?))
}

#[derive(Deserialize)]
struct RemoveQ {
    #[serde(default)]
    blocklist: bool,
    #[serde(default)]
    search: bool,
}

async fn remove_grab(
    State(sh): State<Shared>,
    Path(id): Path<u64>,
    Query(q): Query<RemoveQ>,
) -> ApiResult<Value> {
    eng(&sh)?
        .remove_grab(&sh, id, q.blocklist, q.search)
        .await?;
    ok()
}

async fn retry(State(sh): State<Shared>, Path(id): Path<u64>) -> ApiResult<Value> {
    eng(&sh)?.retry_import(&sh, id).await?;
    ok()
}

async fn pause(State(sh): State<Shared>, Path(id): Path<u64>) -> ApiResult<Value> {
    Engine::pause(&sh, id, true).await?;
    ok()
}

async fn resume(State(sh): State<Shared>, Path(id): Path<u64>) -> ApiResult<Value> {
    Engine::pause(&sh, id, false).await?;
    ok()
}

#[derive(Deserialize)]
struct HistoryQ {
    title: Option<u64>,
    limit: Option<usize>,
}

async fn history(
    State(sh): State<Shared>,
    Query(q): Query<HistoryQ>,
) -> ApiResult<Vec<HistoryEntry>> {
    let mut h: Vec<HistoryEntry> = sh.db.all()?;
    h.retain(|e| q.title.is_none_or(|t| e.title == t));
    h.sort_by_key(|e| std::cmp::Reverse(e.id));
    h.truncate(q.limit.unwrap_or(100));
    Ok(Json(h))
}

async fn blocklist(State(sh): State<Shared>) -> ApiResult<Vec<BlockEntry>> {
    Ok(Json(sh.db.all()?))
}

async fn unblock(State(sh): State<Shared>, Path(id): Path<u64>) -> ApiResult<Value> {
    sh.db.delete::<BlockEntry>(&[id])?;
    ok()
}

async fn jobs(State(sh): State<Shared>) -> ApiResult<Vec<JobInfo>> {
    Ok(Json(eng(&sh)?.jobs(&sh)))
}

async fn run_job(State(sh): State<Shared>, Path(job): Path<Job>) -> ApiResult<Value> {
    let e = eng(&sh)?;
    tokio::spawn(async move {
        let _ = e.run_job(&sh, job).await;
    });
    ok()
}
