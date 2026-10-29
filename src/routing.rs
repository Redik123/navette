//! Trajet modifiable depuis la page orga : les points de passage de l'aller
//! deviennent un itinéraire routier (OSRM), avec le retour par le même chemin.
//! Le tracé choisi est gardé dans `data/route.json`, et survit aux redémarrages.

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::{RouteEditorConfig, StopConfig};
use crate::geo::{LatLon, Route};

const MAX_VIA: usize = 25;
const MIN_LENGTH_M: f64 = 300.0;
/// Un détour plus long que ça par rapport au trajet d'origine vient sans doute
/// d'un point mal placé.
const MAX_LENGTH_RATIO: f64 = 3.0;
/// Un arrêt plus loin que ça du tracé n'aurait plus de temps d'arrivée fiable.
const MIN_STOP_TOLERANCE_M: f64 = 60.0;

/// Trajet modifié, tel qu'enregistré sur disque.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomRoute {
    pub via: Vec<[f64; 2]>,
    /// [lat, lon]
    pub coords: Vec<[f64; 2]>,
}

impl CustomRoute {
    pub fn load(path: &Path) -> Option<Self> {
        let raw = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&raw).ok()
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(self)?)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }

    pub fn route(&self) -> anyhow::Result<Route> {
        Route::new(self.coords.iter().map(|&[lat, lon]| LatLon { lat, lon }).collect())
    }
}

#[derive(Deserialize)]
struct OsrmResponse {
    code: String,
    message: Option<String>,
    #[serde(default)]
    routes: Vec<OsrmRoute>,
}

#[derive(Deserialize)]
struct OsrmRoute {
    geometry: OsrmGeometry,
}

#[derive(Deserialize)]
struct OsrmGeometry {
    /// [lon, lat]
    coordinates: Vec<[f64; 2]>,
}

/// Boucle routière : premier arrêt → points de passage → deuxième arrêt → même
/// chemin à l'envers → premier arrêt. Renvoie les points [lat, lon].
pub async fn compute(cfg: &RouteEditorConfig, stops: &[StopConfig], via: &[[f64; 2]]) -> Result<Vec<[f64; 2]>, String> {
    let [a, b] = stops else {
        return Err("L'éditeur de trajet ne gère que deux arrêts.".into());
    };
    if via.len() > MAX_VIA {
        return Err(format!("{MAX_VIA} points de passage au maximum."));
    }
    if via.iter().any(|&[lat, lon]| !lat.is_finite() || !lon.is_finite() || lat.abs() > 90.0 || lon.abs() > 180.0) {
        return Err("Point de passage invalide.".into());
    }
    let mut points = vec![[a.lat, a.lon]];
    points.extend_from_slice(via);
    points.push([b.lat, b.lon]);
    points.extend(via.iter().rev());
    points.push([a.lat, a.lon]);
    let path = points
        .iter()
        .map(|[lat, lon]| format!("{lon:.6},{lat:.6}"))
        .collect::<Vec<_>>()
        .join(";");
    // continue_straight=false : demi-tour permis aux points, comme à l'Usine.
    let url = format!(
        "{}/route/v1/driving/{path}?overview=full&geometries=geojson&continue_straight=false",
        cfg.osrm_url.trim_end_matches('/')
    );

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent("navette-ragnarok (https://navette.ragnarok-festival.ch)")
        .build()
        .map_err(|e| format!("client HTTP : {e}"))?;
    let unreachable = |_| "Le service d'itinéraires ne répond pas : réessaie dans un instant.".to_string();
    let body = client
        .get(&url)
        .send()
        .await
        .map_err(unreachable)?
        .bytes()
        .await
        .map_err(unreachable)?;
    // OSRM répond en JSON même en cas d'erreur (code « NoRoute », etc.).
    let res: OsrmResponse =
        serde_json::from_slice(&body).map_err(|_| "Réponse illisible du service d'itinéraires.".to_string())?;
    if res.code != "Ok" {
        return Err(format!(
            "Pas d'itinéraire possible ({}). Déplace un point sur une route.",
            res.message.unwrap_or(res.code)
        ));
    }
    let route = res.routes.into_iter().next().ok_or("Pas d'itinéraire trouvé.")?;
    Ok(route
        .geometry
        .coordinates
        .into_iter()
        .map(|[lon, lat]| [round6(lat), round6(lon)])
        .collect())
}

/// Le tracé doit rester une boucle utilisable : longueur plausible par rapport
/// au trajet d'origine, et chaque arrêt dessus.
pub fn check(route: &Route, stops: &[StopConfig], original_m: f64) -> Result<(), String> {
    let km = |m: f64| format!("{:.1}", m / 1000.0).replace('.', ",");
    if route.length < MIN_LENGTH_M {
        return Err(format!("Trajet de {} km : trop court.", km(route.length)));
    }
    if route.length > original_m * MAX_LENGTH_RATIO {
        return Err(format!(
            "Trajet de {} km, plus de trois fois l'original ({} km) : un point est sans doute mal placé.",
            km(route.length),
            km(original_m)
        ));
    }
    for stop in stops {
        let d = route.nearest(LatLon { lat: stop.lat, lon: stop.lon }).dist;
        if d > stop.radius_m.max(MIN_STOP_TOLERANCE_M) {
            return Err(format!(
                "L'arrêt {} est à {:.0} m du trajet : ajoute un point de passage près de lui.",
                stop.name, d
            ));
        }
    }
    Ok(())
}

fn round6(x: f64) -> f64 {
    (x * 1e6).round() / 1e6
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stop(id: &str, lat: f64, lon: f64) -> StopConfig {
        StopConfig {
            id: id.into(),
            name: id.into(),
            lat,
            lon,
            radius_m: 50.0,
            detail: String::new(),
        }
    }

    #[test]
    fn checks_length_and_stops() {
        let stops = [stop("gare", 46.2277, 7.3584), stop("usine", 46.2327, 7.3584)];
        let line = |lon: f64| {
            Route::new(vec![
                LatLon { lat: 46.2277, lon },
                LatLon { lat: 46.2327, lon },
                LatLon { lat: 46.2277, lon },
            ])
            .unwrap()
        };
        assert!(check(&line(7.3584), &stops, 1_100.0).is_ok());
        let err = check(&line(7.3604), &stops, 1_100.0).unwrap_err();
        assert!(err.contains("gare"), "{err}");
        let err = check(&line(7.3584), &stops, 300.0).unwrap_err();
        assert!(err.contains("trois fois"), "{err}");
    }

    #[test]
    fn custom_route_round_trip() {
        let dir = std::env::temp_dir().join(format!("navette-route-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("route.json");
        let custom = CustomRoute {
            via: vec![[46.23, 7.36]],
            coords: vec![[46.2277, 7.3584], [46.2327, 7.3584], [46.2277, 7.3584]],
        };
        custom.save(&path).unwrap();
        let back = CustomRoute::load(&path).unwrap();
        assert_eq!(back.via, custom.via);
        assert!(back.route().unwrap().length > 1000.0);
        std::fs::remove_dir_all(dir).ok();
    }
}
