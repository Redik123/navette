//! « Faire signe à la navette » : une personne sur le trajet, loin d'un arrêt,
//! demande à être prise en route. On ne garde que le point du tracé où elle
//! attend (pas sa position GPS), et la demande disparaît dès que la navette
//! est passée, ou au bout de quelques minutes.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::net::IpAddr;

use serde::{Deserialize, Serialize};

use super::{ServiceMode, Status, Tracker};
use crate::geo::{LatLon, haversine};

/// Fenêtre et plafond de créations par adresse IP.
const RATE_WINDOW_MS: i64 = 10 * 60_000;
const MAX_REQUESTS_PER_IP: usize = 6;
/// Plusieurs téléphones peuvent partager une adresse IP (réseau mobile, Wi-Fi).
const MAX_ACTIVE_PER_IP: usize = 3;
/// Combien de temps la personne peut voir ce qu'est devenue sa demande.
const CLOSED_KEEP_MS: i64 = 15 * 60_000;
/// La navette doit avoir dépassé le point de cette distance pour clore la
/// demande : le temps pour le chauffeur de la marquer « prise ».
const PASS_MARGIN_M: f64 = 40.0;
/// Au-delà, un bond sur le tracé n'est pas un passage (GPS perdu puis retrouvé).
const MAX_PASS_JUMP_M: f64 = 800.0;
const MAX_ACCURACY_M: f64 = 100.0;

pub(super) struct Pickup {
    id: String,
    token: String,
    device: String,
    ip: IpAddr,
    /// Point du tracé, sur le passage qui mène à la destination.
    s: f64,
    point: LatLon,
    count: u32,
    dest: usize,
    created: i64,
}

pub(super) struct ClosedPickup {
    id: String,
    outcome: Outcome,
    ts: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Le chauffeur l'a marquée « prise ».
    Picked,
    /// La navette est passée par ce point.
    Passed,
    /// Le chauffeur n'a pas pu s'arrêter.
    Dismissed,
    Expired,
    Cancelled,
}

/// Ce que l'équipage fait d'une demande.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PickupAction {
    Picked,
    Dismissed,
}

pub struct PickupRequest<'a> {
    pub pos: LatLon,
    pub accuracy: Option<f64>,
    pub count: u32,
    /// Arrêt où la personne veut aller : décide du sens (aller ou retour).
    pub dest: &'a str,
    /// Identifiant aléatoire du téléphone : une demande à la fois.
    pub device: &'a str,
    pub ip: IpAddr,
}

#[derive(Debug, Serialize)]
pub struct Created {
    pub id: String,
    /// Secret qui permet à la personne d'annuler sa demande.
    pub token: String,
    /// Point du tracé où la navette s'arrêtera.
    pub lat: f64,
    pub lon: f64,
}

/// Vu par le public : de quoi suivre sa propre demande, sans aucune position.
#[derive(Debug, Serialize)]
pub struct PublicPickup {
    id: String,
    count: u32,
    dest: String,
    /// Temps avant le passage de la navette (après son départ si elle attend à quai).
    eta_s: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct PublicClosed {
    id: String,
    outcome: Outcome,
}

/// Vu par l'équipage.
#[derive(Debug, Serialize)]
pub struct CrewPickup {
    id: String,
    lat: f64,
    lon: f64,
    count: u32,
    dest: String,
    dest_name: String,
    created: i64,
    /// Distance devant la navette, le long du tracé.
    distance_m: Option<f64>,
    eta_s: Option<f64>,
}

impl Tracker {
    pub fn request_pickup(&mut self, req: PickupRequest, now: i64) -> Result<Created, String> {
        let cfg = self.pickups_cfg.clone();
        if !cfg.enabled {
            return Err("Les demandes de prise en charge sont désactivées.".into());
        }
        if !matches!(
            self.status(now),
            Status::Moving | Status::AtStop | Status::Departing | Status::Stale
        ) {
            return Err("La navette ne circule pas en ce moment.".into());
        }
        if !(1..=cfg.max_group).contains(&req.count) {
            return Err(format!("Groupe de 1 à {} personnes.", cfg.max_group));
        }
        let dest = self
            .stops
            .iter()
            .position(|st| st.cfg.id == req.dest)
            .ok_or("Destination inconnue.")?;
        if let Some(acc) = req.accuracy.filter(|a| *a > MAX_ACCURACY_M) {
            return Err(format!(
                "Position trop imprécise (± {} m) : réessaie à découvert.",
                acc.round()
            ));
        }
        if let Some((st, d)) = self
            .stops
            .iter()
            .map(|st| (st, haversine(req.pos, st.pos())))
            .filter(|&(_, d)| d < cfg.min_stop_distance_m)
            .min_by(|a, b| a.1.total_cmp(&b.1))
        {
            return Err(format!(
                "Tu es à {} m de l'arrêt {} : attends la navette là-bas.",
                round10(d),
                st.cfg.name
            ));
        }

        // Sur une route empruntée à l'aller et au retour, on garde le passage
        // qui mène le plus vite à la destination.
        let dest_s = self.stops[dest].s;
        let best = self
            .route
            .candidates(req.pos, cfg.max_distance_m)
            .into_iter()
            .min_by(|a, b| {
                let da = self.route.deltas(a.s, dest_s).0;
                let db = self.route.deltas(b.s, dest_s).0;
                da.total_cmp(&db)
            })
            .ok_or_else(|| {
                format!(
                    "Tu es à {} m du trajet de la navette : rejoins la route ou un arrêt.",
                    round10(self.route.nearest(req.pos).dist)
                )
            })?;

        self.pickup_requests.retain(|&(_, t)| now - t < RATE_WINDOW_MS);
        let recent = self.pickup_requests.iter().filter(|(ip, _)| *ip == req.ip).count();
        if recent >= MAX_REQUESTS_PER_IP {
            return Err("Trop de demandes depuis ce réseau : réessaie dans quelques minutes.".into());
        }
        let others = self.pickups.iter().filter(|p| p.device != req.device);
        let (same_ip, total) = others.fold((0, 0), |(ip, n), p| (ip + usize::from(p.ip == req.ip), n + 1));
        if same_ip >= MAX_ACTIVE_PER_IP || total >= cfg.max_active {
            return Err("Trop de demandes en cours : rends-toi à un arrêt.".into());
        }
        // Une seule demande par téléphone : la nouvelle remplace l'ancienne.
        if let Some(i) = self.pickups.iter().position(|p| p.device == req.device) {
            let old = self.pickups.remove(i);
            self.close(old.id, Outcome::Cancelled, now);
        }

        self.pickup_requests.push((req.ip, now));
        let pickup = Pickup {
            id: random_hex(1),
            token: random_hex(2),
            device: req.device.to_owned(),
            ip: req.ip,
            s: best.s,
            point: best.point,
            count: req.count,
            dest,
            created: now,
        };
        let created = Created {
            id: pickup.id.clone(),
            token: pickup.token.clone(),
            lat: pickup.point.lat,
            lon: pickup.point.lon,
        };
        self.pickups.push(pickup);
        Ok(created)
    }

    /// Annulation par la personne qui a fait la demande.
    pub fn cancel_pickup(&mut self, id: &str, token: &str, now: i64) -> bool {
        let Some(i) = self.pickups.iter().position(|p| p.id == id && p.token == token) else {
            return false;
        };
        let p = self.pickups.remove(i);
        self.close(p.id, Outcome::Cancelled, now);
        true
    }

    /// « Pris » ou « ignoré » par le chauffeur ou l'orga.
    pub fn resolve_pickup(&mut self, id: &str, action: PickupAction, now: i64) -> bool {
        let Some(i) = self.pickups.iter().position(|p| p.id == id) else {
            return false;
        };
        let p = self.pickups.remove(i);
        let outcome = match action {
            PickupAction::Picked => {
                self.set_passengers(self.passengers.unwrap_or(0) + p.count);
                Outcome::Picked
            }
            PickupAction::Dismissed => Outcome::Dismissed,
        };
        self.close(p.id, outcome, now);
        true
    }

    /// Clôt les demandes dont la navette vient de dépasser le point.
    pub(super) fn close_passed_pickups(&mut self, from: f64, to: f64, now: i64) {
        let (moved, _) = self.route.deltas(from, to);
        if moved <= 0.0 || moved > MAX_PASS_JUMP_M {
            return;
        }
        let route = self.route.clone();
        let (passed, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.pickups)
            .into_iter()
            .partition(|p| {
                let mut target = p.s + PASS_MARGIN_M;
                if route.closed {
                    target = target.rem_euclid(route.length);
                }
                route.deltas(from, target).0 <= moved
            });
        self.pickups = kept;
        for p in passed {
            self.close(p.id, Outcome::Passed, now);
        }
    }

    /// Ménage périodique. Renvoie `true` si quelque chose a changé.
    pub fn expire_pickups(&mut self, now: i64) -> bool {
        let max_age = self.pickups_cfg.expire_after_min * 60_000;
        let off = self.service == ServiceMode::Off;
        let before = (self.pickups.len(), self.closed_pickups.len());
        let (expired, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.pickups)
            .into_iter()
            .partition(|p| off || now - p.created >= max_age);
        self.pickups = kept;
        for p in expired {
            let outcome = if off { Outcome::Dismissed } else { Outcome::Expired };
            self.close(p.id, outcome, now);
        }
        self.closed_pickups.retain(|c| now - c.ts < CLOSED_KEEP_MS);
        before != (self.pickups.len(), self.closed_pickups.len())
    }

    fn close(&mut self, id: String, outcome: Outcome, now: i64) {
        self.closed_pickups.push(ClosedPickup { id, outcome, ts: now });
    }

    pub(super) fn public_pickups(&self, now: i64) -> Vec<PublicPickup> {
        let live = self.live_snap(now);
        let at = self.arrival.map(|(i, _)| i);
        self.pickups
            .iter()
            .map(|p| PublicPickup {
                id: p.id.clone(),
                count: p.count,
                dest: self.stops[p.dest].cfg.id.clone(),
                eta_s: live
                    .and_then(|snap| self.travel(snap.s, p.s, at))
                    .map(|(_, t)| t.round()),
            })
            .collect()
    }

    pub(super) fn public_closed(&self) -> Vec<PublicClosed> {
        self.closed_pickups
            .iter()
            .map(|c| PublicClosed {
                id: c.id.clone(),
                outcome: c.outcome,
            })
            .collect()
    }

    /// Demandes en cours, de la plus proche devant la navette à la plus lointaine.
    pub fn crew_pickups(&self, now: i64) -> Vec<CrewPickup> {
        let live = self.live_snap(now);
        let at = self.arrival.map(|(i, _)| i);
        let mut list: Vec<CrewPickup> = self
            .pickups
            .iter()
            .map(|p| {
                let travel = live.and_then(|snap| self.travel(snap.s, p.s, at));
                CrewPickup {
                    id: p.id.clone(),
                    lat: p.point.lat,
                    lon: p.point.lon,
                    count: p.count,
                    dest: self.stops[p.dest].cfg.id.clone(),
                    dest_name: self.stops[p.dest].cfg.name.clone(),
                    created: p.created,
                    distance_m: travel.map(|(d, _)| d.round()),
                    eta_s: travel.map(|(_, t)| t.round()),
                }
            })
            .collect();
        list.sort_by(|a, b| {
            let key = |p: &CrewPickup| p.distance_m.unwrap_or(f64::INFINITY);
            key(a).total_cmp(&key(b))
        });
        list
    }
}

fn round10(m: f64) -> i64 {
    ((m / 10.0).round() * 10.0) as i64
}

/// Identifiants imprévisibles sans dépendance : `RandomState` tire des clés
/// aléatoires du système à chaque appel.
fn random_hex(words: usize) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    (0..words)
        .map(|i| {
            let mut h = RandomState::new().build_hasher();
            h.write_u128(nanos);
            h.write_usize(i);
            format!("{:016x}", h.finish())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::tests::{LAT, LON, fix, shuttle};
    use super::*;

    const IP: IpAddr = IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);

    /// Au milieu de la rue, 10 m à l'est du tracé.
    fn midway() -> LatLon {
        LatLon {
            lat: LAT + 0.0025,
            lon: LON + 10.0 / (111_195.0 * LAT.to_radians().cos()),
        }
    }

    fn request<'a>(pos: LatLon, dest: &'a str, device: &'a str) -> PickupRequest<'a> {
        PickupRequest {
            pos,
            accuracy: Some(10.0),
            count: 2,
            dest,
            device,
            ip: IP,
        }
    }

    fn running() -> Tracker {
        let mut t = shuttle();
        t.ingest(fix(0.0, 0.0, 1_000), 1_000);
        t
    }

    #[test]
    fn refused_when_not_running_near_a_stop_or_off_route() {
        let mut t = shuttle();
        assert!(t.request_pickup(request(midway(), "festival", "a"), 1_000).is_err());

        let mut t = running();
        let near_stop = LatLon { lat: LAT + 0.0005, lon: LON };
        let err = t.request_pickup(request(near_stop, "festival", "a"), 2_000).unwrap_err();
        assert!(err.contains("Gare"), "{err}");

        let far = LatLon { lat: LAT + 0.0025, lon: LON + 0.003 };
        let err = t.request_pickup(request(far, "festival", "a"), 2_000).unwrap_err();
        assert!(err.contains("trajet"), "{err}");
    }

    #[test]
    fn destination_picks_the_leg() {
        let mut t = running();
        t.request_pickup(request(midway(), "festival", "a"), 2_000).unwrap();
        t.request_pickup(request(midway(), "gare", "b"), 2_000).unwrap();
        assert!((t.pickups[0].s - 278.0).abs() < 5.0, "aller : {}", t.pickups[0].s);
        assert!((t.pickups[1].s - 832.0).abs() < 5.0, "retour : {}", t.pickups[1].s);
    }

    #[test]
    fn one_request_per_phone() {
        let mut t = running();
        let first = t.request_pickup(request(midway(), "festival", "a"), 2_000).unwrap();
        t.request_pickup(request(midway(), "gare", "a"), 3_000).unwrap();
        assert_eq!(t.pickups.len(), 1);
        assert_eq!(t.closed_pickups[0].id, first.id);
        assert_eq!(t.closed_pickups[0].outcome, Outcome::Cancelled);
    }

    #[test]
    fn closed_when_the_bus_passes_or_too_old() {
        let mut t = running();
        let up = t.request_pickup(request(midway(), "festival", "a"), 2_000).unwrap();
        let down = t.request_pickup(request(midway(), "gare", "b"), 2_000).unwrap();

        // La navette monte vers le festival : seule la demande « aller » est servie.
        t.ingest(fix(0.003, 0.0, 30_000), 30_000);
        assert_eq!(t.pickups.len(), 1);
        assert_eq!(t.pickups[0].id, down.id);
        assert_eq!(t.closed_pickups[0].id, up.id);
        assert_eq!(t.closed_pickups[0].outcome, Outcome::Passed);

        assert!(t.expire_pickups(2_000 + 20 * 60_000));
        assert!(t.pickups.is_empty());
    }

    #[test]
    fn picked_adds_passengers() {
        let mut t = running();
        let p = t.request_pickup(request(midway(), "festival", "a"), 2_000).unwrap();
        t.set_passengers(5);
        assert!(t.resolve_pickup(&p.id, PickupAction::Picked, 3_000));
        assert_eq!(t.passengers, Some(7));
        assert!(!t.cancel_pickup(&p.id, &p.token, 4_000), "déjà close");
    }
}
