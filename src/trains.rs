//! Prochains trains en gare, via l'API publique transport.opendata.ch
//! (horaires CFF avec retards en temps réel, gratuite et sans clé) : les
//! départs pour la page publique, les arrivées pour l'équipage (combien de
//! temps attendre le prochain train à la gare).
//!
//! L'API demande de mettre en cache : le serveur interroge chaque tableau au
//! plus une fois par `refresh_s`, et seulement quand quelqu'un regarde.
//! 200 festivaliers connectés = une requête par minute.

use std::time::Duration;

use anyhow::Context;
use axum::body::Bytes;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tokio::time::Instant;
use tracing::warn;

use crate::config::TrainsConfig;
use crate::state::now_ms;

const API_URL: &str = "https://transport.opendata.ch/v1/stationboard";

/// Champs demandés : la réponse complète pèse ~40 Ko, celle-ci ~3 Ko.
const FIELDS: [&str; 8] = [
    "stationboard/category",
    "stationboard/number",
    "stationboard/to",
    "stationboard/stop/departureTimestamp",
    "stationboard/stop/arrivalTimestamp",
    "stationboard/stop/delay",
    "stationboard/stop/platform",
    "stationboard/stop/prognosis/platform",
];

/// Après un échec, on réessaie plus tôt qu'au rythme normal.
const RETRY_AFTER: Duration = Duration::from_secs(20);

/// Tableau demandé à l'API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Departures,
    Arrivals,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct Train {
    /// Départ ou arrivée prévus à l'horaire, selon le tableau (ms Unix).
    pub time: i64,
    /// Retard annoncé, en minutes.
    pub delay_min: i64,
    /// Ex. « IR 95 », « R 91 ».
    pub line: String,
    pub category: String,
    /// Terminus du train (la gare elle-même s'il s'y arrête).
    pub to: String,
    pub platform: Option<String>,
    /// La voie n'est pas celle de l'horaire.
    pub platform_changed: bool,
}

#[derive(Serialize)]
struct Board<'a> {
    station: &'a str,
    limit: usize,
    /// Dernière réponse de l'API (ms Unix), `null` si aucune.
    updated: Option<i64>,
    /// Une seule des deux listes, selon le tableau.
    #[serde(skip_serializing_if = "Option::is_none")]
    departures: Option<&'a [Train]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    arrivals: Option<&'a [Train]>,
}

pub struct Trains {
    cfg: TrainsConfig,
    client: reqwest::Client,
    departures: Mutex<Cache>,
    arrivals: Mutex<Cache>,
}

struct Cache {
    json: Bytes,
    next_fetch: Instant,
}

impl Trains {
    pub fn new(cfg: TrainsConfig) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .user_agent(concat!("navette/", env!("CARGO_PKG_VERSION"), " (Ragnarok Festival)"))
            .build()?;
        let empty = |kind| {
            Mutex::new(Cache {
                json: board_json(&cfg, kind, None, &[]),
                next_fetch: Instant::now(),
            })
        };
        Ok(Self {
            departures: empty(Kind::Departures),
            arrivals: empty(Kind::Arrivals),
            cfg,
            client,
        })
    }

    /// Tableau des départs ou des arrivées en JSON, rafraîchi si le cache a
    /// expiré. Le verrou reste pris pendant la requête : les visiteurs qui
    /// arrivent au même moment attendent sa réponse au lieu de relancer l'API.
    /// En cas d'échec, on garde les derniers trains connus.
    pub async fn board_json(&self, kind: Kind) -> Bytes {
        let cache = match kind {
            Kind::Departures => &self.departures,
            Kind::Arrivals => &self.arrivals,
        };
        let mut cache = cache.lock().await;
        if Instant::now() >= cache.next_fetch {
            match self.fetch(kind).await {
                Ok(trains) => {
                    cache.json = board_json(&self.cfg, kind, Some(now_ms()), &trains);
                    cache.next_fetch = Instant::now() + Duration::from_secs(self.cfg.refresh_s);
                }
                Err(e) => {
                    warn!("horaires CFF indisponibles : {e:#}");
                    cache.next_fetch = Instant::now() + RETRY_AFTER;
                }
            }
        }
        cache.json.clone()
    }

    async fn fetch(&self, kind: Kind) -> anyhow::Result<Vec<Train>> {
        // Quelques trains de plus : la page masque ceux qui sont déjà passés.
        let limit = (self.cfg.limit + 4).to_string();
        let mut query = vec![("id", self.cfg.station_id.as_str()), ("limit", &limit)];
        if kind == Kind::Arrivals {
            query.push(("type", "arrival"));
        }
        query.extend(self.cfg.transportations.iter().map(|t| ("transportations[]", t.as_str())));
        query.extend(FIELDS.map(|f| ("fields[]", f)));

        let body = self
            .client
            .get(API_URL)
            .query(&query)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        parse(&body, kind)
    }
}

fn board_json(cfg: &TrainsConfig, kind: Kind, updated: Option<i64>, trains: &[Train]) -> Bytes {
    let board = Board {
        station: &cfg.station_name,
        limit: cfg.limit,
        updated,
        departures: (kind == Kind::Departures).then_some(trains),
        arrivals: (kind == Kind::Arrivals).then_some(trains),
    };
    serde_json::to_vec(&board).unwrap_or_default().into()
}

#[derive(Deserialize)]
struct ApiBoard {
    stationboard: Vec<ApiEntry>,
}

#[derive(Deserialize)]
struct ApiEntry {
    category: Option<String>,
    number: Option<String>,
    to: Option<String>,
    stop: ApiStop,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiStop {
    departure_timestamp: Option<i64>,
    arrival_timestamp: Option<i64>,
    delay: Option<i64>,
    platform: Option<String>,
    prognosis: Option<ApiPrognosis>,
}

#[derive(Deserialize)]
struct ApiPrognosis {
    platform: Option<String>,
}

fn parse(body: &[u8], kind: Kind) -> anyhow::Result<Vec<Train>> {
    let board: ApiBoard = serde_json::from_slice(body).context("réponse illisible")?;
    Ok(board
        .stationboard
        .into_iter()
        .filter_map(|e| e.into_train(kind))
        .collect())
}

impl ApiEntry {
    fn into_train(self, kind: Kind) -> Option<Train> {
        let non_empty = |s: Option<String>| s.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
        // Sur le tableau des arrivées, l'API range l'heure d'arrivée dans
        // `departureTimestamp` et laisse `arrivalTimestamp` vide.
        let time = match kind {
            Kind::Departures => self.stop.departure_timestamp,
            Kind::Arrivals => self.stop.arrival_timestamp.or(self.stop.departure_timestamp),
        }? * 1000;
        let to = non_empty(self.to)?;
        let category = non_empty(self.category).unwrap_or_default();
        let mut number = non_empty(self.number).unwrap_or_default();
        // Les numéros de train arrivent parfois complétés de zéros (« 000044 »).
        if number.bytes().all(|b| b.is_ascii_digit()) {
            let trimmed = number.trim_start_matches('0');
            if !trimmed.is_empty() {
                number = trimmed.to_owned();
            }
        }
        // Certaines lignes portent déjà leur catégorie (« S1 »).
        let line = if number.starts_with(&category) {
            number
        } else {
            format!("{category} {number}").trim().to_owned()
        };
        let planned = non_empty(self.stop.platform);
        let actual = non_empty(self.stop.prognosis.and_then(|p| p.platform));
        let platform_changed = actual.is_some() && actual != planned;
        Some(Train {
            time,
            delay_min: self.stop.delay.unwrap_or(0).max(0),
            line,
            category,
            to,
            platform: actual.or(planned),
            platform_changed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_stationboard() {
        let body = r#"{"stationboard":[
            {"stop":{"departureTimestamp":1793101140,"delay":1,"platform":"3","prognosis":{"platform":null}},
             "category":"IR","number":"95","to":"Genève-Aéroport"},
            {"stop":{"departureTimestamp":1793102220,"delay":null,"platform":"2","prognosis":{"platform":"4"}},
             "category":"R","number":"91","to":"Brig"},
            {"stop":{"departureTimestamp":1793102520,"delay":0,"platform":null,"prognosis":null},
             "category":"S","number":"S1","to":"Monthey"},
            {"stop":{"departureTimestamp":null,"delay":0,"platform":"1","prognosis":null},
             "category":"R","number":"91","to":"Sans heure"},
            {"stop":{"departureTimestamp":1793102700,"delay":0,"platform":"3","prognosis":null},
             "category":"EC","number":"000044","to":"Genève"}
        ]}"#;
        let deps = parse(body.as_bytes(), Kind::Departures).unwrap();
        assert_eq!(deps.len(), 4);
        assert_eq!(deps[3].line, "EC 44");
        assert_eq!(
            deps[0],
            Train {
                time: 1_793_101_140_000,
                delay_min: 1,
                line: "IR 95".into(),
                category: "IR".into(),
                to: "Genève-Aéroport".into(),
                platform: Some("3".into()),
                platform_changed: false,
            }
        );
        assert_eq!(deps[1].delay_min, 0);
        assert_eq!(deps[1].platform.as_deref(), Some("4"));
        assert!(deps[1].platform_changed);
        assert_eq!(deps[2].line, "S1");
        assert_eq!(deps[2].platform, None);
    }

    #[test]
    fn parses_arrivals() {
        // Réponse réelle de l'API (type=arrival) : l'heure d'arrivée est dans
        // `departureTimestamp`, un train qui s'arrête à Sion va « à Sion ».
        let body = r#"{"stationboard":[
            {"stop":{"arrivalTimestamp":null,"departureTimestamp":1793395080,"delay":2,"platform":"2","prognosis":{"platform":null}},
             "category":"IR","number":"95","to":"Sion"},
            {"stop":{"arrivalTimestamp":1793396100,"departureTimestamp":1793396160,"delay":null,"platform":"3","prognosis":null},
             "category":"R","number":"91","to":"St-Maurice"}
        ]}"#;
        let arrs = parse(body.as_bytes(), Kind::Arrivals).unwrap();
        assert_eq!(arrs.len(), 2);
        assert_eq!(arrs[0].time, 1_793_395_080_000);
        assert_eq!(arrs[0].delay_min, 2);
        assert_eq!(arrs[0].to, "Sion");
        // Si l'API remplit un jour l'heure d'arrivée, c'est elle qui compte.
        assert_eq!(arrs[1].time, 1_793_396_100_000);
    }

    #[test]
    fn board_keeps_departures_key() {
        // La page publique lit `departures` : la clé ne doit pas changer.
        let cfg = TrainsConfig::default();
        let json = |kind| serde_json::from_slice::<serde_json::Value>(&board_json(&cfg, kind, None, &[])).unwrap();
        let deps = json(Kind::Departures);
        assert!(deps["departures"].is_array());
        assert!(deps.get("arrivals").is_none());
        let arrs = json(Kind::Arrivals);
        assert!(arrs["arrivals"].is_array());
        assert!(arrs.get("departures").is_none());
        assert_eq!(arrs["station"], "Sion");
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse(b"<html>maintenance</html>", Kind::Departures).is_err());
    }
}
