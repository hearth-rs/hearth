use std::net::SocketAddr;

use clap::Parser;
use hearth_network::auth::login;
use hearth_rpc::*;
use remoc::rtc::{async_trait, ServerShared};
use tokio::net::TcpStream;
use tracing::{debug, error, info};

/// Client program to the Hearth virtual space server.
#[derive(Parser, Debug)]
pub struct Args {
    /// IP address and port of the server to connect to.
    // TODO support DNS resolution too
    #[arg(short, long)]
    pub server: SocketAddr,

    /// Password to use to authenticate to the server. Defaults to empty.
    #[arg(short, long, default_value = "")]
    pub password: String,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let format = tracing_subscriber::fmt::format().compact();
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .event_format(format)
        .init();

    info!("Connecting to server at {:?}", args.server);
    let mut socket = match TcpStream::connect(args.server).await {
        Ok(s) => s,
        Err(err) => {
            error!("Failed to connect to server: {:?}", err);
            return;
        }
    };

    info!("Authenticating");
    let session_key = match login(&mut socket, args.password.as_bytes()).await {
        Ok(key) => key,
        Err(err) => {
            error!("Failed to authenticate with server: {:?}", err);
            return;
        }
    };

    use hearth_network::encryption::{AsyncDecryptor, AsyncEncryptor, Key};
    let client_key = Key::from_client_session(&session_key);
    let server_key = Key::from_server_session(&session_key);

    let (server_rx, server_tx) = tokio::io::split(socket);
    let server_rx = AsyncDecryptor::new(&server_key, server_rx);
    let server_tx = AsyncEncryptor::new(&client_key, server_tx);

    use remoc::rch::base::{Receiver, Sender};
    let cfg = remoc::Cfg::default();
    let (conn, mut tx, mut rx): (_, Sender<ClientOffer>, Receiver<ServerOffer>) =
        match remoc::Connect::io(cfg, server_rx, server_tx).await {
            Ok(v) => v,
            Err(err) => {
                error!("Remoc connection failure: {:?}", err);
                return;
            }
        };

    debug!("Spawning Remoc connection thread");
    let join_connection = tokio::spawn(conn);

    debug!("Receiving server offer");
    let offer = rx.recv().await.unwrap().unwrap();

    info!("Assigned peer ID {:?}", offer.new_id);

    let peer_api = PeerApiImpl {
        info: PeerInfo { nickname: None },
    };

    let peer_api = std::sync::Arc::new(peer_api);
    let (peer_api_server, peer_api_client) =
        PeerApiServerShared::<_, remoc::codec::Default>::new(peer_api, 1024);

    debug!("Spawning peer API server thread");
    tokio::spawn(async move {
        peer_api_server.serve(true).await;
    });

    tx.send(ClientOffer {
        peer_api: peer_api_client,
    })
    .await
    .unwrap();

    info!("Successfully connected!");

    debug!("Initializing IPC");
    let daemon_listener = match hearth_ipc::Listener::new().await {
        Ok(l) => l,
        Err(err) => {
            tracing::error!("IPC listener setup error: {:?}", err);
            return;
        }
    };

    let daemon_offer = DaemonOffer {
        peer_provider: offer.peer_provider.clone(),
        peer_id: offer.new_id,
    };

    hearth_ipc::listen(daemon_listener, daemon_offer);

    debug!("Waiting to join connection thread");
    join_connection.await.unwrap().unwrap();
}

pub struct PeerApiImpl {
    pub info: PeerInfo,
}

#[async_trait]
impl PeerApi for PeerApiImpl {
    async fn get_info(&self) -> CallResult<PeerInfo> {
        Ok(self.info.clone())
    }

    async fn get_process_store(&self) -> CallResult<ProcessStoreClient> {
        error!("Process stores are unimplemented");
        Err(remoc::rtc::CallError::RemoteForward)
    }
}