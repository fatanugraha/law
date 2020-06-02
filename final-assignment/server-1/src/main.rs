#[macro_use]
extern crate envconfig_derive;
extern crate envconfig;

use askama::Template;
use std::collections::HashMap;
use warp::Filter; // bring trait in scope

use lapin::{options::*, types::FieldTable, Connection, ConnectionProperties};
use tokio_amqp::*;

use envconfig::Envconfig;

#[derive(Envconfig)]
struct Config {
    #[envconfig(from = "AMQP_ADDR", default = "amqp://127.0.0.1:5672//")]
    pub amqp_addr: String,

    #[envconfig(from = "AMQP_PREFIX", default = "1606862753")]
    pub amqp_prefix: String,

    #[envconfig(from = "PORT", default = "8000")]
    pub port: u16,
}

#[derive(Template)]
#[template(path = "form.html")]
struct DownloadForm<'a> {
    files: Vec<&'a str>,
}

#[derive(Template)]
#[template(path = "progress.html")]
struct DownloadProgress<'a> {
    files: Vec<&'a str>,
}

static FILE_NUM: u32 = 10;

fn reply_download_form() -> impl warp::reply::Reply {
    let mut vec = Vec::new();
    for _ in 0..FILE_NUM {
        vec.push("http://yeay.xyz/robot.txt")
    }

    let form = DownloadForm { files: vec };
    warp::reply::html(form.render().unwrap())
}

async fn reply_download_progress(
    form: HashMap<String, String>,
    channel: lapin::Channel
) -> Result<impl warp::reply::Reply, std::convert::Infallible> {
    let mut template = DownloadProgress { files: vec![] };
    for i in 0..FILE_NUM {
        if let Some(file_url) = form.get(&format!("file-{}", i)) {
            template.files.push(file_url)
        } else {
            return Ok(warp::reply::html(format!("file-{} is required", i)));
        }
    }

    Ok(warp::reply::html(template.render().unwrap()))
}

async fn get_amqp_channel(addr: &str, prefix: &str) -> lapin::Channel {
    let conn = Connection::connect(addr, ConnectionProperties::default().with_tokio())
        .await
        .unwrap();

    let exchange_name = format!("{}-download-queue", prefix);
    let queue_name = "file-urls";

    let channel = conn.create_channel().await.unwrap();
    channel
        .exchange_declare(
            &exchange_name,
            lapin::ExchangeKind::Direct,
            ExchangeDeclareOptions::default(),
            FieldTable::default(),
        )
        .await
        .unwrap();

    let mut args = FieldTable::default();
    args.insert(
        "x-message-ttl".into(),
        lapin::types::AMQPValue::LongString("60000".into()),
    );
    channel
        .queue_declare(
            &queue_name,
            QueueDeclareOptions {
                durable: true,
                auto_delete: false,
                exclusive: false,
                nowait: false,
                passive: false,
            },
            args,
        )
        .await
        .unwrap();

    channel
        .queue_bind(
            &queue_name,
            &exchange_name,
            &queue_name,
            QueueBindOptions::default(),
            FieldTable::default(),
        )
        .await
        .unwrap();

    channel
}

fn with_channel(
    channel: lapin::Channel,
) -> impl Filter<Extract = (lapin::Channel,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || channel.clone())
}

#[tokio::main]
async fn main() {
    let config = Config::init().unwrap();

    let channel = get_amqp_channel(&config.amqp_addr, &config.amqp_prefix).await;

    let download_form = warp::get()
        .and(warp::filters::path::end())
        .map(reply_download_form);

    let process_form = warp::post()
        .and(warp::filters::path::end())
        .and(warp::body::form())
        .and(with_channel(channel))
        .and_then(reply_download_progress);

    let filters = download_form.or(process_form);
    warp::serve(filters).run(([0, 0, 0, 0], config.port)).await;
}
