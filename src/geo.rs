//! Géométrie du tracé : projection locale en mètres, accrochage d'une
//! position GPS au tracé et abscisse curviligne (distance parcourue).

use std::path::Path;

use anyhow::{Context, bail};
use serde::Serialize;

const EARTH_RADIUS_M: f64 = 6_371_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct LatLon {
    pub lat: f64,
    pub lon: f64,
}

/// Distance orthodromique en mètres.
pub fn haversine(a: LatLon, b: LatLon) -> f64 {
    let (la1, la2) = (a.lat.to_radians(), b.lat.to_radians());
    let dla = la2 - la1;
    let dlo = (b.lon - a.lon).to_radians();
    let h = (dla / 2.0).sin().powi(2) + la1.cos() * la2.cos() * (dlo / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_M * h.sqrt().asin()
}

/// Écart absolu entre deux caps, en degrés (0..=180).
pub fn angle_diff(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(360.0);
    if d > 180.0 { 360.0 - d } else { d }
}

/// Projection équirectangulaire autour d'un point de référence. Largement
/// assez précise pour un tracé de quelques kilomètres.
#[derive(Debug, Clone, Copy)]
struct Projector {
    lat0: f64,
    lon0: f64,
    kx: f64,
    ky: f64,
}

impl Projector {
    fn new(origin: LatLon) -> Self {
        let ky = EARTH_RADIUS_M * std::f64::consts::PI / 180.0;
        Self {
            lat0: origin.lat,
            lon0: origin.lon,
            kx: ky * origin.lat.to_radians().cos(),
            ky,
        }
    }

    fn to_xy(self, p: LatLon) -> (f64, f64) {
        ((p.lon - self.lon0) * self.kx, (p.lat - self.lat0) * self.ky)
    }

    fn to_latlon(self, x: f64, y: f64) -> LatLon {
        LatLon {
            lat: self.lat0 + y / self.ky,
            lon: self.lon0 + x / self.kx,
        }
    }
}

/// Résultat de la projection d'un point sur un segment du tracé.
#[derive(Debug, Clone, Copy)]
pub struct Projection {
    /// Distance depuis le début du tracé, en mètres.
    pub s: f64,
    /// Distance entre le point et le tracé, en mètres.
    pub dist: f64,
    /// Cap du segment (0 = nord, sens horaire).
    pub bearing: f64,
    pub point: LatLon,
}

#[derive(Debug)]
pub struct Route {
    proj: Projector,
    xy: Vec<(f64, f64)>,
    coords: Vec<LatLon>,
    cum: Vec<f64>,
    pub length: f64,
    /// Une boucle (aller + retour) : la fin rejoint le début.
    pub closed: bool,
}

impl Route {
    pub fn new(coords: Vec<LatLon>) -> anyhow::Result<Self> {
        if coords.len() < 2 {
            bail!("le tracé doit contenir au moins 2 points");
        }
        let proj = Projector::new(coords[0]);
        let xy: Vec<_> = coords.iter().map(|&c| proj.to_xy(c)).collect();
        let mut cum = Vec::with_capacity(xy.len());
        let mut acc = 0.0;
        cum.push(0.0);
        for w in xy.windows(2) {
            acc += (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1);
            cum.push(acc);
        }
        let closed = haversine(coords[0], coords[coords.len() - 1]) < 30.0;
        Ok(Self {
            proj,
            xy,
            coords,
            cum,
            length: acc,
            closed,
        })
    }

    /// Lit un GeoJSON contenant une `LineString` (Feature, FeatureCollection ou géométrie nue).
    pub fn from_geojson_file(path: &Path) -> anyhow::Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("lecture de {}", path.display()))?;
        // Windows PowerShell 5.1 écrit l'UTF-8 avec BOM.
        let v: serde_json::Value = serde_json::from_str(raw.trim_start_matches('\u{feff}'))
            .with_context(|| format!("JSON invalide dans {}", path.display()))?;
        let line = find_linestring(&v).context("aucune LineString trouvée dans le GeoJSON")?;
        let coords = line
            .iter()
            .map(|c| {
                let lon = c.get(0).and_then(|x| x.as_f64());
                let lat = c.get(1).and_then(|x| x.as_f64());
                match (lat, lon) {
                    (Some(lat), Some(lon)) => Ok(LatLon { lat, lon }),
                    _ => bail!("coordonnée invalide : {c}"),
                }
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        Self::new(coords)
    }

    pub fn coords(&self) -> &[LatLon] {
        &self.coords
    }

    /// Toutes les projections (une par segment) à moins de `max_dist` mètres.
    pub fn candidates(&self, p: LatLon, max_dist: f64) -> Vec<Projection> {
        let (px, py) = self.proj.to_xy(p);
        (0..self.xy.len() - 1)
            .map(|i| self.project_on_segment(i, px, py))
            .filter(|c| c.dist <= max_dist)
            .collect()
    }

    /// La projection la plus proche sur l'ensemble du tracé.
    pub fn nearest(&self, p: LatLon) -> Projection {
        let (px, py) = self.proj.to_xy(p);
        (0..self.xy.len() - 1)
            .map(|i| self.project_on_segment(i, px, py))
            .min_by(|a, b| a.dist.total_cmp(&b.dist))
            .expect("tracé non vide")
    }

    fn project_on_segment(&self, i: usize, px: f64, py: f64) -> Projection {
        let (ax, ay) = self.xy[i];
        let (bx, by) = self.xy[i + 1];
        let (dx, dy) = (bx - ax, by - ay);
        let len2 = dx * dx + dy * dy;
        let t = if len2 > 0.0 {
            (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (qx, qy) = (ax + t * dx, ay + t * dy);
        Projection {
            s: self.cum[i] + t * len2.sqrt(),
            dist: (px - qx).hypot(py - qy),
            bearing: dx.atan2(dy).to_degrees().rem_euclid(360.0),
            point: self.proj.to_latlon(qx, qy),
        }
    }

    /// Distances (en avant, en arrière) pour aller de `from` à `to` sur le
    /// tracé. Sur un tracé ouvert, la direction impossible vaut l'infini.
    pub fn deltas(&self, from: f64, to: f64) -> (f64, f64) {
        if self.closed {
            let fwd = (to - from).rem_euclid(self.length);
            (fwd, (self.length - fwd) % self.length)
        } else if to >= from {
            (to - from, f64::INFINITY)
        } else {
            (f64::INFINITY, from - to)
        }
    }

    /// Point et cap du tracé à la distance `s` du départ.
    pub fn point_at(&self, s: f64) -> (LatLon, f64) {
        let s = if self.closed {
            s.rem_euclid(self.length)
        } else {
            s.clamp(0.0, self.length)
        };
        let i = match self.cum.binary_search_by(|c| c.total_cmp(&s)) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        }
        .min(self.xy.len() - 2);
        let seg = self.cum[i + 1] - self.cum[i];
        let t = if seg > 0.0 { (s - self.cum[i]) / seg } else { 0.0 };
        let (ax, ay) = self.xy[i];
        let (bx, by) = self.xy[i + 1];
        let point = self.proj.to_latlon(ax + t * (bx - ax), ay + t * (by - ay));
        let bearing = (bx - ax).atan2(by - ay).to_degrees().rem_euclid(360.0);
        (point, bearing)
    }
}

fn find_linestring(v: &serde_json::Value) -> Option<&Vec<serde_json::Value>> {
    match v.get("type")?.as_str()? {
        "LineString" => v.get("coordinates")?.as_array(),
        "Feature" => find_linestring(v.get("geometry")?),
        "FeatureCollection" => v
            .get("features")?
            .as_array()?
            .iter()
            .find_map(find_linestring),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Route {
        // Boucle d'environ 4 × 111 m autour de Sion.
        let d = 0.001;
        let (lat, lon): (f64, f64) = (46.2277, 7.3584);
        let k = d / lat.to_radians().cos();
        Route::new(vec![
            LatLon { lat, lon },
            LatLon { lat: lat + d, lon },
            LatLon { lat: lat + d, lon: lon + k },
            LatLon { lat, lon: lon + k },
            LatLon { lat, lon },
        ])
        .unwrap()
    }

    #[test]
    fn loop_detection_and_length() {
        let r = square();
        assert!(r.closed);
        assert!((r.length - 4.0 * 111.2).abs() < 2.0, "{}", r.length);
    }

    #[test]
    fn projection_on_first_segment() {
        let r = square();
        let p = r.nearest(LatLon { lat: 46.2282, lon: 7.35835 });
        assert!((p.s - 55.6).abs() < 1.0, "s = {}", p.s);
        assert!(p.dist < 5.0);
        assert!(angle_diff(p.bearing, 0.0) < 1.0);
    }

    #[test]
    fn deltas_wrap_on_loop() {
        let r = square();
        let (fwd, back) = r.deltas(r.length - 10.0, 5.0);
        assert!((fwd - 15.0).abs() < 1e-6);
        assert!((back - (r.length - 15.0)).abs() < 1e-6);
    }

    #[test]
    fn point_at_roundtrip() {
        let r = square();
        let (p, bearing) = r.point_at(200.0);
        assert!((r.nearest(p).s - 200.0).abs() < 0.5);
        assert!(angle_diff(bearing, 90.0) < 1.0);
    }

    #[test]
    fn angle_diff_wraps() {
        assert_eq!(angle_diff(350.0, 10.0), 20.0);
        assert_eq!(angle_diff(90.0, 270.0), 180.0);
    }
}
