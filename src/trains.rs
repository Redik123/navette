//! Prochains départs de la gare, via l'API publique transport.opendata.ch
//! (horaires CFF avec retards en temps réel, gratuite et sans clé).
//!
//! L'API demande de mettre en cache : le serveur l'interroge au plus une fois
//! par `refresh_s`, et seulement quand quelqu'un regarde. 200 festivaliers
//! connectés = une requête par minute.

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
const FIELDS: [&str; 7] = [
    "stationboard/category",
    "stationboard/number",
    "stationboard/to",
    "stationboard/stop/departureTimestamp",
    "stationboard/stop/delay",
    "stationboard/stop/platform",
    "stationboard/stop/prognosis/platform",
];

/// Après un échec, on réessaie plus tôt qu'au rythme normal.
const RETRY_AFTER: Duration = Duration::from_secs(20);

#[derive(Debug, PartialEq, Serialize)]
pub struct Departure {
    /// Départ prévu à l'horaire (ms Unix).
    pub time: i64,
    /// Retard annoncé, en minutes.
    pub delay_min: i64,
    /// Ex. « IR 95 », « R 91 ».
    pub line: String,
    pub category: String,
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
    departures: &'a [Departure],
}

pub struct Trains {
    cfg: TrainsConfig,
    client: reqwest::Client,
    cache: Mutex<Cache>,
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
        let json = board_json(&cfg, None, &[]);
        Ok(Self {
            cfg,
            client,
            cache: Mutex::new(Cache {
                json,
                next_fetch: Instant::now(),
            }),
        })
    }

    /// Tableau des départs en JSON, rafraîchi si le cache a expiré.
    /// Le verrou reste pris pendant la requête : les visiteurs qui arrivent
    /// au même moment attendent sa réponse au lieu de relancer l'API.
    /// En cas d'échec, on garde les derniers départs connus.
    pub async fn board_json(&self) -> Bytes {
        let mut cache = self.cache.lock().await;
        if Instant::now() >= cache.next_fetch {
            match self.fetch().await {
                Ok(departures) => {
                    cache.json = board_json(&self.cfg, Some(now_ms()), &departures);
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

    async fn fetch(&self) -> anyhow::Result<Vec<Departure>> {
        // Quelques départs de plus : la page masque ceux qui sont déjà partis.
        let limit = (self.cfg.limit + 4).to_string();
        let mut query = vec![("id", self.cfg.station_id.as_str()), ("limit", &limit)];
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
        parse(&body)
    }
}

fn board_json(cfg: &TrainsConfig, updated: Option<i64>, departures: &[Departure]) -> Bytes {
    let board = Board {
        station: &cfg.station_name,
        limit: cfg.limit,
        updated,
        departures,
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
    delay: Option<i64>,
    platform: Option<String>,
    prognosis: Option<ApiPrognosis>,
}

#[derive(Deserialize)]
struct ApiPrognosis {
    platform: Option<String>,
}

fn parse(body: &[u8]) -> anyhow::Result<Vec<Departure>> {
    let board: ApiBoard = serde_json::from_slice(body).context("réponse illisible")?;
    Ok(board
        .stationboard
        .into_iter()
        .filter_map(ApiEntry::into_departure)
        .collect())
}

impl ApiEntry {
    fn into_departure(self) -> Option<Departure> {
        let non_empty = |s: Option<String>| s.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
        let time = self.stop.departure_timestamp? * 1000;
        let to = non_empty(self.to)?;
        let category = non_empty(self.category).unwrap_or_default();
        let number = non_empty(self.number).unwrap_or_default();
        // Certaines lignes portent déjà leur catégorie (« S1 »).
        let line = if number.starts_with(&category) {
            number
        } else {
            format!("{category} {number}").trim().to_owned()
        };
        let planned = non_empty(self.stop.platform);
        let actual = non_empty(self.stop.prognosis.and_then(|p| p.platform));
        let platform_changed = actual.is_some() && actual != planned;
        Some(Departure {
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
             "category":"R","number":"91","to":"Sans heure"}
        ]}"#;
        let deps = parse(body.as_bytes()).unwrap();
        assert_eq!(deps.len(), 3);
        assert_eq!(
            deps[0],
            Departure {
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
    fn rejects_garbage() {
        assert!(parse(b"<html>maintenance</html>").is_err());
    }
}
