//! « Demander de l'aide » : une personne en danger près du trajet prévient
//! l'équipage (chauffeur et orga), qui essaie de venir. Sa position exacte
//! n'est montrée qu'à l'équipage, jamais au public. L'alerte disparaît quand
//! l'équipage la clôt, quand la personne l'annule, ou au bout de deux heures.
//! Ça ne remplace pas les secours : la page publique renvoie d'abord au 117.

use std::net::IpAddr;

use serde::{Deserialize, Serialize};

use super::pickups::random_hex;
use super::{ServiceMode, Tracker};
use crate::geo::{LatLon, haversine};

const RATE_WINDOW_MS: i64 = 10 * 60_000;
const MAX_REQUESTS_PER_IP: usize = 4;
const MAX_ACTIVE_PER_IP: usize = 2;
const MAX_ACTIVE: usize = 20;
const EXPIRE_MS: i64 = 2 * 60 * 60_000;
/// Combien de temps la personne voit que son alerte a été close.
const CLOSED_KEEP_MS: i64 = 30 * 60_000;
/// Au-delà, l'équipage ne peut pas venir à pied ou en navette.
const MAX_ROUTE_DISTANCE_M: f64 = 2_000.0;
const MAX_NOTE_CHARS: usize = 200;

pub(super) struct Alert {
    id: String,
    token: String,
    device: String,
    ip: IpAddr,
    pos: LatLon,
    accuracy: Option<f64>,
    note: String,
    created: i64,
    /// Heure à laquelle quelqu'un de l'équipage a répondu « j'y vais ».
    seen: Option<i64>,
}

pub(super) struct ClosedAlert {
    id: String,
    token: String,
    ts: i64,
}

/// Ce que l'équipage fait d'une alerte.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertAction {
    /// « J'y vais » : la personne voit que quelqu'un arrive.
    Seen,
    Closed,
}

/// Où en est une alerte, vu par la personne qui l'a envoyée.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertState {
    Open,
    Seen,
    Closed,
}

pub struct AlertRequest<'a> {
    pub pos: LatLon,
    pub accuracy: Option<f64>,
    pub note: &'a str,
    pub device: &'a str,
    pub ip: IpAddr,
}

#[derive(Debug, Serialize)]
pub struct AlertCreated {
    pub id: String,
    /// Secret qui permet à la personne de suivre et d'annuler son alerte.
    pub token: String,
}

/// Vu par l'équipage seulement.
#[derive(Debug, Serialize)]
pub struct CrewAlert {
    id: String,
    lat: f64,
    lon: f64,
    accuracy: Option<f64>,
    note: String,
    created: i64,
    seen: Option<i64>,
    /// Distance au trajet de la navette.
    route_m: f64,
    /// Distance à vol d'oiseau depuis la dernière position de la navette.
    bus_m: Option<f64>,
}

impl Tracker {
    pub fn request_alert(&mut self, req: AlertRequest, now: i64) -> Result<AlertCreated, String> {
        if self.service == ServiceMode::Off {
            return Err("La navette est hors service : appelle le 117 (police) ou le 144 (ambulance).".into());
        }
        let route_m = self.route.nearest(req.pos).dist;
        if route_m > MAX_ROUTE_DISTANCE_M {
            return Err(
                "Tu es trop loin du trajet pour que l'équipe vienne : appelle le 117 (police) ou le 144 (ambulance)."
                    .into(),
            );
        }

        self.alert_requests.retain(|&(_, t)| now - t < RATE_WINDOW_MS);
        if self.alert_requests.iter().filter(|(ip, _)| *ip == req.ip).count() >= MAX_REQUESTS_PER_IP {
            return Err("Trop d'alertes depuis ce réseau : appelle le 117.".into());
        }
        let others = self.alerts.iter().filter(|a| a.device != req.device);
        let (same_ip, total) = others.fold((0, 0), |(ip, n), a| (ip + usize::from(a.ip == req.ip), n + 1));
        if same_ip >= MAX_ACTIVE_PER_IP || total >= MAX_ACTIVE {
            return Err("Trop d'alertes en cours : appelle le 117.".into());
        }
        // Une seule alerte par téléphone : la nouvelle (position à jour) remplace l'ancienne.
        if let Some(i) = self.alerts.iter().position(|a| a.device == req.device) {
            let old = self.alerts.remove(i);
            self.close_alert(old, now);
        }

        self.alert_requests.push((req.ip, now));
        let alert = Alert {
            id: random_hex(1),
            token: random_hex(2),
            device: req.device.to_owned(),
            ip: req.ip,
            pos: req.pos,
            accuracy: req.accuracy,
            note: req.note.trim().chars().take(MAX_NOTE_CHARS).collect(),
            created: now,
            seen: None,
        };
        let created = AlertCreated {
            id: alert.id.clone(),
            token: alert.token.clone(),
        };
        self.alerts.push(alert);
        Ok(created)
    }

    pub fn alert_state(&self, id: &str, token: &str) -> Option<AlertState> {
        if let Some(a) = self.alerts.iter().find(|a| a.id == id && a.token == token) {
            return Some(if a.seen.is_some() { AlertState::Seen } else { AlertState::Open });
        }
        self.closed_alerts
            .iter()
            .any(|c| c.id == id && c.token == token)
            .then_some(AlertState::Closed)
    }

    /// Annulation par la personne qui a envoyé l'alerte.
    pub fn cancel_alert(&mut self, id: &str, token: &str, now: i64) -> bool {
        let Some(i) = self.alerts.iter().position(|a| a.id == id && a.token == token) else {
            return false;
        };
        let a = self.alerts.remove(i);
        self.close_alert(a, now);
        true
    }

    /// « J'y vais » ou « clore » par le chauffeur ou l'orga.
    pub fn resolve_alert(&mut self, id: &str, action: AlertAction, now: i64) -> bool {
        let Some(i) = self.alerts.iter().position(|a| a.id == id) else {
            return false;
        };
        match action {
            AlertAction::Seen => {
                self.alerts[i].seen.get_or_insert(now);
            }
            AlertAction::Closed => {
                let a = self.alerts.remove(i);
                self.close_alert(a, now);
            }
        }
        true
    }

    /// Ménage périodique.
    pub fn expire_alerts(&mut self, now: i64) {
        let (expired, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.alerts)
            .into_iter()
            .partition(|a| now - a.created >= EXPIRE_MS);
        self.alerts = kept;
        for a in expired {
            self.close_alert(a, now);
        }
        self.closed_alerts.retain(|c| now - c.ts < CLOSED_KEEP_MS);
    }

    fn close_alert(&mut self, a: Alert, now: i64) {
        self.closed_alerts.push(ClosedAlert {
            id: a.id,
            token: a.token,
            ts: now,
        });
    }

    /// Alertes en cours, la plus récente d'abord.
    pub fn crew_alerts(&self) -> Vec<CrewAlert> {
        let bus = self.last_fix.map(|f| f.pos);
        let mut list: Vec<CrewAlert> = self
            .alerts
            .iter()
            .map(|a| CrewAlert {
                id: a.id.clone(),
                lat: a.pos.lat,
                lon: a.pos.lon,
                accuracy: a.accuracy.map(f64::round),
                note: a.note.clone(),
                created: a.created,
                seen: a.seen,
                route_m: self.route.nearest(a.pos).dist.round(),
                bus_m: bus.map(|b| haversine(b, a.pos).round()),
            })
            .collect();
        list.sort_by_key(|a| std::cmp::Reverse(a.created));
        list
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{LAT, LON, fix, shuttle};
    use super::*;

    const IP: IpAddr = IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);

    fn request(pos: LatLon, device: &str) -> AlertRequest<'_> {
        AlertRequest {
            pos,
            accuracy: Some(15.0),
            note: "  devant le kiosque  ",
            device,
            ip: IP,
        }
    }

    fn near() -> LatLon {
        LatLon { lat: LAT + 0.002, lon: LON + 0.002 }
    }

    #[test]
    fn refused_far_away_or_when_off() {
        let mut t = shuttle();
        let far = LatLon { lat: LAT + 0.05, lon: LON };
        assert!(t.request_alert(request(far, "a"), 1_000).unwrap_err().contains("117"));
        t.service = ServiceMode::Off;
        assert!(t.request_alert(request(near(), "a"), 1_000).unwrap_err().contains("117"));
    }

    #[test]
    fn crew_sees_it_the_public_never_does() {
        let mut t = shuttle();
        t.ingest(fix(0.0, 0.0, 1_000), 1_000);
        let a = t.request_alert(request(near(), "a"), 2_000).unwrap();
        let crew = t.crew_alerts();
        assert_eq!(crew.len(), 1);
        assert_eq!(crew[0].note, "devant le kiosque");
        assert!(crew[0].bus_m.unwrap() > 100.0);
        let public = serde_json::to_string(&t.snapshot(2_000)).unwrap();
        assert!(!public.contains(&a.id), "l'alerte ne doit pas être diffusée");
    }

    #[test]
    fn seen_then_closed_then_forgotten() {
        let mut t = shuttle();
        let a = t.request_alert(request(near(), "a"), 1_000).unwrap();
        assert_eq!(t.alert_state(&a.id, &a.token), Some(AlertState::Open));
        assert_eq!(t.alert_state(&a.id, "faux"), None);
        assert!(t.resolve_alert(&a.id, AlertAction::Seen, 2_000));
        assert_eq!(t.alert_state(&a.id, &a.token), Some(AlertState::Seen));
        assert!(t.resolve_alert(&a.id, AlertAction::Closed, 3_000));
        assert_eq!(t.alert_state(&a.id, &a.token), Some(AlertState::Closed));
        t.expire_alerts(3_000 + CLOSED_KEEP_MS);
        assert_eq!(t.alert_state(&a.id, &a.token), None);
    }

    #[test]
    fn one_alert_per_phone_and_expiry() {
        let mut t = shuttle();
        let first = t.request_alert(request(near(), "a"), 1_000).unwrap();
        t.request_alert(request(near(), "a"), 2_000).unwrap();
        assert_eq!(t.alerts.len(), 1);
        assert_eq!(t.alert_state(&first.id, &first.token), Some(AlertState::Closed));
        t.expire_alerts(2_000 + EXPIRE_MS);
        assert!(t.alerts.is_empty());
    }
}
