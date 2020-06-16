#[macro_use]
extern crate envconfig_derive;
extern crate envconfig;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use envconfig::Envconfig;
use lapin::{options::*, BasicProperties, Connection, ConnectionProperties};
use serde::Serialize;
use tokio_amqp::*;

#[derive(Envconfig)]
struct Config {
    #[envconfig(from = "AMQP_ADDR", default = "amqp://127.0.0.1:5672//")]
    pub amqp_addr: String,

    #[envconfig(from = "AMQP_PREFIX", default = "1606862753")]
    pub amqp_prefix: String,
}

#[derive(Serialize)]
struct Payload<'a> {
    r#type: &'a str,
    timestamp: u64,
}

#[tokio::main]
async fn main() {
    let config = Config::init().unwrap();

    let conn = Connection::connect(
        &config.amqp_addr,
        ConnectionProperties::default().with_tokio(),
    )
    .await
    .unwrap();

    let channel = conn.create_channel().await.unwrap();
    let exchange_name = format!("{}_TOPIC", &config.amqp_prefix);
    let routing_key = format!("{}.time", &config.amqp_prefix);

    let mut payload = Payload {
        r#type: &"time",
        timestamp: 0,
    };

    loop {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        payload.timestamp = now;

        match channel
            .basic_publish(
                &exchange_name,
                &routing_key,
                BasicPublishOptions::default(),
                serde_json::to_vec(&payload).unwrap(),
                BasicProperties::default().with_expiration("60000".into()),
            )
            .await
        {
            Err(_) => println!("{}: failed to publish", now),
            _ => (),
        }

        std::thread::sleep(Duration::from_secs(1));
    }
}
