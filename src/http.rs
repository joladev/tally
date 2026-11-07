use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::Arc;

use axum::Router;
use axum::extract::{Form, Path, Query, State};
use axum::http::StatusCode;
use axum::http::header::{ACCESS_CONTROL_ALLOW_ORIGIN, CACHE_CONTROL};
use axum::response::{Html, IntoResponse, Json, Redirect, Response};
use axum::routing::get;
use serde::{Deserialize, Serialize};

use crate::counts::Counts;
use crate::store::{Period, Store};
use crate::top::Top;
use crate::zones::{SaveError, Zone, Zones};

const PAGE: &str = include_str!("page.html");

const SHOWN: usize = 20;

const PUBLIC_CACHE: &str = "public, max-age=60";

const MIN_SHOWN_PARAM_COUNT: u64 = 5;

#[derive(Clone)]
struct App {
    store: Arc<Store>,
    zones: Arc<Zones>,
}

pub fn internal_router(store: Arc<Store>, zones: Arc<Zones>) -> Router {
    Router::new()
        .route("/", get(index).post(add_zone))
        .route("/{zone}", get(page))
        .route("/{zone}/stats.json", get(stats))
        .route("/{zone}/settings", get(settings).post(save_settings))
        .with_state(App { store, zones })
}

pub fn public_router(store: Arc<Store>, zones: Arc<Zones>) -> Router {
    Router::new()
        .route("/{zone}/stats.json", get(public_stats))
        .with_state(App { store, zones })
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Range {
    Day,
    #[default]
    Week,
    Month,
    Year,
    All,
}

impl Range {
    fn rows(self, now: i64) -> (Period, i64) {
        let hours_back = |hours: i64| Period::Hour.start(now) - hours * 3_600;
        let days_back = |days: i64| Period::Day.start(now) - days * 86_400;
        match self {
            Range::Day => (Period::Hour, hours_back(23)),
            Range::Week => (Period::Hour, hours_back(7 * 24 - 1)),
            Range::Month => (Period::Day, days_back(29)),
            Range::Year => (Period::Day, days_back(364)),
            Range::All => (Period::Month, 0),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StatsQuery {
    #[serde(default)]
    range: Range,
}

#[derive(Serialize)]
struct Stats {
    zone: String,
    period: Period,
    series: Vec<(i64, u64)>,
    requests: u64,
    statuses: BTreeMap<String, u64>,
    cache: BTreeMap<String, u64>,
    countries: BTreeMap<String, u64>,
    clients: Vec<(String, u64)>,
    referrers: Vec<(String, u64)>,
    paths: Vec<(String, u64)>,
    params: BTreeMap<String, Vec<(String, u64)>>,
}

#[derive(Deserialize)]
struct NewZone {
    id: u64,
    name: String,
    token: String,
}

async fn index(State(app): State<App>) -> Html<String> {
    let mut links = String::new();
    for zone in app.zones.all() {
        let name = escape(&zone.name);
        let _ = write!(links, r#"<li><a href="/{name}">{name}</a></li>"#);
    }
    Html(format!(
        r#"<!doctype html><meta charset=utf-8><title>tally</title>
<h1>tally</h1>
<ul>{links}</ul>
<h2>Add a zone</h2>
<form method=post>
<label>Pull zone ID <input name=id required inputmode=numeric></label>
<label>Name <input name=name required></label>
<label>Token <input name=token required autocomplete=off></label>
<button>Add</button>
</form>
<p>Set the same token in the zone's log forwarding settings in Bunny.</p>"#
    ))
}

async fn add_zone(State(app): State<App>, Form(new): Form<NewZone>) -> Response {
    let page = Redirect::to(&format!("/{}", new.name)).into_response();
    let zone = Zone {
        id: new.id,
        name: new.name,
        token: new.token,
        ..Zone::default()
    };
    saved(app.zones.add(zone), page)
}

async fn page(State(app): State<App>, Path(zone): Path<String>) -> Response {
    match app.zones.find(&zone) {
        Some(_) => Html(PAGE).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn stats(
    State(app): State<App>,
    Path(zone): Path<String>,
    Query(query): Query<StatsQuery>,
) -> Response {
    match app.zones.find(&zone) {
        Some(zone) => zone_stats(&app, zone, query.range),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn settings(State(app): State<App>, Path(zone): Path<String>) -> Response {
    match app.zones.find(&zone) {
        Some(zone) => Json(zone).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn save_settings(
    State(app): State<App>,
    Path(zone): Path<String>,
    Json(settings): Json<Zone>,
) -> Response {
    let Some(zone) = app.zones.find(&zone) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let zone = Zone {
        id: zone.id,
        ..settings
    };
    saved(app.zones.save(zone), StatusCode::NO_CONTENT.into_response())
}

fn saved(result: Result<(), SaveError>, success: Response) -> Response {
    match result {
        Ok(()) => success,
        Err(SaveError::Invalid(problem)) => (StatusCode::BAD_REQUEST, problem).into_response(),
        Err(SaveError::Failed(err)) => failed(&err),
    }
}

async fn public_stats(
    State(app): State<App>,
    Path(zone): Path<String>,
    Query(query): Query<StatsQuery>,
) -> Response {
    match app.zones.find(&zone) {
        Some(zone) if zone.public => {
            let headers = [
                (ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
                (CACHE_CONTROL, PUBLIC_CACHE),
            ];
            (headers, zone_stats(&app, zone, query.range)).into_response()
        }
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

fn zone_stats(app: &App, zone: Zone, range: Range) -> Response {
    let now = jiff::Timestamp::now().as_second();
    let (period, from) = range.rows(now);
    let rows = match app.store.rows(zone.id, period, from) {
        Ok(rows) => rows,
        Err(err) => return failed(&err),
    };

    let mut total = Counts::default();
    for (_, row) in &rows {
        total.merge(row);
    }
    let first = rows
        .first()
        .map_or(Period::Hour.start(now), |(start, _)| *start);
    let start = if from == 0 { first } else { from };
    Json(Stats {
        zone: zone.name,
        period,
        series: series(&rows, period, start, now),
        requests: total.requests,
        statuses: total.statuses,
        cache: total.cache,
        countries: total.countries,
        clients: shown(&total.clients),
        referrers: shown(&total.referrers),
        paths: shown(&total.paths),
        params: total
            .params
            .iter()
            .map(|(name, top)| (name.clone(), shown_params(top)))
            .collect(),
    })
    .into_response()
}

fn series(rows: &[(i64, Counts)], period: Period, from: i64, now: i64) -> Vec<(i64, u64)> {
    let mut rows = rows.iter().peekable();
    let mut series = Vec::new();
    let mut start = period.start(from);
    while start <= now {
        let requests = rows
            .next_if(|(row_start, _)| *row_start == start)
            .map_or(0, |(_, row)| row.requests);
        series.push((start, requests));
        start = period.next(start);
    }
    series
}

fn shown(top: &Top) -> Vec<(String, u64)> {
    let ranked = top.ranked().into_iter().take(SHOWN);
    ranked
        .map(|(value, count)| (value.to_owned(), count.count))
        .collect()
}

fn shown_params(top: &Top) -> Vec<(String, u64)> {
    let ranked = top.ranked();
    let common: Vec<_> = ranked
        .iter()
        .filter(|(_, count)| count.count >= MIN_SHOWN_PARAM_COUNT)
        .collect();
    let complete = common.iter().filter(|(value, _)| {
        !common
            .iter()
            .any(|(other, _)| other.len() > value.len() && other.starts_with(value))
    });
    let shown = complete.take(SHOWN);
    shown
        .map(|(value, count)| ((*value).to_owned(), count.count))
        .collect()
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('"', "&quot;")
}

fn failed(err: &anyhow::Error) -> Response {
    tracing::error!("store operation failed: {err:#}");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shown_params_leave_out_rare_values_and_unfinished_words() {
        let mut top = Top::default();
        for (value, times) in [("j", 9), ("ja", 8), ("jay", 7), ("paul", 6), ("private", 1)] {
            for _ in 0..times {
                top.add(value);
            }
        }

        let shown: Vec<_> = shown_params(&top)
            .into_iter()
            .map(|(value, _)| value)
            .collect();

        assert_eq!(shown, ["jay", "paul"]);
    }
}
