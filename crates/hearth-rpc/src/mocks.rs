use super::*;
use remoc::rtc::ServerShared;
use remoc::{
    robs::{hash_map::ObservableHashMap, list::ObservableList},
    rtc::async_trait,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct MockProcessStore {
    services: RwLock<ObservableHashMap<String, LocalProcessId>>,
    processes: ObservableHashMap<LocalProcessId, ProcessInfo>,
    mock_processes: HashMap<LocalProcessId, ProcessApiClient>,
}

#[async_trait]
impl ProcessStore for MockProcessStore {
    async fn print_hello_world(&self) -> CallResult<()> {
        Ok(())
    }

    async fn find_process(&self, pid: LocalProcessId) -> ResourceResult<ProcessApiClient> {
        match self.mock_processes.get(&pid) {
            None => Err(ResourceError::Unavailable),
            Some(api) => Ok(api.clone()),
        }
    }

    async fn register_service(&self, pid: LocalProcessId, name: String) -> ResourceResult<()> {
        if !self.processes.contains_key(&pid) {
            return Err(ResourceError::Unavailable);
        }
        let mut services = self.services.write().await;
        if services.contains_key(&name) {
            return Err(ResourceError::BadParams);
        }
        services.insert(name, pid);
        Ok(())
    }

    async fn deregister_service(&self, name: String) -> ResourceResult<()> {
        match self.services.write().await.remove(&name) {
            None => Err(ResourceError::Unavailable),
            _ => Ok(()),
        }
    }

    async fn follow_process_list(
        &self,
    ) -> CallResult<HashMapSubscription<LocalProcessId, ProcessInfo>> {
        Ok(self.processes.subscribe(128))
    }

    async fn follow_service_list(&self) -> CallResult<HashMapSubscription<String, LocalProcessId>> {
        Ok(self.services.read().await.subscribe(128))
    }
}

impl MockProcessStore {
    pub fn new() -> Self {
        let mut processes = ObservableHashMap::new();
        let mut mock_processes = HashMap::new();
        let test_pid = LocalProcessId(0);
        let test_info = ProcessInfo {
            source_lump: LumpId(Default::default()),
        };
        let test_process = MockProcessApi::new();
        processes.insert(test_pid, test_info);
        let (process_server, process_client) =
            ProcessApiServerShared::<_, remoc::codec::Default>::new(Arc::new(test_process), 1024);
        tokio::spawn(async move {
            process_server.serve(true).await;
        });
        mock_processes.insert(test_pid, process_client);
        Self {
            services: Default::default(),
            processes,
            mock_processes,
        }
    }
}

pub struct MockProcessApi {
    log: RwLock<ObservableList<ProcessLogEvent>>,
}

#[async_trait]
impl ProcessApi for MockProcessApi {
    async fn is_alive(&self) -> CallResult<bool> {
        Ok(true)
    }

    async fn kill(&self) -> ResourceResult<()> {
        Err(ResourceError::BadParams)
    }

    async fn send_message(&self, msg: Vec<u8>) -> ResourceResult<()> {
        self.log.write().await.push(ProcessLogEvent {
            level: ProcessLogLevel::Debug,
            module: String::from("Received Message"),
            content: String::from_utf8(msg.clone()).unwrap_or_else(|_| format!("{:?}", msg)),
        });
        Ok(())
    }

    async fn follow_log(&self) -> ResourceResult<ListSubscription<ProcessLogEvent>> {
        Ok(self.log.read().await.subscribe())
    }
}

impl MockProcessApi {
    pub fn new() -> Self {
        Self {
            log: RwLock::new(
                vec![
                    ProcessLogEvent {
                        level: ProcessLogLevel::Info,
                        module: String::from("init"),
                        content: String::from(
                            "This is an info level log message generated on process initialization",
                        ),
                    },
                    ProcessLogEvent {
                        level: ProcessLogLevel::Warning,
                        module: String::from("init"),
                        content: String::from("This is a mock process"),
                    },
                    ProcessLogEvent {
                        level: ProcessLogLevel::Trace,
                        module: String::from("tracer from overwatch"),
                        content: String::from("low level thing you cant understand"),
                    },
                    ProcessLogEvent {
                        level: ProcessLogLevel::Debug,
                        module: String::from("spider"),
                        content: String::from("The spider has been de-bugged :("),
                    },
                    ProcessLogEvent {
                        level: ProcessLogLevel::Error,
                        module: String::from("awwww fuck"),
                        content: String::from("oi can belie ya don dis"),
                    },
                ]
                .into(),
            ),
        }
    }
}

pub struct MockPeerApi {
    peer_info: PeerInfo,
    process_store: ProcessStoreClient,
}

#[async_trait]
impl PeerApi for MockPeerApi {
    async fn get_info(&self) -> CallResult<PeerInfo> {
        Ok(self.peer_info.clone())
    }

    async fn get_process_store(&self) -> CallResult<ProcessStoreClient> {
        Ok(self.process_store.clone())
    }

    async fn get_lump_store(&self) -> CallResult<LumpStoreClient> {
        Err(CallError::RemoteForward)
    }
}

impl MockPeerApi {
    pub fn new() -> Self {
        let test_store = MockProcessStore::new();
        let (store_server, process_store) =
            ProcessStoreServerShared::<_, remoc::codec::Default>::new(Arc::new(test_store), 1024);
        tokio::spawn(async move {
            store_server.serve(true).await;
        });
        MockPeerApi {
            peer_info: PeerInfo {
                nickname: Some("New peer".into()),
            },
            process_store,
        }
    }
}

pub struct MockPeerProvider {
    peers: HashMap<PeerId, PeerApiClient>,
    peer_info: ObservableHashMap<PeerId, PeerInfo>,
}

#[async_trait]
impl PeerProvider for MockPeerProvider {
    async fn find_peer(&self, id: PeerId) -> ResourceResult<PeerApiClient> {
        match self.peers.get(&id) {
            None => Err(ResourceError::Unavailable),
            Some(peer) => Ok(peer.clone()),
        }
    }

    async fn follow_peer_list(&self) -> CallResult<HashMapSubscription<PeerId, PeerInfo>> {
        Ok(self.peer_info.subscribe(128))
    }
}

impl MockPeerProvider {
    pub fn new() -> Self {
        let mut peer_info = ObservableHashMap::new();
        let mut peers = HashMap::new();
        let test_pid = PeerId(0);
        let test_info = PeerInfo {
            nickname: { Some("Silly Peer".into()) },
        };
        let test_process = MockPeerApi::new();
        peer_info.insert(test_pid, test_info);
        let (peer_server, peer_client) =
            PeerApiServerShared::<_, remoc::codec::Default>::new(Arc::new(test_process), 1024);
        tokio::spawn(async move {
            peer_server.serve(true).await;
        });
        peers.insert(test_pid, peer_client);
        Self { peers, peer_info }
    }
}
