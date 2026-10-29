//! Mode démo (`--simulate`) : une fausse navette parcourt le tracé, avec du
//! bruit GPS et des arrêts, en passant par le même chemin que les vraies positions.

use std::time::Duration;

use tracing::info;

use crate::geo::LatLon;
use crate::state::{AppState, now_ms};
use crate::tracker::Fix;

const SPEED_MPS: f64 = 8.0;
const STEP_S: u64 = 2;
/// Attente max à un arrêt, si la navette ne se remplit pas avant.
const DWELL_STEPS: u32 = 20;

pub async fn run(state: AppState) {
    let capacity = state.cfg.shuttle.capacity;
    info!("simulation : navette fictive à {:.0} km/h", SPEED_MPS * 3.6);

    let mut s = state.tracker().stop_positions().next().map_or(0.0, |(_, s)| s);
    let mut wait = DWELL_STEPS;
    let mut aboard = 0;
    let mut rng = Lcg(0x5eed_cafe);
    let mut tick = tokio::time::interval(Duration::from_secs(STEP_S));
    state.set_passengers(0);

    loop {
        tick.tick().await;
        // Relu à chaque pas : l'orga peut changer le tracé en cours de route.
        let route = state.route();
        let stops: Vec<f64> = state.tracker().stop_positions().map(|(_, s)| s).collect();
        s = s.min(route.length);
        if wait > 0 {
            // Des festivaliers montent ; départ dès que c'est plein, ou au bout de l'attente.
            wait -= 1;
            if rng.next() > 0.2 {
                aboard = (aboard + 1 + (rng.next().abs() * 2.0) as u32).min(capacity);
                state.set_passengers(aboard);
            }
            if aboard == capacity {
                wait = 0;
            }
            if wait == 0 {
                state.depart();
            }
        } else {
            let step = SPEED_MPS * STEP_S as f64;
            let next_stop = stops
                .iter()
                .map(|&st| (st, route.deltas(s, st).0))
                .filter(|&(_, d)| d > 0.0 && d <= step)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            match next_stop {
                Some((st, _)) => {
                    // Le serveur remet le compteur à zéro à l'arrivée : tout le monde descend.
                    s = st;
                    wait = DWELL_STEPS;
                    aboard = 0;
                }
                None if route.closed => s = (s + step).rem_euclid(route.length),
                None if s + step >= route.length => s = 0.0,
                None => s += step,
            }
        }

        let (p, bearing) = route.point_at(s);
        // ±4 m de bruit, comme un vrai GPS de téléphone.
        let (dx, dy) = (rng.next() * 4.0, rng.next() * 4.0);
        let pos = LatLon {
            lat: p.lat + dy / 111_195.0,
            lon: p.lon + dx / (111_195.0 * p.lat.to_radians().cos()),
        };
        state.ingest(Fix {
            pos,
            accuracy: Some(6.0),
            speed: Some(if wait > 0 { 0.0 } else { SPEED_MPS }),
            heading: Some(bearing),
            ts: now_ms(),
            battery: Some(87.0),
        });
    }
}

/// Générateur pseudo-aléatoire minimal, valeurs dans [-1, 1].
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
    }
}
