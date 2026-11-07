mod counts;
mod http;
mod line;
mod store;
mod top;
mod zones;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context as _;
use clap::Parser;
use tokio::net::{TcpListener, UdpSocket};
use tokio::signal::unix::{SignalKind, signal};
use tracing_subscriber::EnvFilter;

use crate::counts::Counts;
use crate::store::{Period, Store};
use crate::zones::Zones;

const FLUSH_INTERVAL: Duration = Duration::from_secs(60);

type Pending = Mutex<HashMap<(u64, i64), Counts>>;

#[derive(Parser)]
#[command(about)]
struct Args {
    #[arg(long, env = "TALLY_SYSLOG_BIND", default_value = "0.0.0.0:5514")]
    syslog_bind: SocketAddr,
    #[arg(long, env = "TALLY_PUBLIC_BIND", default_value = "0.0.0.0:3000")]
    public_bind: SocketAddr,
    #[arg(long, env = "TALLY_INTERNAL_BIND", default_value = "0.0.0.0:5515")]
    internal_bind: SocketAddr,
    #[arg(long, env = "TALLY_DATA_DIR", default_value = "data")]
    data_dir: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    let args = Args::parse();

    let store = Arc::new(Store::open(&args.data_dir).context("failed to open the store")?);
    let zones = Arc::new(Zones::load(Arc::clone(&store))?);
    let pending = Arc::new(Pending::default());
    let socket = UdpSocket::bind(args.syslog_bind).await?;
    let public_listener = TcpListener::bind(args.public_bind).await?;
    let internal_listener = TcpListener::bind(args.internal_bind).await?;
    tracing::info!(
        syslog = %args.syslog_bind,
        public = %args.public_bind,
        internal = %args.internal_bind,
        "listening"
    );

    let public = http::public_router(Arc::clone(&store), Arc::clone(&zones));
    let internal = http::internal_router(Arc::clone(&store), Arc::clone(&zones));
    let mut terminate = signal(SignalKind::terminate())?;
    tokio::select! {
        result = receive(socket, zones, Arc::clone(&pending)) => result?,
        result = axum::serve(public_listener, public).into_future() => result?,
        result = axum::serve(internal_listener, internal).into_future() => result?,
        result = flush_regularly(Arc::clone(&store), Arc::clone(&pending)) => result?,
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate.recv() => {}
    }
    flush(store, pending).await
}

async fn receive(
    socket: UdpSocket,
    zones: Arc<Zones>,
    pending: Arc<Pending>,
) -> anyhow::Result<()> {
    let mut buffer = vec![0; 65_536];
    loop {
        let (length, _) = socket.recv_from(&mut buffer).await?;
        // bunny sends valid utf8
        let Ok(line) = std::str::from_utf8(&buffer[..length]) else {
            continue;
        };
        // each bunny syslog message is one line
        let Some(request) = line::parse(line) else {
            tracing::debug!(%line, "not a bunny log line");
            continue;
        };

        zones.with(request.zone, |zone| {
            let Some(zone) = zone else {
                return;
            };
            if !zone.accepts(request.token) || !zone.counts(request.path) {
                return;
            }
            let hour = Period::Hour.start(request.time);
            let mut pending = pending.lock().expect("no thread panics holding the lock");
            let counts = pending.entry((request.zone, hour)).or_default();
            counts.add(&request, &zone.params);
        });
    }
}

async fn flush_regularly(store: Arc<Store>, pending: Arc<Pending>) -> anyhow::Result<()> {
    let mut interval = tokio::time::interval(FLUSH_INTERVAL);
    loop {
        interval.tick().await;
        flush(Arc::clone(&store), Arc::clone(&pending)).await?;
    }
}

async fn flush(store: Arc<Store>, pending: Arc<Pending>) -> anyhow::Result<()> {
    let taken = std::mem::take(&mut *pending.lock().expect("no thread panics holding the lock"));
    tokio::task::spawn_blocking(move || {
        for ((zone, hour), counts) in taken {
            store.add(zone, hour, &counts)?;
        }
        store.persist()
    })
    .await?
    .context("failed to save counts")
}
