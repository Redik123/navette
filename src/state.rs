use std::ops::Deref;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use std::time::Duration;

use serde_json::json;
use tokio::sync::watch;
use tracing::{info, warn};

use crate::config::{Config, StopConfig};
use crate::geo::Route;
use crate::routing::{self, CustomRoute};
use crate::tracker::{
    AlertAction, AlertCreated, AlertRequest, AlertState, Created, Fix, Ingest, Message, Persisted, PickupAction,
    PickupRequest, ServiceMode, Tracker,
};
use crate::trains::Trains;

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn custom_route_path(cfg: &Config) -> std::path::PathBuf {
    cfg.server.data_dir.join("route.json")
}

fn build_config_json(cfg: &Config, route: &Route, stops: &[StopConfig], version: i64, custom: bool) -> Arc<str> {
    json!({
        "event": cfg.event,
        "stops": stops,
        "route": route.coords().iter().map(|c| [c.lat, c.lon]).collect::<Vec<_>>(),
        "route_length_m": route.length.round(),
        "route_version": version,
        "route_custom": custom,
        "stale_after_s": cfg.tracking.stale_after_s,
        "capacity": cfg.shuttle.capacity,
        "pickups": cfg.pickups.enabled.then(|| json!({
            "max_group": cfg.pickups.max_group,
            "min_stop_distance_m": cfg.pickups.min_stop_distance_m,
            "max_distance_m": cfg.pickups.max_distance_m,
        })),
        "trains":cfg.trains.enabled.then(|| json!({ "station": cfg.trains.station_name })),
    })
    .to_string()
    .into()
}

#[derive(Clone)]
pub struct AppState(Arc<Inner>);

pub struct Inner {
    pub cfg: Config,
    /// Le tracé courant : celui de `route.geojson`, ou celui modifié par l'orga.
    route: RwLock<RouteNow>,
    tracker: Mutex<Tracker>,
    /// Dernier état sérialisé : sérialisé une seule fois, partagé par tous les clients SSE.
    pub tx: watch::Sender<Arc<str>>,
    pub viewers: AtomicUsize,
    /// Prochains départs de la gare (`None` si désactivé).
    pub trains: Option<Trains>,
    /// Longueur du tracé d'origine (`route.geojson`), pour juger un détour.
    pub original_route_m: f64,
}

struct RouteNow {
    route: Arc<Route>,
    /// Points de passage de l'aller, et s'ils viennent de l'orga (sinon de la config).
    via: Vec<[f64; 2]>,
    /// Arrêts en vigueur : ceux de la config, éventuellement déplacés par l'orga.
    stops: Vec<StopConfig>,
    custom: bool,
    /// Réponse de `/api/config`, recalculée à chaque changement de tracé.
    config_json: Arc<str>,
}

impl Deref for AppState {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.0
    }
}

impl AppState {
    pub fn new(cfg: Config, route: Arc<Route>) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&cfg.server.data_dir)?;
        let persisted = std::fs::read_to_string(cfg.server.data_dir.join("state.json"))
            .ok()
            .and_then(|raw| serde_json::from_str::<Persisted>(&raw).ok())
            .unwrap_or_default();

        // Un tracé modifié par l'orga passe avant celui de l'image.
        let original_route_m = route.length;
        let custom_route = CustomRoute::load(&custom_route_path(&cfg))
            .map(|c| -> anyhow::Result<_> {
                let stops = routing::apply_stops(&cfg.stops, &c.stops).map_err(anyhow::Error::msg)?;
                Ok((c.route()?, c.via, stops))
            });
        let (route, via, stops, custom) = match custom_route {
            Some(Ok((r, via, stops))) => {
                info!("tracé modifié par l'orga : {:.0} m", r.length);
                (Arc::new(r), via, stops, true)
            }
            Some(Err(e)) => {
                warn!("tracé modifié illisible, on garde l'original : {e}");
                (route, cfg.route_editor.via.clone(), cfg.stops.clone(), false)
            }
            None => (route, cfg.route_editor.via.clone(), cfg.stops.clone(), false),
        };

        let version = now_ms();
        let mut tracker = Tracker::new(
            route.clone(),
            &stops,
            cfg.tracking.clone(),
            cfg.shuttle.capacity,
            cfg.pickups.clone(),
            persisted,
        );
        tracker.route_version = version;
        for (id, s) in tracker.stop_positions() {
            info!("arrêt {id} à {s:.0} m du départ du tracé");
        }

        let config_json = build_config_json(&cfg, &route, &stops, version, custom);
        let initial = serde_json::to_string(&tracker.snapshot(now_ms()))?;
        let (tx, _) = watch::channel(Arc::from(initial));
        let trains = cfg
            .trains
            .enabled
            .then(|| Trains::new(cfg.trains.clone()))
            .transpose()?;

        Ok(Self(Arc::new(Inner {
            cfg,
            route: RwLock::new(RouteNow {
                route,
                via,
                stops,
                custom,
                config_json,
            }),
            tracker: Mutex::new(tracker),
            tx,
            viewers: AtomicUsize::new(0),
            trains,
            original_route_m,
        })))
    }

    pub fn tracker(&self) -> MutexGuard<'_, Tracker> {
        self.tracker.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn route(&self) -> Arc<Route> {
        self.route.read().unwrap_or_else(|e| e.into_inner()).route.clone()
    }

    pub fn config_json(&self) -> Arc<str> {
        self.route.read().unwrap_or_else(|e| e.into_inner()).config_json.clone()
    }

    /// Points de passage et arrêts courants, ceux de la config, et si le tracé a été modifié.
    pub fn route_editor_state(&self) -> serde_json::Value {
        let now = self.route.read().unwrap_or_else(|e| e.into_inner());
        json!({
            "via": now.via,
            "stops": now.stops,
            "default_via": self.cfg.route_editor.via,
            "default_stops": self.cfg.stops,
            "custom": now.custom,
            "length_m": now.route.length.round(),
        })
    }

    /// Remplace le tracé (et les arrêts) pour tout le monde. `custom: None`
    /// revient à l'original. Les arrêts de `custom` doivent déjà être validés.
    pub fn set_route(&self, route: Route, custom: Option<CustomRoute>) -> anyhow::Result<()> {
        let path = custom_route_path(&self.cfg);
        let (via, stops) = match &custom {
            Some(c) => {
                let stops = routing::apply_stops(&self.cfg.stops, &c.stops).map_err(anyhow::Error::msg)?;
                c.save(&path)?;
                (c.via.clone(), stops)
            }
            None => {
                if path.exists() {
                    std::fs::remove_file(&path)?;
                }
                (self.cfg.route_editor.via.clone(), self.cfg.stops.clone())
            }
        };
        let route = Arc::new(route);
        let version = now_ms();
        let mut t = self.tracker();
        t.set_route(route.clone(), &stops, version);
        *self.route.write().unwrap_or_else(|e| e.into_inner()) = RouteNow {
            config_json: build_config_json(&self.cfg, &route, &stops, version, custom.is_some()),
            route: route.clone(),
            via,
            stops,
            custom: custom.is_some(),
        };
        self.publish(&mut t);
        info!("nouveau tracé : {:.0} m{}", route.length, if custom.is_some() { "" } else { " (original)" });
        Ok(())
    }

    fn publish(&self, t: &mut Tracker) {
        let snap = t.snapshot(now_ms());
        t.published_status = Some(snap.status);
        match serde_json::to_string(&snap) {
            Ok(json) => {
                self.tx.send_replace(Arc::from(json));
            }
            Err(e) => warn!("sérialisation de l'état impossible : {e}"),
        }
    }

    pub fn snapshot_json(&self) -> String {
        let t = self.tracker();
        serde_json::to_string(&t.snapshot(now_ms())).unwrap_or_default()
    }

    pub fn ingest(&self, fix: Fix) -> Ingest {
        let mut t = self.tracker();
        let trips = t.trips().len();
        let result = t.ingest(fix, now_ms());
        if matches!(result, Ingest::Accepted) {
            self.publish(&mut t);
        }
        // Arrivée à un arrêt avec du monde à bord : on garde le trajet sur disque.
        if let Some(trip) = t.trips().get(trips) {
            info!("trajet terminé : {} personne(s) déposée(s) à {}", trip.count, trip.stop);
            self.persist(&t);
        }
        result
    }

    pub fn set_service(&self, mode: ServiceMode) {
        let mut t = self.tracker();
        t.service = mode;
        self.persist(&t);
        self.publish(&mut t);
        info!("mode de service : {mode:?}");
    }

    pub fn set_message(&self, message: Option<Message>) {
        let mut t = self.tracker();
        t.message = message;
        self.persist(&t);
        self.publish(&mut t);
        info!("message : {:?}", t.message.as_ref().map(|m| &m.text));
    }

    pub fn set_passengers(&self, count: u32) {
        let mut t = self.tracker();
        t.set_passengers(count);
        self.publish(&mut t);
    }

    pub fn depart(&self) {
        let mut t = self.tracker();
        t.depart(now_ms());
        self.publish(&mut t);
        info!("départ annoncé");
    }

    pub fn request_pickup(&self, req: PickupRequest) -> Result<Created, String> {
        let mut t = self.tracker();
        let created = t.request_pickup(req, now_ms())?;
        self.publish(&mut t);
        info!("demande de prise en charge {}", created.id);
        Ok(created)
    }

    pub fn cancel_pickup(&self, id: &str, token: &str) -> bool {
        let mut t = self.tracker();
        let done = t.cancel_pickup(id, token, now_ms());
        if done {
            self.publish(&mut t);
        }
        done
    }

    pub fn resolve_pickup(&self, id: &str, action: PickupAction) -> bool {
        let mut t = self.tracker();
        let done = t.resolve_pickup(id, action, now_ms());
        if done {
            self.publish(&mut t);
        }
        done
    }

    // Les alertes ne sont pas diffusées au public : rien à publier, l'équipage
    // les reçoit par /api/crew/state. On ne journalise jamais la position.
    pub fn request_alert(&self, req: AlertRequest) -> Result<AlertCreated, String> {
        let created = self.tracker().request_alert(req, now_ms())?;
        warn!("demande d'aide {}", created.id);
        Ok(created)
    }

    pub fn alert_state(&self, id: &str, token: &str) -> Option<AlertState> {
        self.tracker().alert_state(id, token)
    }

    pub fn cancel_alert(&self, id: &str, token: &str) -> bool {
        self.tracker().cancel_alert(id, token, now_ms())
    }

    pub fn resolve_alert(&self, id: &str, action: AlertAction) -> bool {
        let done = self.tracker().resolve_alert(id, action, now_ms());
        if done {
            info!("demande d'aide {id} : {action:?}");
        }
        done
    }

    /// Écrit dans un fichier à côté puis le renomme : un arrêt brutal pendant
    /// l'écriture ne peut pas abîmer `state.json` (et les statistiques).
    fn persist(&self, t: &Tracker) {
        let path = self.cfg.server.data_dir.join("state.json");
        let tmp = path.with_extension("json.tmp");
        let res = serde_json::to_vec_pretty(&t.persisted())
            .map_err(std::io::Error::other)
            .and_then(|raw| std::fs::write(&tmp, raw))
            .and_then(|()| std::fs::rename(&tmp, &path));
        if let Err(e) = res {
            warn!("écriture de {} impossible : {e}", path.display());
        }
    }

    /// Republie l'état quand le statut change sans nouvelle position
    /// (ex. la navette ne donne plus signe de vie → « signal perdu »),
    /// et fait le ménage dans les demandes de prise en charge.
    pub async fn status_ticker(self) {
        let mut tick = tokio::time::interval(Duration::from_secs(5));
        loop {
            tick.tick().await;
            let now = now_ms();
            let mut t = self.tracker();
            t.expire_alerts(now);
            let pickups_changed = t.expire_pickups(now);
            if pickups_changed || t.published_status != Some(t.status(now)) {
                self.publish(&mut t);
            }
        }
    }
}
