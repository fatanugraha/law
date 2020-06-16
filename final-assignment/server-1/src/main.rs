#[macro_use]
extern crate envconfig_derive;
extern crate envconfig;

use askama::Template;
use std::collections::HashMap;
use uuid::Uuid;
use warp::Filter;

use lapin::{options::*, types::FieldTable, BasicProperties};
use serde::Serialize;
use std::sync::Arc;
mod amqp;

use envconfig::Envconfig;

#[derive(Envconfig, Clone)]
struct Config {
    #[envconfig(from = "AMQP_ADDR", default = "amqp://127.0.0.1:5672//")]
    pub amqp_addr: String,

    #[envconfig(from = "AMQP_PREFIX", default = "1606862753")]
    pub amqp_prefix: String,

    #[envconfig(from = "WS_URL", default = "http://152.118.148.95:15674/stomp")]
    pub ws_url: String,

    #[envconfig(from = "WS_NAMESPACE", default = "0806444524")]
    pub ws_namespace: String,

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
    task_id: &'a str,
    config: Config,
}

#[derive(Serialize)]
struct DownloadMessage<'a> {
    urls: &'a [&'a str],
    task_id: &'a str,
}

static FILE_NUM: u32 = 10;

fn reply_download_form() -> impl warp::reply::Reply {
    let mut vec = Vec::new();
    for _ in 0..FILE_NUM {
        vec.push("https://unsplash.com/photos/Eo_OJXgi_P8/download?force=true")
    }

    let form = DownloadForm { files: vec };
    warp::reply::html(form.render().unwrap())
}

async fn reply_download_progress(
    form: HashMap<String, String>,
    amqp: Arc<amqp::AMQP>,
    config: Config,
) -> Result<impl warp::reply::Reply, std::convert::Infallible> {
    let task_id = format!("{}", Uuid::new_v4().to_hyphenated());

    let mut template = DownloadProgress {
        files: vec![],
        task_id: &task_id,
        config,
    };
    for i in 0..FILE_NUM {
        if let Some(file_url) = form.get(&format!("file-{}", i)) {
            template.files.push(file_url)
        } else {
            return Ok(warp::reply::html(format!("file-{} is required", i)));
        }
    }

    let chan = amqp.create_channel().await;
    let download_exchange_name = format!("{}_DIRECT", amqp.prefix);
    let download_queue_name = format!("{}.file-urls", amqp.prefix);

    let payload = DownloadMessage {
        urls: template.files.as_slice(),
        task_id: &task_id,
    };
    chan.basic_publish(
        &download_exchange_name,
        &download_queue_name,
        BasicPublishOptions::default(),
        serde_json::to_vec(&payload).unwrap(),
        BasicProperties::default(),
    )
    .await
    .unwrap();

    Ok(warp::reply::html(template.render().unwrap()))
}

async fn ensure_queue(rabbitmq: &amqp::AMQP) {
    let channel = rabbitmq.create_channel().await;

    let exchange_name = format!("{}_DIRECT", rabbitmq.prefix);
    let queue_name = format!("{}.file-urls", rabbitmq.prefix);

    channel
        .exchange_declare(
            &exchange_name,
            lapin::ExchangeKind::Direct,
            ExchangeDeclareOptions::default(),
            FieldTable::default(),
        )
        .await
        .unwrap();

    channel
        .queue_declare(
            &queue_name,
            QueueDeclareOptions::default(),
            FieldTable::default(),
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
}

fn with_rabbitmq(
    amqp: Arc<amqp::AMQP>,
) -> impl Filter<Extract = (Arc<amqp::AMQP>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || amqp.clone())
}

fn with_config(
    config: Config,
) -> impl Filter<Extract = (Config,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || config.clone())
}

#[tokio::main]
async fn main() {
    let config = Config::init().unwrap();

    let amqp = amqp::AMQP::init(&config.amqp_addr, &config.amqp_prefix).await;
    ensure_queue(&amqp).await;

    let rabbitmq = Arc::new(amqp);

    let download_form = warp::get()
        .and(warp::filters::path::end())
        .map(reply_download_form);

    let process_form = warp::post()
        .and(warp::filters::path::end())
        .and(warp::body::form())
        .and(with_rabbitmq(rabbitmq))
        .and(with_config(config.clone()))
        .and_then(reply_download_progress);

    let filters = download_form.or(process_form);

    println!("Listened on :{}", config.port);
    warp::serve(filters).run(([0, 0, 0, 0], config.port)).await;
}
