use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub tracking: TrackingConfig,
    #[serde(default)]
    pub shuttle: ShuttleConfig,
    #[serde(default)]
    pub pickups: PickupsConfig,
    pub event: EventConfig,
    pub stops: Vec<StopConfig>,
    #[serde(default)]
    pub trains: TrainsConfig,
    #[serde(default)]
    pub route_editor: RouteEditorConfig,
}

/// Trajet modifiable depuis la page orga (route barrée, travaux…).
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RouteEditorConfig {
    /// Points de passage de l'aller, du premier au deuxième arrêt ([lat, lon]) ;
    /// le retour reprend le même chemin à l'envers. Ce sont ceux qui ont servi à
    /// générer `route.geojson`.
    pub via: Vec<[f64; 2]>,
    /// Serveur d'itinéraires OSRM (profil voiture).
    pub osrm_url: String,
}

impl Default for RouteEditorConfig {
    fn default() -> Self {
        Self {
            via: Vec::new(),
            osrm_url: "https://router.project-osrm.org".into(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_listen")]
    pub listen: String,
    #[serde(default = "default_static_dir")]
    pub static_dir: PathBuf,
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
    #[serde(default = "default_route_file")]
    pub route_file: PathBuf,
    /// Nombre max de connexions SSE simultanées (protection mémoire).
    #[serde(default = "default_max_viewers")]
    pub max_viewers: usize,
}

#[derive(Debug, Default, Deserialize)]
pub struct AuthConfig {
    pub driver_token: Option<String>,
    pub admin_token: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TrackingConfig {
    /// Vitesse moyenne de la navette, arrêts aux feux compris (sert au calcul des ETA).
    pub avg_speed_kmh: f64,
    /// Temps d'arrêt moyen à chaque arrêt intermédiaire.
    pub dwell_s: f64,
    /// Distance max entre la position GPS et le tracé pour être considéré "sur le tracé".
    pub snap_max_m: f64,
    /// Au-delà, la position est affichée comme "signal perdu".
    pub stale_after_s: i64,
    /// Au-delà, la navette est considérée comme hors ligne.
    pub offline_after_s: i64,
    /// Les positions moins précises que ça sont ignorées.
    pub max_accuracy_m: f64,
}

impl Default for TrackingConfig {
    fn default() -> Self {
        Self {
            avg_speed_kmh: 22.0,
            dwell_s: 60.0,
            snap_max_m: 60.0,
            stale_after_s: 90,
            offline_after_s: 600,
            max_accuracy_m: 150.0,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ShuttleConfig {
    /// Places assises : la navette part dès qu'elle est pleine.
    pub capacity: u32,
}

impl Default for ShuttleConfig {
    fn default() -> Self {
        Self { capacity: 16 }
    }
}

/// « Faire signe à la navette » : quelqu'un sur le trajet, loin d'un arrêt,
/// demande à être pris en route.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PickupsConfig {
    pub enabled: bool,
    /// Distance max entre la personne et le tracé.
    pub max_distance_m: f64,
    /// Plus près d'un arrêt que ça, on attend à l'arrêt.
    pub min_stop_distance_m: f64,
    /// Taille max d'un groupe.
    pub max_group: u32,
    /// Une demande non servie disparaît au bout de… (minutes).
    pub expire_after_min: i64,
    /// Demandes simultanées max, toutes confondues (protection contre les abus).
    pub max_active: usize,
}

impl Default for PickupsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_distance_m: 50.0,
            min_stop_distance_m: 150.0,
            max_group: 6,
            expire_after_min: 20,
            max_active: 12,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventConfig {
    pub name: String,
    #[serde(default)]
    pub info: String,
    #[serde(default)]
    pub schedule: Vec<String>,
    #[serde(default)]
    pub tickets_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopConfig {
    pub id: String,
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    #[serde(default = "default_stop_radius")]
    pub radius_m: f64,
    #[serde(default)]
    pub detail: String,
}

/// Prochains départs de la gare, via l'API publique transport.opendata.ch.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct TrainsConfig {
    pub enabled: bool,
    /// Identifiant officiel de la gare (Sion : 8501506).
    pub station_id: String,
    /// Nom affiché sur la page.
    pub station_name: String,
    /// Nombre de départs affichés.
    pub limit: usize,
    /// Moyens de transport : train, bus, tram, ship, cableway.
    pub transportations: Vec<String>,
    /// Durée du cache. L'API demande de ne pas l'interroger plus souvent
    /// que son temps réel ne change (30 s).
    pub refresh_s: u64,
}

impl Default for TrainsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            station_id: "8501506".into(),
            station_name: "Sion".into(),
            limit: 6,
            transportations: vec!["train".into()],
            refresh_s: 60,
        }
    }
}

fn default_listen() -> String {
    "0.0.0.0:8080".into()
}
fn default_static_dir() -> PathBuf {
    "static".into()
}
fn default_data_dir() -> PathBuf {
    "data".into()
}
fn default_route_file() -> PathBuf {
    "route.geojson".into()
}
fn default_max_viewers() -> usize {
    5000
}
fn default_stop_radius() -> f64 {
    50.0
}

impl Config {
    /// Charge le fichier TOML puis applique les variables d'environnement
    /// (`NAVETTE_LISTEN`, `NAVETTE_DRIVER_TOKEN`, `NAVETTE_ADMIN_TOKEN`).
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("lecture de {}", path.display()))?;
        let mut cfg: Config = toml::from_str(raw.trim_start_matches('\u{feff}'))
            .with_context(|| format!("format de {}", path.display()))?;

        if let Ok(v) = std::env::var("NAVETTE_LISTEN") {
            cfg.server.listen = v;
        }
        if let Ok(v) = std::env::var("NAVETTE_DRIVER_TOKEN") {
            cfg.auth.driver_token = Some(v);
        }
        if let Ok(v) = std::env::var("NAVETTE_ADMIN_TOKEN") {
            cfg.auth.admin_token = Some(v);
        }

        // Les chemins relatifs sont résolus depuis le dossier du fichier de config.
        let base = path.parent().unwrap_or(Path::new("."));
        for p in [
            &mut cfg.server.static_dir,
            &mut cfg.server.data_dir,
            &mut cfg.server.route_file,
        ] {
            if p.is_relative() {
                *p = base.join(&*p);
            }
        }

        cfg.validate()?;
        Ok(cfg)
    }

    fn validate(&self) -> anyhow::Result<()> {
        for (name, tok) in [
            ("NAVETTE_DRIVER_TOKEN", &self.auth.driver_token),
            ("NAVETTE_ADMIN_TOKEN", &self.auth.admin_token),
        ] {
            match tok {
                Some(t) if t.len() >= 16 => {}
                _ => bail!("{name} manquant ou trop court (16 caractères minimum)"),
            }
        }
        if self.stops.is_empty() {
            bail!("au moins un arrêt [[stops]] est requis");
        }
        if self.tracking.avg_speed_kmh <= 0.0 {
            bail!("tracking.avg_speed_kmh doit être positif");
        }
        if self.shuttle.capacity == 0 {
            bail!("shuttle.capacity doit être positif");
        }
        if self.pickups.enabled && self.pickups.max_group == 0 {
            bail!("pickups.max_group doit être positif");
        }
        if self.trains.enabled {
            if self.trains.refresh_s < 30 {
                bail!("trains.refresh_s : 30 secondes minimum (règle de l'API)");
            }
            if !(1..=12).contains(&self.trains.limit) {
                bail!("trains.limit doit être entre 1 et 12");
            }
        }
        Ok(())
    }

    pub fn driver_token(&self) -> &str {
        self.auth.driver_token.as_deref().unwrap_or_default()
    }

    pub fn admin_token(&self) -> &str {
        self.auth.admin_token.as_deref().unwrap_or_default()
    }
}
