use std::collections::HashMap;
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::Ordering;
use std::time::Duration;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{ConnectInfo, Query, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::Json;
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio_stream::wrappers::WatchStream;
use tower_http::compression::CompressionLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;
use tracing::{debug, warn};

use crate::config::StopConfig;
use crate::geo::{LatLon, Route};
use crate::routing;
use crate::state::{AppState, now_ms};
use crate::tracker::{
    AlertAction, AlertRequest, Fix, Ingest, Message, MessageLevel, PickupAction, PickupRequest, ServiceMode,
};
use crate::trains::Kind;

pub fn router(state: AppState) -> Router {
    let dir = &state.cfg.server.static_dir;
    let static_dir = ServeDir::new(dir);
    Router::new()
        // Adresses propres pour l'équipage ; les anciens liens en .html (dont le
        // lien chauffeur avec son jeton après le #) y sont redirigés.
        .route_service("/driver", ServeFile::new(dir.join("driver.html")))
        .route_service("/admin", ServeFile::new(dir.join("admin.html")))
        .route("/driver.html", get(|| async { Redirect::permanent("/driver") }))
        .route("/admin.html", get(|| async { Redirect::permanent("/admin") }))
        .route("/api/config", get(get_config))
        .route("/api/state", get(get_state))
        .route("/api/stream", get(stream))
        .route("/api/trains", get(get_trains))
        .route("/api/trains/arrivals", get(get_arrivals))
        .route("/api/driver/position", post(driver_position))
        .route("/api/osmand", get(osmand).post(osmand))
        .route("/api/pickups", post(pickup_request))
        .route("/api/pickups/cancel", post(pickup_cancel))
        .route("/api/help", post(help_request))
        .route("/api/help/status", post(help_status))
        .route("/api/help/cancel", post(help_cancel))
        .route("/api/crew/state", get(crew_state))
        .route("/api/crew/passengers", post(crew_passengers))
        .route("/api/crew/depart", post(crew_depart))
        .route("/api/crew/pickup", post(crew_pickup))
        .route("/api/crew/help", post(crew_help))
        .route("/api/crew/service", post(crew_service))
        .route("/api/crew/message", post(crew_message))
        .route("/api/admin/route", get(admin_route).post(admin_route_apply))
        .route("/api/admin/route/preview", post(admin_route_preview))
        .route("/api/admin/route/reset", post(admin_route_reset))
        .with_state(state)
        .fallback_service(static_dir)
        // Le flux SSE est exclu d'office par le prédicat par défaut.
        .layer(CompressionLayer::new())
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-cache"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
}

// ---------------------------------------------------------------- public

async fn get_config(State(st): State<AppState>) -> Response {
    (
        [
            (header::CONTENT_TYPE, "application/json"),
            // Le tracé peut changer (page orga) : jamais de copie périmée.
            (header::CACHE_CONTROL, "no-cache"),
        ],
        st.config_json().to_string(),
    )
        .into_response()
}

async fn get_state(State(st): State<AppState>) -> Response {
    (
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        st.snapshot_json(),
    )
        .into_response()
}

/// Prochains trains au départ de la gare (mis en cache par le serveur).
async fn get_trains(State(st): State<AppState>) -> Response {
    trains_board(&st, Kind::Departures).await
}

/// Prochains trains à l'arrivée en gare, pour l'équipage : combien de temps
/// attendre les festivaliers du prochain train.
async fn get_arrivals(State(st): State<AppState>) -> Response {
    trains_board(&st, Kind::Arrivals).await
}

async fn trains_board(st: &AppState, kind: Kind) -> Response {
    let Some(trains) = &st.trains else {
        return StatusCode::NOT_FOUND.into_response();
    };
    (
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, "public, max-age=30"),
        ],
        trains.board_json(kind).await,
    )
        .into_response()
}

/// Décrémente le compteur de spectateurs quand le client se déconnecte.
struct ViewerGuard(AppState);

impl Drop for ViewerGuard {
    fn drop(&mut self) {
        self.0.viewers.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Flux temps réel (Server-Sent Events). Chaque client reçoit l'état courant
/// puis chaque mise à jour ; la sérialisation est partagée entre tous.
async fn stream(State(st): State<AppState>) -> Response {
    if st.viewers.fetch_add(1, Ordering::Relaxed) >= st.cfg.server.max_viewers {
        st.viewers.fetch_sub(1, Ordering::Relaxed);
        return (StatusCode::SERVICE_UNAVAILABLE, "trop de connexions").into_response();
    }
    let guard = ViewerGuard(st.clone());
    let events = WatchStream::new(st.tx.subscribe()).map(move |json| {
        let _keep_alive = &guard;
        Ok::<_, Infallible>(Event::default().data(&*json))
    });
    (
        [(
            HeaderName::from_static("x-accel-buffering"),
            HeaderValue::from_static("no"),
        )],
        Sse::new(events).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))),
    )
        .into_response()
}

// ---------------------------------------------------------------- chauffeur

fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::trim)
}

fn authorized(headers: &HeaderMap, token: &str) -> bool {
    bearer(headers).is_some_and(|t| ct_eq(t, token))
}

fn unauthorized() -> Response {
    (StatusCode::UNAUTHORIZED, "jeton invalide").into_response()
}

fn ingest_response(result: Ingest) -> Response {
    match result {
        Ingest::Accepted => Json(json!({ "accepted": true })).into_response(),
        Ingest::Ignored(reason) => {
            debug!("position ignorée : {reason}");
            Json(json!({ "accepted": false, "reason": reason })).into_response()
        }
    }
}

#[derive(Deserialize)]
struct DriverFix {
    lat: f64,
    lon: f64,
    accuracy: Option<f64>,
    speed: Option<f64>,
    heading: Option<f64>,
    ts: Option<i64>,
    battery: Option<f64>,
}

/// Positions envoyées par la page `/driver` ouverte sur le téléphone du bus.
/// Le jeton orga est accepté aussi : son téléphone peut prendre le relais.
async fn driver_position(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<DriverFix>,
) -> Response {
    if !crew(&headers, &st) {
        warn!("position refusée : jeton chauffeur invalide");
        return unauthorized();
    }
    let fix = Fix {
        pos: LatLon { lat: body.lat, lon: body.lon },
        accuracy: body.accuracy,
        speed: body.speed,
        heading: body.heading,
        ts: body.ts.unwrap_or_else(now_ms),
        battery: body.battery,
    };
    ingest_response(st.ingest(fix))
}

/// Protocole OsmAnd, utilisé par l'application gratuite Traccar Client
/// (Android / iOS), qui continue d'envoyer en arrière-plan écran éteint.
/// Accepte l'ancien format (paramètres d'URL ou formulaire) et le nouveau (JSON).
async fn osmand(
    State(st): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
    body: Bytes,
) -> Response {
    let (device, fixes) = match parse_osmand(query, &body) {
        Ok(v) => v,
        Err(e) => return (StatusCode::BAD_REQUEST, e).into_response(),
    };
    if !ct_eq(&device, st.cfg.driver_token()) {
        warn!("position OsmAnd refusée : identifiant d'appareil inconnu");
        return (StatusCode::UNAUTHORIZED, "appareil inconnu").into_response();
    }
    for fix in fixes {
        if let Ingest::Ignored(reason) = st.ingest(fix) {
            debug!("position ignorée : {reason}");
        }
    }
    StatusCode::OK.into_response()
}

fn parse_osmand(
    mut params: HashMap<String, String>,
    body: &[u8],
) -> Result<(String, Vec<Fix>), &'static str> {
    let trimmed = body.trim_ascii();
    if trimmed.first() == Some(&b'{') {
        return parse_osmand_json(trimmed);
    }
    params.extend(form_urlencoded::parse(trimmed).into_owned());

    let num = |k: &str| params.get(k).and_then(|v| v.parse::<f64>().ok());
    let device = params
        .get("id")
        .or_else(|| params.get("deviceid"))
        .cloned()
        .ok_or("paramètre id manquant")?;
    let (Some(lat), Some(lon)) = (num("lat"), num("lon")) else {
        return Err("paramètres lat/lon manquants");
    };
    let fix = Fix {
        pos: LatLon { lat, lon },
        accuracy: num("accuracy"),
        // OsmAnd transmet la vitesse en nœuds.
        speed: num("speed").map(|knots| knots * 0.514_444),
        heading: num("bearing").or_else(|| num("heading")),
        ts: params
            .get("timestamp")
            .and_then(|t| parse_timestamp(t))
            .unwrap_or_else(now_ms),
        battery: num("batt"),
    };
    Ok((device, vec![fix]))
}

fn parse_osmand_json(body: &[u8]) -> Result<(String, Vec<Fix>), &'static str> {
    let v: Value = serde_json::from_slice(body).map_err(|_| "JSON invalide")?;
    let device = v
        .get("device_id")
        .and_then(Value::as_str)
        .ok_or("device_id manquant")?
        .to_owned();
    let locations = match v.get("location") {
        Some(Value::Array(list)) => list.iter().collect(),
        Some(loc) => vec![loc],
        None => return Err("location manquante"),
    };
    let fixes = locations
        .into_iter()
        .filter_map(|loc| {
            let coords = loc.get("coords")?;
            let f = |k: &str| coords.get(k).and_then(Value::as_f64);
            Some(Fix {
                pos: LatLon {
                    lat: f("latitude")?,
                    lon: f("longitude")?,
                },
                accuracy: f("accuracy"),
                speed: f("speed"),
                heading: f("heading"),
                ts: loc
                    .get("timestamp")
                    .and_then(Value::as_str)
                    .and_then(parse_timestamp)
                    .unwrap_or_else(now_ms),
                battery: loc
                    .pointer("/battery/level")
                    .and_then(Value::as_f64)
                    .map(|l| (l * 100.0).round()),
            })
        })
        .collect::<Vec<_>>();
    if fixes.is_empty() {
        return Err("aucune position exploitable");
    }
    Ok((device, fixes))
}

/// Secondes ou millisecondes Unix, RFC 3339, ou « AAAA-MM-JJ HH:MM:SS » (UTC).
fn parse_timestamp(raw: &str) -> Option<i64> {
    if let Ok(n) = raw.parse::<f64>() {
        return Some(if n > 1e12 { n as i64 } else { (n * 1000.0) as i64 });
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(raw) {
        return Some(dt.timestamp_millis());
    }
    chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S")
        .ok()
        .map(|dt| dt.and_utc().timestamp_millis())
}

// ---------------------------------------------------------------- « faire signe »

/// Adresse du visiteur, pour limiter les abus. Derrière Caddy (proxy local),
/// la vraie adresse est dans X-Forwarded-For ; un client qui se connecte
/// directement ne peut pas s'en servir pour tricher.
fn client_ip(headers: &HeaderMap, peer: SocketAddr) -> IpAddr {
    let peer_ip = peer.ip().to_canonical();
    let local = match peer_ip {
        IpAddr::V4(ip) => ip.is_loopback() || ip.is_private(),
        IpAddr::V6(ip) => ip.is_loopback() || ip.is_unique_local(),
    };
    if !local {
        return peer_ip;
    }
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit(',').next())
        .and_then(|ip| ip.trim().parse::<IpAddr>().ok())
        .map_or(peer_ip, |ip| ip.to_canonical())
}

#[derive(Deserialize)]
struct PickupBody {
    lat: f64,
    lon: f64,
    accuracy: Option<f64>,
    count: u32,
    dest: String,
    device: String,
}

/// Quelqu'un sur le trajet, loin d'un arrêt, demande à être pris en route.
async fn pickup_request(
    State(st): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<PickupBody>,
) -> Response {
    let device = body.device.trim();
    if device.is_empty() || device.len() > 64 {
        return (StatusCode::BAD_REQUEST, "identifiant d'appareil invalide").into_response();
    }
    let req = PickupRequest {
        pos: LatLon { lat: body.lat, lon: body.lon },
        accuracy: body.accuracy,
        count: body.count,
        dest: &body.dest,
        device,
        ip: client_ip(&headers, peer),
    };
    match st.request_pickup(req) {
        Ok(created) => (StatusCode::CREATED, Json(created)).into_response(),
        Err(error) => (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({ "error": error }))).into_response(),
    }
}

#[derive(Deserialize)]
struct CancelBody {
    id: String,
    token: String,
}

async fn pickup_cancel(State(st): State<AppState>, Json(body): Json<CancelBody>) -> Response {
    if st.cancel_pickup(&body.id, &body.token) {
        StatusCode::NO_CONTENT.into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

#[derive(Deserialize)]
struct HelpBody {
    lat: f64,
    lon: f64,
    accuracy: Option<f64>,
    #[serde(default)]
    note: String,
    device: String,
}

/// Quelqu'un en danger près du trajet prévient l'équipage.
async fn help_request(
    State(st): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<HelpBody>,
) -> Response {
    let device = body.device.trim();
    if device.is_empty() || device.len() > 64 || !body.lat.is_finite() || !body.lon.is_finite() {
        return (StatusCode::BAD_REQUEST, "demande invalide").into_response();
    }
    let req = AlertRequest {
        pos: LatLon { lat: body.lat, lon: body.lon },
        accuracy: body.accuracy,
        note: &body.note,
        device,
        ip: client_ip(&headers, peer),
    };
    match st.request_alert(req) {
        Ok(created) => (StatusCode::CREATED, Json(created)).into_response(),
        Err(error) => (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({ "error": error }))).into_response(),
    }
}

/// Suivi par la personne : en POST, pour que le secret ne finisse pas dans les journaux.
async fn help_status(State(st): State<AppState>, Json(body): Json<CancelBody>) -> Response {
    match st.alert_state(&body.id, &body.token) {
        Some(state) => Json(json!({ "state": state })).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn help_cancel(State(st): State<AppState>, Json(body): Json<CancelBody>) -> Response {
    if st.cancel_alert(&body.id, &body.token) {
        StatusCode::NO_CONTENT.into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

// ---------------------------------------------------------------- équipage (chauffeur ou orga)

fn crew(headers: &HeaderMap, st: &AppState) -> bool {
    authorized(headers, st.cfg.driver_token()) || authorized(headers, st.cfg.admin_token())
}

async fn crew_state(State(st): State<AppState>, headers: HeaderMap) -> Response {
    if !crew(&headers, &st) {
        return unauthorized();
    }
    let snapshot: Value = serde_json::from_str(&st.snapshot_json()).unwrap_or(Value::Null);
    let (last_fix, pickups, alerts) = {
        let t = st.tracker();
        (t.last_fix().copied(), t.crew_pickups(now_ms()), t.crew_alerts())
    };
    Json(json!({
        "snapshot": snapshot,
        "last_fix": last_fix,
        "pickups": pickups,
        "alerts": alerts,
        "viewers": st.viewers.load(Ordering::Relaxed),
        "route_length_m": st.route().length.round(),
    }))
    .into_response()
}

#[derive(Deserialize)]
struct PassengersBody {
    passengers: u32,
}

async fn crew_passengers(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<PassengersBody>,
) -> Response {
    if !crew(&headers, &st) {
        return unauthorized();
    }
    st.set_passengers(body.passengers);
    StatusCode::NO_CONTENT.into_response()
}

async fn crew_depart(State(st): State<AppState>, headers: HeaderMap) -> Response {
    if !crew(&headers, &st) {
        return unauthorized();
    }
    st.depart();
    StatusCode::NO_CONTENT.into_response()
}

#[derive(Deserialize)]
struct CrewPickupBody {
    id: String,
    action: PickupAction,
}

async fn crew_pickup(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CrewPickupBody>,
) -> Response {
    if !crew(&headers, &st) {
        return unauthorized();
    }
    if st.resolve_pickup(&body.id, body.action) {
        StatusCode::NO_CONTENT.into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

#[derive(Deserialize)]
struct CrewHelpBody {
    id: String,
    action: AlertAction,
}

async fn crew_help(State(st): State<AppState>, headers: HeaderMap, Json(body): Json<CrewHelpBody>) -> Response {
    if !crew(&headers, &st) {
        return unauthorized();
    }
    if st.resolve_alert(&body.id, body.action) {
        StatusCode::NO_CONTENT.into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

#[derive(Deserialize)]
struct ServiceBody {
    mode: ServiceMode,
}

async fn crew_service(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ServiceBody>,
) -> Response {
    if !crew(&headers, &st) {
        return unauthorized();
    }
    st.set_service(body.mode);
    StatusCode::NO_CONTENT.into_response()
}

#[derive(Deserialize)]
struct MessageBody {
    text: String,
    #[serde(default)]
    level: MessageLevel,
}

async fn crew_message(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<MessageBody>,
) -> Response {
    if !crew(&headers, &st) {
        return unauthorized();
    }
    let text: String = body.text.trim().chars().take(280).collect();
    st.set_message((!text.is_empty()).then_some(Message {
        text,
        level: body.level,
    }));
    StatusCode::NO_CONTENT.into_response()
}

// ---------------------------------------------------------------- orga : trajet

#[derive(Deserialize)]
struct RouteBody {
    via: Vec<[f64; 2]>,
    /// Arrêts déplacés ; absents = ceux de la config.
    #[serde(default)]
    stops: Vec<routing::StopEdit>,
}

struct RoutePlan {
    stops: Vec<StopConfig>,
    /// Les arrêts déplacés, recalés sur la route (ce qu'on enregistre).
    edits: Vec<routing::StopEdit>,
    coords: Vec<[f64; 2]>,
    route: Route,
}

/// Arrêts appliqués, trajet calculé et tracé, ou le message d'erreur à afficher.
/// Un arrêt glissé du doigt tombe rarement pile sur la rue : on le recale sur
/// la route trouvée par OSRM.
async fn plan_route(st: &AppState, body: &RouteBody) -> Result<RoutePlan, String> {
    let draft = routing::apply_stops(&st.cfg.stops, &body.stops)?;
    let planned = routing::compute(&st.cfg.route_editor, &draft, &body.via).await?;
    let mut edits = body.stops.clone();
    for edit in &mut edits {
        if let Some(&[lat, lon]) = st.cfg.stops.iter().position(|s| s.id == edit.id).and_then(|i| planned.stops.get(i)) {
            edit.lat = lat;
            edit.lon = lon;
        }
    }
    let stops = routing::apply_stops(&st.cfg.stops, &edits)?;
    let route = routing_route(&planned.coords)?;
    Ok(RoutePlan {
        stops,
        edits,
        coords: planned.coords,
        route,
    })
}

fn route_error(error: String) -> Response {
    (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({ "error": error }))).into_response()
}

async fn admin_route(State(st): State<AppState>, headers: HeaderMap) -> Response {
    if !authorized(&headers, st.cfg.admin_token()) {
        return unauthorized();
    }
    Json(st.route_editor_state()).into_response()
}

/// Calcule un trajet sans rien changer, pour l'aperçu dans la page orga.
async fn admin_route_preview(State(st): State<AppState>, headers: HeaderMap, Json(body): Json<RouteBody>) -> Response {
    if !authorized(&headers, st.cfg.admin_token()) {
        return unauthorized();
    }
    let plan = match plan_route(&st, &body).await {
        Ok(plan) => plan,
        Err(e) => return route_error(e),
    };
    Json(json!({
        "route": plan.coords,
        "length_m": plan.route.length.round(),
        "stops": plan.stops,
        "problem": routing::check(&plan.route, &plan.stops, st.original_route_m).err(),
    }))
    .into_response()
}

/// Publie le trajet : recalculé ici (on ne fait pas confiance au tracé envoyé).
async fn admin_route_apply(State(st): State<AppState>, headers: HeaderMap, Json(body): Json<RouteBody>) -> Response {
    if !authorized(&headers, st.cfg.admin_token()) {
        return unauthorized();
    }
    let plan = match plan_route(&st, &body).await {
        Ok(plan) => plan,
        Err(e) => return route_error(e),
    };
    if let Err(e) = routing::check(&plan.route, &plan.stops, st.original_route_m) {
        return route_error(e);
    }
    let custom = routing::CustomRoute {
        via: body.via,
        stops: plan.edits,
        coords: plan.coords,
    };
    match st.set_route(plan.route, Some(custom)) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            warn!("enregistrement du tracé impossible : {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "enregistrement impossible").into_response()
        }
    }
}

async fn admin_route_reset(State(st): State<AppState>, headers: HeaderMap) -> Response {
    if !authorized(&headers, st.cfg.admin_token()) {
        return unauthorized();
    }
    let result = Route::from_geojson_file(&st.cfg.server.route_file).and_then(|r| st.set_route(r, None));
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            warn!("retour au tracé d'origine impossible : {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "tracé d'origine illisible").into_response()
        }
    }
}

fn routing_route(coords: &[[f64; 2]]) -> Result<Route, String> {
    Route::new(coords.iter().map(|&[lat, lon]| LatLon { lat, lon }).collect()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osmand_query_params() {
        let q = HashMap::from([
            ("id".to_string(), "abc".to_string()),
            ("lat".to_string(), "46.2277".to_string()),
            ("lon".to_string(), "7.3584".to_string()),
            ("speed".to_string(), "10".to_string()),
            ("timestamp".to_string(), "1793000000".to_string()),
            ("batt".to_string(), "76".to_string()),
        ]);
        let (id, fixes) = parse_osmand(q, b"").unwrap();
        assert_eq!(id, "abc");
        assert_eq!(fixes[0].ts, 1_793_000_000_000);
        assert!((fixes[0].speed.unwrap() - 5.144).abs() < 0.01);
        assert_eq!(fixes[0].battery, Some(76.0));
    }

    #[test]
    fn osmand_form_body() {
        let (id, fixes) =
            parse_osmand(HashMap::new(), b"id=abc&lat=46.1&lon=7.3&bearing=90").unwrap();
        assert_eq!(id, "abc");
        assert_eq!(fixes[0].heading, Some(90.0));
    }

    #[test]
    fn osmand_json_body() {
        let body = br#"{"device_id":"abc","location":{"timestamp":"2026-10-30T21:00:00.000Z",
            "coords":{"latitude":46.2277,"longitude":7.3584,"accuracy":5,"speed":8.2,"heading":-1},
            "battery":{"level":0.53,"is_charging":true}}}"#;
        let (id, fixes) = parse_osmand(HashMap::new(), body).unwrap();
        assert_eq!(id, "abc");
        assert_eq!(fixes[0].speed, Some(8.2));
        assert_eq!(fixes[0].battery, Some(53.0));
        assert_eq!(fixes[0].ts, 1_793_394_000_000);
    }

    #[test]
    fn client_ip_trusts_only_a_local_proxy() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("1.2.3.4, 5.6.7.8"));
        let proxy: SocketAddr = "172.18.0.3:40000".parse().unwrap();
        let direct: SocketAddr = "9.9.9.9:40000".parse().unwrap();
        assert_eq!(client_ip(&headers, proxy), "5.6.7.8".parse::<IpAddr>().unwrap());
        assert_eq!(client_ip(&headers, direct), "9.9.9.9".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn constant_time_compare() {
        assert!(ct_eq("secret-token", "secret-token"));
        assert!(!ct_eq("secret-token", "secret-tokem"));
        assert!(!ct_eq("short", "secret-token"));
    }
}
