use std::ops::Deref;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde_json::json;
use tokio::sync::watch;
use tracing::{info, warn};

use crate::config::Config;
use crate::geo::Route;
use crate::tracker::{
    Created, Fix, Ingest, Message, Persisted, PickupAction, PickupRequest, ServiceMode, Tracker,
};
use crate::trains::Trains;

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[derive(Clone)]
pub struct AppState(Arc<Inner>);

pub struct Inner {
    pub cfg: Config,
    pub route: Arc<Route>,
    tracker: Mutex<Tracker>,
    /// Dernier état sérialisé : sérialisé une seule fois, partagé par tous les clients SSE.
    pub tx: watch::Sender<Arc<str>>,
    pub viewers: AtomicUsize,
    /// Réponse de `/api/config`, calculée au démarrage.
    pub config_json: String,
    /// Prochains départs de la gare (`None` si désactivé).
    pub trains: Option<Trains>,
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

        let tracker = Tracker::new(
            route.clone(),
            &cfg.stops,
            cfg.tracking.clone(),
            cfg.shuttle.capacity,
            cfg.pickups.clone(),
            persisted,
        );
        for (id, s) in tracker.stop_positions() {
            info!("arrêt {id} à {s:.0} m du départ du tracé");
        }

        let config_json = json!({
            "event": cfg.event,
            "stops": cfg.stops,
            "route": route.coords().iter().map(|c| [c.lat, c.lon]).collect::<Vec<_>>(),
            "route_length_m": route.length.round(),
            "stale_after_s": cfg.tracking.stale_after_s,
            "capacity": cfg.shuttle.capacity,
            "pickups": cfg.pickups.enabled.then(|| json!({
                "max_group": cfg.pickups.max_group,
                "min_stop_distance_m": cfg.pickups.min_stop_distance_m,
                "max_distance_m": cfg.pickups.max_distance_m,
            })),
            "trains":cfg.trains.enabled.then(|| json!({ "station": cfg.trains.station_name })),
        })
        .to_string();

        let initial = serde_json::to_string(&tracker.snapshot(now_ms()))?;
        let (tx, _) = watch::channel(Arc::from(initial));
        let trains = cfg
            .trains
            .enabled
            .then(|| Trains::new(cfg.trains.clone()))
            .transpose()?;

        Ok(Self(Arc::new(Inner {
            cfg,
            route,
            tracker: Mutex::new(tracker),
            tx,
            viewers: AtomicUsize::new(0),
            config_json,
            trains,
        })))
    }

    pub fn tracker(&self) -> MutexGuard<'_, Tracker> {
        self.tracker.lock().unwrap_or_else(|e| e.into_inner())
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
        let result = t.ingest(fix, now_ms());
        if matches!(result, Ingest::Accepted) {
            self.publish(&mut t);
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

    fn persist(&self, t: &Tracker) {
        let path = self.cfg.server.data_dir.join("state.json");
        let res = serde_json::to_vec_pretty(&t.persisted())
            .map_err(std::io::Error::other)
            .and_then(|raw| std::fs::write(&path, raw));
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
            let pickups_changed = t.expire_pickups(now);
            if pickups_changed || t.published_status != Some(t.status(now)) {
                self.publish(&mut t);
            }
        }
    }
}
