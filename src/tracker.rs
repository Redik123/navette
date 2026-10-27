//! État de la navette : réception des positions, accrochage au tracé,
//! statut et temps d'arrivée estimés aux arrêts.

mod pickups;

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::config::{PickupsConfig, StopConfig, TrackingConfig};
use crate::geo::{LatLon, Projection, Route, angle_diff, haversine};

pub use pickups::{Created, PickupAction, PickupRequest};
use pickups::{ClosedPickup, Pickup, PublicClosed, PublicPickup};

/// Recul toléré (bruit GPS) avant de considérer qu'on change de branche du tracé.
const JITTER_M: f64 = 40.0;
/// Vitesse max plausible d'un bus en ville, pour filtrer les sauts de position.
const MAX_SPEED_MPS: f64 = 25.0;
/// On ne quitte un arrêt qu'une fois sorti de son rayon de cette marge en plus :
/// le GPS qui sautille au bord du rayon ne doit pas remettre le compteur à zéro.
const STOP_EXIT_MARGIN_M: f64 = 40.0;

/// Une position reçue du téléphone de la navette.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Fix {
    #[serde(flatten)]
    pub pos: LatLon,
    pub accuracy: Option<f64>,
    /// m/s
    pub speed: Option<f64>,
    /// degrés, 0 = nord
    pub heading: Option<f64>,
    /// horodatage Unix en millisecondes
    pub ts: i64,
    /// batterie du téléphone en %
    pub battery: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceMode {
    /// Statut déduit des positions reçues.
    #[default]
    Auto,
    /// Pause du chauffeur : position affichée, pas d'ETA.
    Pause,
    /// Hors service : navette masquée.
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageLevel {
    #[default]
    Info,
    Warn,
    Alert,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub text: String,
    #[serde(default)]
    pub level: MessageLevel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Moving,
    /// À quai, en attente : pas d'horaire, elle part quand elle est pleine.
    AtStop,
    /// Encore à l'arrêt, mais l'équipage a annoncé le départ.
    Departing,
    Stale,
    Offline,
    Pause,
    Off,
}

/// Ce qui survit à un redémarrage du serveur.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Persisted {
    #[serde(default)]
    pub service: ServiceMode,
    pub message: Option<Message>,
}

#[derive(Debug)]
pub enum Ingest {
    Accepted,
    Ignored(&'static str),
}

/// Départ annoncé par le chauffeur ou l'orga.
#[derive(Debug, Clone, Serialize)]
pub struct Departure {
    /// Arrêt quitté (`None` si annoncé ailleurs qu'à un arrêt).
    pub stop: Option<String>,
    pub ts: i64,
}

#[derive(Debug, Clone, Copy)]
struct Snap {
    s: f64,
    point: LatLon,
    bearing: Option<f64>,
    on_route: bool,
}

struct Stop {
    cfg: StopConfig,
    s: f64,
}

impl Stop {
    fn pos(&self) -> LatLon {
        LatLon {
            lat: self.cfg.lat,
            lon: self.cfg.lon,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Snapshot {
    pub server_time: i64,
    pub service: ServiceMode,
    pub status: Status,
    pub message: Option<Message>,
    pub bus: Option<BusView>,
    pub at_stop: Option<String>,
    pub next_stop: Option<String>,
    pub etas: Vec<Eta>,
    pub capacity: u32,
    /// Personnes à bord, `null` tant que l'équipage n'a rien indiqué.
    pub passengers: Option<u32>,
    pub departure: Option<Departure>,
    /// Demandes de prise en charge en cours, sans position : chacun y
    /// retrouve la sienne par son identifiant.
    pub pickups: Vec<PublicPickup>,
    pub pickups_closed: Vec<PublicClosed>,
}

#[derive(Debug, Serialize)]
pub struct BusView {
    pub lat: f64,
    pub lon: f64,
    /// Position recalée sur le tracé (plus propre à afficher).
    pub snap_lat: f64,
    pub snap_lon: f64,
    pub on_route: bool,
    pub heading: Option<f64>,
    pub speed_kmh: Option<f64>,
    pub accuracy: Option<f64>,
    pub ts: i64,
}

#[derive(Debug, Serialize)]
pub struct Eta {
    pub stop: String,
    pub name: String,
    pub distance_m: f64,
    pub eta_s: f64,
}

pub struct Tracker {
    route: Arc<Route>,
    stops: Vec<Stop>,
    tracking: TrackingConfig,
    capacity: u32,
    pickups_cfg: PickupsConfig,
    last_fix: Option<Fix>,
    snap: Option<Snap>,
    /// Arrêt où se trouve la navette et heure (serveur) de son arrivée.
    arrival: Option<(usize, i64)>,
    passengers: Option<u32>,
    departure: Option<Departure>,
    pickups: Vec<Pickup>,
    closed_pickups: Vec<ClosedPickup>,
    /// Créations récentes de demandes, pour limiter les abus.
    pickup_requests: Vec<(std::net::IpAddr, i64)>,
    pub service: ServiceMode,
    pub message: Option<Message>,
    /// Dernier statut diffusé aux clients, pour ne republier qu'en cas de changement.
    pub published_status: Option<Status>,
}

impl Tracker {
    pub fn new(
        route: Arc<Route>,
        stops: &[StopConfig],
        tracking: TrackingConfig,
        capacity: u32,
        pickups_cfg: PickupsConfig,
        persisted: Persisted,
    ) -> Self {
        let stops = stops
            .iter()
            .map(|c| Stop {
                s: route.nearest(LatLon { lat: c.lat, lon: c.lon }).s,
                cfg: c.clone(),
            })
            .collect();
        Self {
            route,
            stops,
            tracking,
            capacity,
            pickups_cfg,
            last_fix: None,
            snap: None,
            arrival: None,
            passengers: None,
            departure: None,
            pickups: Vec::new(),
            closed_pickups: Vec::new(),
            pickup_requests: Vec::new(),
            service: persisted.service,
            message: persisted.message,
            published_status: None,
        }
    }

    pub fn persisted(&self) -> Persisted {
        Persisted {
            service: self.service,
            message: self.message.clone(),
        }
    }

    pub fn last_fix(&self) -> Option<&Fix> {
        self.last_fix.as_ref()
    }

    /// Distance de chaque arrêt depuis le début du tracé.
    pub fn stop_positions(&self) -> impl Iterator<Item = (&str, f64)> {
        self.stops.iter().map(|st| (st.cfg.id.as_str(), st.s))
    }

    pub fn ingest(&mut self, mut fix: Fix, now: i64) -> Ingest {
        let LatLon { lat, lon } = fix.pos;
        if !(-90.0..=90.0).contains(&lat)
            || !(-180.0..=180.0).contains(&lon)
            || (lat == 0.0 && lon == 0.0)
        {
            return Ingest::Ignored("coordonnées invalides");
        }
        if fix
            .accuracy
            .is_some_and(|a| a > self.tracking.max_accuracy_m)
        {
            return Ingest::Ignored("précision insuffisante");
        }
        // Horloge du téléphone en avance : on se fie à celle du serveur.
        if fix.ts > now + 60_000 {
            fix.ts = now;
        }
        if fix.ts < now - self.tracking.offline_after_s * 1000 {
            return Ingest::Ignored("position trop ancienne");
        }
        // Traccar renvoie les positions mises en tampon hors réseau : on ne recule jamais.
        if self.last_fix.is_some_and(|prev| fix.ts <= prev.ts) {
            return Ingest::Ignored("position plus ancienne que la dernière reçue");
        }
        // Valeurs « inconnu » envoyées par certaines applis.
        fix.speed = fix.speed.filter(|v| *v >= 0.0);
        fix.heading = fix.heading.filter(|h| *h >= 0.0);

        let prev_s = self.snap.filter(|s| s.on_route).map(|s| s.s);
        let snap = self.snap_fix(&fix);
        self.snap = Some(snap);
        self.last_fix = Some(fix);
        self.update_arrival(now);
        if let (Some(from), true) = (prev_s, snap.on_route) {
            self.close_passed_pickups(from, snap.s, now);
        }
        Ingest::Accepted
    }

    /// Choisit le point du tracé le plus cohérent avec la nouvelle position :
    /// proche, dans le sens de marche, et dans la continuité de la précédente.
    /// Indispensable quand l'aller et le retour empruntent la même route.
    fn snap_fix(&self, fix: &Fix) -> Snap {
        let max_dist = (fix.accuracy.unwrap_or(0.0) * 1.5).clamp(self.tracking.snap_max_m, 250.0);
        let candidates = self.route.candidates(fix.pos, max_dist);

        let prev = match (self.snap, self.last_fix) {
            (Some(s), Some(f))
                if s.on_route && fix.ts - f.ts < self.tracking.offline_after_s * 1000 =>
            {
                Some((s.s, f.ts))
            }
            _ => None,
        };

        if candidates.is_empty() {
            return Snap {
                s: prev.map_or(0.0, |(s, _)| s),
                point: fix.pos,
                bearing: None,
                on_route: false,
            };
        }

        let heading = match (fix.heading, fix.speed) {
            (Some(h), Some(v)) if v > 2.0 => Some(h),
            _ => None,
        };

        let score = |c: &Projection| {
            let mut score = c.dist;
            if let Some(h) = heading {
                let diff = angle_diff(h, c.bearing);
                if diff > 100.0 {
                    score += 60.0;
                } else if diff > 60.0 {
                    score += 20.0;
                }
            }
            if let Some((ps, pts)) = prev {
                let dt = ((fix.ts - pts) as f64 / 1000.0).max(1.0);
                let (fwd, back) = self.route.deltas(ps, c.s);
                if fwd <= 30.0 + dt * MAX_SPEED_MPS {
                    score += fwd * 0.02;
                } else if back <= JITTER_M {
                    score += 5.0 + back * 0.2;
                } else {
                    score += 150.0;
                }
            }
            score
        };

        let best = candidates
            .iter()
            .min_by(|a, b| score(a).total_cmp(&score(b)))
            .copied()
            .expect("candidats non vides");

        // Petit recul dû au bruit GPS : la progression reste où elle était.
        let s = match prev {
            Some((ps, _)) => {
                let (fwd, back) = self.route.deltas(ps, best.s);
                if back > 0.0 && back <= JITTER_M && fwd > back {
                    ps
                } else {
                    best.s
                }
            }
            None => best.s,
        };

        Snap {
            s,
            point: best.point,
            bearing: Some(best.bearing),
            on_route: true,
        }
    }

    fn at_stop(&self) -> Option<usize> {
        let fix = self.last_fix?;
        self.stops
            .iter()
            .position(|st| haversine(fix.pos, st.pos()) <= st.cfg.radius_m)
    }

    /// Repère l'arrivée à un arrêt : tout le monde descend, le compteur de
    /// places repart de zéro (si l'équipage s'en sert).
    fn update_arrival(&mut self, now: i64) {
        let Some(fix) = self.last_fix else { return };
        if let Some((i, _)) = self.arrival {
            let st = &self.stops[i];
            if haversine(fix.pos, st.pos()) <= st.cfg.radius_m + STOP_EXIT_MARGIN_M {
                return;
            }
            self.arrival = None;
        }
        if let Some(i) = self.at_stop() {
            self.arrival = Some((i, now));
            if self.passengers.is_some() {
                self.passengers = Some(0);
            }
        }
    }

    /// Départ annoncé depuis l'arrêt où se trouve la navette.
    fn departed(&self) -> bool {
        match (self.arrival, &self.departure) {
            (Some((_, arrived)), Some(dep)) => dep.ts >= arrived,
            _ => false,
        }
    }

    pub fn status(&self, now: i64) -> Status {
        match self.service {
            ServiceMode::Off => return Status::Off,
            ServiceMode::Pause => return Status::Pause,
            ServiceMode::Auto => {}
        }
        let Some(fix) = self.last_fix else {
            return Status::Offline;
        };
        let age = (now - fix.ts) / 1000;
        if age > self.tracking.offline_after_s {
            Status::Offline
        } else if age > self.tracking.stale_after_s {
            Status::Stale
        } else if self.arrival.is_none() {
            Status::Moving
        } else if self.departed() {
            Status::Departing
        } else {
            Status::AtStop
        }
    }

    pub fn set_passengers(&mut self, count: u32) {
        self.passengers = Some(count.min(self.capacity));
    }

    pub fn depart(&mut self, now: i64) {
        self.departure = Some(Departure {
            stop: self.arrival.map(|(i, _)| self.stops[i].cfg.id.clone()),
            ts: now,
        });
    }

    /// Distance et temps pour aller de `from` à `to` en suivant le tracé,
    /// arrêts intermédiaires compris (`at` : l'arrêt où se trouve la navette).
    fn travel(&self, from: f64, to: f64, at: Option<usize>) -> Option<(f64, f64)> {
        let (fwd, _) = self.route.deltas(from, to);
        if !fwd.is_finite() {
            return None;
        }
        let stops_between = self
            .stops
            .iter()
            .enumerate()
            .filter(|&(j, o)| {
                let d = self.route.deltas(from, o.s).0;
                Some(j) != at && d > 0.0 && d < fwd
            })
            .count();
        let speed = self.tracking.avg_speed_kmh / 3.6;
        Some((fwd, fwd / speed + stops_between as f64 * self.tracking.dwell_s))
    }

    fn etas(&self, snap: Snap, at: Option<usize>) -> Vec<Eta> {
        let mut etas: Vec<Eta> = self
            .stops
            .iter()
            .enumerate()
            .filter_map(|(i, st)| {
                let (distance, eta) = if Some(i) == at {
                    (0.0, 0.0)
                } else {
                    self.travel(snap.s, st.s, at)?
                };
                Some(Eta {
                    stop: st.cfg.id.clone(),
                    name: st.cfg.name.clone(),
                    distance_m: distance.round(),
                    eta_s: eta.round(),
                })
            })
            .collect();
        etas.sort_by(|a, b| a.eta_s.total_cmp(&b.eta_s));
        etas
    }

    /// Position de la navette sur le tracé, si elle circule et y est accrochée.
    fn live_snap(&self, now: i64) -> Option<Snap> {
        let live = matches!(
            self.status(now),
            Status::Moving | Status::AtStop | Status::Departing
        );
        self.snap.filter(|s| live && s.on_route)
    }

    pub fn snapshot(&self, now: i64) -> Snapshot {
        let status = self.status(now);
        let at = self.arrival.map(|(i, _)| i);

        let bus = match (status, self.last_fix, self.snap) {
            (Status::Off | Status::Offline, ..) => None,
            (_, Some(fix), Some(snap)) => Some(BusView {
                lat: fix.pos.lat,
                lon: fix.pos.lon,
                snap_lat: snap.point.lat,
                snap_lon: snap.point.lon,
                on_route: snap.on_route,
                heading: snap
                    .bearing
                    .or(fix.heading.filter(|_| fix.speed.unwrap_or(0.0) > 1.5))
                    .map(f64::round),
                speed_kmh: fix.speed.map(|v| (v * 3.6).round()),
                accuracy: fix.accuracy.map(f64::round),
                ts: fix.ts,
            }),
            _ => None,
        };

        let live_snap = self.live_snap(now);
        let etas = live_snap.map_or_else(Vec::new, |snap| self.etas(snap, at));
        let at_stop = at
            .filter(|_| live_snap.is_some())
            .map(|i| self.stops[i].cfg.id.clone());
        let next_stop = etas.iter().find(|e| e.eta_s > 0.0).map(|e| e.stop.clone());

        Snapshot {
            server_time: now,
            service: self.service,
            status,
            message: self.message.clone(),
            bus,
            at_stop,
            next_stop,
            etas,
            capacity: self.capacity,
            passengers: self.passengers,
            departure: self.departure.clone(),
            pickups: self.public_pickups(now),
            pickups_closed: self.public_closed(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) const LAT: f64 = 46.2277;
    pub(super) const LON: f64 = 7.3584;

    /// Aller-retour sur une même rue nord-sud de ~555 m : le cas difficile.
    pub(super) fn shuttle() -> Tracker {
        let north = LatLon { lat: LAT + 0.005, lon: LON };
        let south = LatLon { lat: LAT, lon: LON };
        let route = Arc::new(Route::new(vec![south, north, south]).unwrap());
        let stops = [
            StopConfig {
                id: "gare".into(),
                name: "Gare".into(),
                lat: south.lat,
                lon: south.lon,
                radius_m: 30.0,
                detail: String::new(),
            },
            StopConfig {
                id: "festival".into(),
                name: "Festival".into(),
                lat: north.lat,
                lon: north.lon,
                radius_m: 30.0,
                detail: String::new(),
            },
        ];
        Tracker::new(
            route,
            &stops,
            TrackingConfig::default(),
            16,
            PickupsConfig::default(),
            Persisted::default(),
        )
    }

    pub(super) fn fix(dlat: f64, heading: f64, ts: i64) -> Fix {
        Fix {
            pos: LatLon { lat: LAT + dlat, lon: LON },
            accuracy: Some(5.0),
            speed: Some(8.0),
            heading: Some(heading),
            ts,
            battery: None,
        }
    }

    #[test]
    fn heading_picks_the_right_direction() {
        let mut t = shuttle();
        t.ingest(fix(0.002, 0.0, 1_000), 1_000);
        let northbound = t.snap.unwrap().s;
        assert!(northbound < 300.0, "aller attendu, s = {northbound}");

        let mut t = shuttle();
        t.ingest(fix(0.002, 180.0, 1_000), 1_000);
        let southbound = t.snap.unwrap().s;
        assert!(southbound > 600.0, "retour attendu, s = {southbound}");
    }

    #[test]
    fn continuity_without_heading() {
        let mut t = shuttle();
        t.ingest(fix(0.004, 180.0, 1_000), 1_000);
        let mut f = fix(0.0035, 0.0, 6_000);
        f.speed = None;
        f.heading = None;
        t.ingest(f, 6_000);
        assert!(t.snap.unwrap().s > 600.0, "doit rester sur le retour");
    }

    #[test]
    fn etas_follow_the_loop() {
        let mut t = shuttle();
        t.ingest(fix(0.001, 0.0, 1_000), 1_000);
        let snap = t.snapshot(2_000);
        assert_eq!(snap.status, Status::Moving);
        assert_eq!(snap.next_stop.as_deref(), Some("festival"));
        assert_eq!(snap.etas[0].stop, "festival");
        assert!((snap.etas[0].distance_m - 445.0).abs() < 5.0);
        assert_eq!(snap.etas[1].stop, "gare");
    }

    #[test]
    fn at_stop_and_staleness() {
        let mut t = shuttle();
        t.ingest(fix(0.005, 0.0, 1_000), 1_000);
        let snap = t.snapshot(2_000);
        assert_eq!(snap.status, Status::AtStop);
        assert_eq!(snap.at_stop.as_deref(), Some("festival"));
        assert_eq!(snap.etas[0].eta_s, 0.0);

        assert_eq!(t.status(1_000 + 120_000), Status::Stale);
        assert_eq!(t.status(1_000 + 700_000), Status::Offline);
        assert!(t.snapshot(1_000 + 700_000).bus.is_none());
    }

    #[test]
    fn rejects_out_of_order_and_inaccurate() {
        let mut t = shuttle();
        assert!(matches!(t.ingest(fix(0.001, 0.0, 5_000), 5_000), Ingest::Accepted));
        assert!(matches!(t.ingest(fix(0.002, 0.0, 4_000), 5_000), Ingest::Ignored(_)));
        let mut bad = fix(0.002, 0.0, 6_000);
        bad.accuracy = Some(500.0);
        assert!(matches!(t.ingest(bad, 6_000), Ingest::Ignored(_)));
    }

    #[test]
    fn departure_and_passengers() {
        let mut t = shuttle();
        t.ingest(fix(0.0, 0.0, 1_000), 1_000);
        assert_eq!(t.status(1_500), Status::AtStop);
        t.set_passengers(20);
        assert_eq!(t.passengers, Some(16), "plafonné à la capacité");

        // Le GPS sautille au bord du rayon : on reste à quai, le compteur aussi.
        t.ingest(fix(0.0004, 0.0, 2_000), 2_000);
        assert_eq!(t.status(2_500), Status::AtStop);
        assert_eq!(t.passengers, Some(16));

        t.depart(3_000);
        assert_eq!(t.status(3_000), Status::Departing);
        assert_eq!(t.departure.as_ref().unwrap().stop.as_deref(), Some("gare"));

        t.ingest(fix(0.002, 0.0, 30_000), 30_000);
        assert_eq!(t.status(30_000), Status::Moving);

        // Arrivée au festival : tout le monde descend, nouvelle attente.
        t.ingest(fix(0.005, 0.0, 70_000), 70_000);
        assert_eq!(t.status(70_000), Status::AtStop);
        assert_eq!(t.passengers, Some(0));
    }
}
