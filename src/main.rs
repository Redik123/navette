mod api;
mod config;
mod geo;
mod sim;
mod state;
mod tracker;
mod trains;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::net::TcpListener;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::geo::Route;
use crate::state::AppState;

const USAGE: &str = "\
Usage : navette [config.toml] [--simulate]
        navette --healthcheck

  config.toml    fichier de configuration (défaut : ./config.toml)
  --simulate     fait rouler une navette fictive sur le tracé (démo / tests)
  --healthcheck  vérifie que le serveur local répond (utilisé par Docker)

Variables d'environnement :
  NAVETTE_DRIVER_TOKEN   jeton du téléphone de la navette (obligatoire)
  NAVETTE_ADMIN_TOKEN    jeton de la page d'administration (obligatoire)
  NAVETTE_LISTEN         adresse d'écoute, ex. 0.0.0.0:8080
  RUST_LOG               niveau de logs, ex. navette=debug";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("navette=info")),
        )
        .init();

    let mut config_path = PathBuf::from("config.toml");
    let mut simulate = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--simulate" => simulate = true,
            "--healthcheck" => return healthcheck(),
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            path => config_path = path.into(),
        }
    }

    let cfg = Config::load(&config_path)?;
    let route = Arc::new(Route::from_geojson_file(&cfg.server.route_file)?);
    info!(
        "tracé : {:.0} m, {} points{}",
        route.length,
        route.coords().len(),
        if route.closed { ", en boucle" } else { "" }
    );
    if !route.closed {
        warn!("le tracé n'est pas une boucle : les ETA ne couvrent que les arrêts devant la navette");
    }

    let listen = cfg.server.listen.clone();
    let state = AppState::new(cfg, route)?;
    tokio::spawn(state.clone().status_ticker());
    if simulate {
        tokio::spawn(sim::run(state.clone()));
    }

    let listener = TcpListener::bind(&listen).await?;
    info!("navette en écoute sur http://{listen}");
    // Pas d'arrêt « gracieux » : les flux SSE ne se terminent jamais d'eux-mêmes,
    // et l'état persistant est déjà écrit sur disque à chaque changement.
    // L'adresse du client sert à limiter les demandes « faire signe ».
    let app = api::router(state).into_make_service_with_connect_info::<SocketAddr>();
    axum::serve(listener, app).await?;
    Ok(())
}

/// Le serveur local répond-il ? L'image Docker n'a ni curl ni wget : une
/// requête HTTP minimale suffit (code de sortie 0 = en bonne santé).
fn healthcheck() -> anyhow::Result<()> {
    use std::io::{Read, Write};

    let listen = std::env::var("NAVETTE_LISTEN").unwrap_or_else(|_| "0.0.0.0:8080".into());
    let port: u16 = listen.rsplit(':').next().unwrap_or("8080").parse()?;
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port))?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(3)))?;
    stream.write_all(b"GET /api/config HTTP/1.0\r\nHost: localhost\r\n\r\n")?;
    let mut head = [0u8; 12];
    stream.read_exact(&mut head)?;
    if head.starts_with(b"HTTP/1.") && &head[9..12] == b"200" {
        Ok(())
    } else {
        anyhow::bail!("réponse inattendue : {}", String::from_utf8_lossy(&head))
    }
}
