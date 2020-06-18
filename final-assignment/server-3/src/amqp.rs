use lapin::{Connection, ConnectionProperties};
use tokio_amqp::*;

pub struct AMQP {
    pub addr: String,
    pub prefix: String,

    conn: lapin::Connection,
}

impl AMQP {
    pub async fn init(addr: &str, prefix: &str) -> AMQP {
        let conn = Connection::connect(&addr, ConnectionProperties::default().with_tokio())
            .await
            .unwrap();

        AMQP {
            addr: addr.into(),
            prefix: prefix.into(),
            conn: conn
        }
    }

    pub async fn create_channel(&self) -> lapin::Channel {
        self.conn.create_channel().await.unwrap()
    }
}
