use remoc::{robs::{list::ObservableList, hash_map::ObservableHashMap}, rtc::async_trait};
use tokio::sync::RwLock;
use super::*;

pub struct MockProcessStore {
    services: RwLock<ObservableHashMap<String, LocalProcessId>>,
    processes: ObservableHashMap<LocalProcessId, ProcessInfo>,

}

#[async_trait]
impl ProcessStore for MockProcessStore {
    async fn print_hello_world(&self) -> CallResult<()>{
        Ok(())
    }

    async fn find_process(&self, pid: LocalProcessId) -> ResourceResult<ProcessApiClient>{
        match self.processes.get(&pid) {
            None => Err(ResourceError::Unavailable),
            Some => Ok(()),
        }
    }

    async fn register_service(&self, pid: LocalProcessId, name: String) -> ResourceResult<()>{
        
        if !self.processes.contains_key(&pid) {
            return Err(ResourceError::Unavailable);
        }
        let mut services = self.services.write().await;
        if services.contains_key(&name){
            return Err(ResourceError::BadParams);
        }
        services.insert(name, pid);
        Ok(())
    }

    async fn deregister_service(&self, name: String) -> ResourceResult<()>{
        match self.services.write().await.remove(&name) {
            None => Err(ResourceError::Unavailable),
            _ => Ok(())
        }

    }

    async fn follow_process_list(
        &self,
    ) -> CallResult<HashMapSubscription<LocalProcessId, ProcessInfo>> {
        Ok(self.processes.subscribe(128))

    }

    async fn follow_service_list(&self) -> CallResult<HashMapSubscription<String, LocalProcessId>>{
        Ok(self.services.read().await.subscribe(128))

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
            log: RwLock::new(vec![
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
            .into()),
        }
    }
}


