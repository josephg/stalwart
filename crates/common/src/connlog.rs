use std::sync::Arc;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

#[derive(Clone, Default)]
pub struct ConnLog {
    conn_id: u32,
    file: Option<Arc<tokio::sync::Mutex<File>>>,
}

impl ConnLog {
    pub fn new(conn_id: u32, file: File) -> Self {
        Self {
            conn_id,
            file: Some(Arc::new(tokio::sync::Mutex::new(file))),
        }
    }

    pub fn clone_with_conn_id(&self, conn_id: u32) -> Self {
        Self {
            conn_id,
            file: self.file.clone(),
        }
    }

    pub async fn write_log_msg<'a>(&self, op: ConnLogBody<'a>) {
        if let Some(file) = self.file.as_ref() {
            let mut log = file.lock().await;
            let conn_id = self.conn_id;
            // let conn_fd = &mut log;

            let msg = ConnLogMsg {
                conn_id,
                op,
                ts: OffsetDateTime::now_utc(),
            };

            // This is inefficient, but it doesn't matter.
            let mut msg_bytes = serde_json::to_vec(&msg).unwrap();
            msg_bytes.push(b'\n');
            log.write_all(&msg_bytes[..]).await.expect("Could not write to connection log");
        }
    }
}

type ConnId = u32;

#[derive(Debug, Serialize, Deserialize)]
pub enum ConnLogBody<'a> {
    ServerStartup,
    ServerShutdown,

    Connect {
        // Could be fancier with this to have v4 / v6 addresses but eh.
        ipaddr: String,
    },
    ServerToClientMsg(&'a [u8]),
    ClientToServerMsg(&'a [u8]),
    Close,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConnLogMsg<'a> {
    pub conn_id: ConnId,
    #[serde(borrow)]
    pub op: ConnLogBody<'a>,

    #[serde(with = "time::serde::rfc3339")]
    pub ts: OffsetDateTime,
}
