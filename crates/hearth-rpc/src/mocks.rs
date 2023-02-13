use remoc::{robs::list::ObservableList, rtc::async_trait};
use tokio::sync::RwLock;
use super::*;

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
