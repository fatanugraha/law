#[macro_use]
extern crate envconfig_derive;
extern crate envconfig;

use askama::Template;
use std::collections::HashMap;
use uuid::Uuid;
use warp::Filter;

use futures;
use lapin::{options::*, types::FieldTable, BasicProperties};
use serde::Serialize;
use std::sync::Arc;
mod amqp;

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
    task_id: &'a str,
}

#[derive(Serialize)]
struct DownloadMessage<'a> {
    url: &'a str,
    task_id: &'a str,
    job_id: u8,
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
    amqp: Arc<amqp::AMQP>,
) -> Result<impl warp::reply::Reply, std::convert::Infallible> {
    let task_id = format!("{}", Uuid::new_v4().to_hyphenated());

    let mut template = DownloadProgress {
        files: vec![],
        task_id: &task_id,
    };
    for i in 0..FILE_NUM {
        if let Some(file_url) = form.get(&format!("file-{}", i)) {
            template.files.push(file_url)
        } else {
            return Ok(warp::reply::html(format!("file-{} is required", i)));
        }
    }

    let chan = amqp.create_channel().await;
    let exchange_name = format!("{}-download-queue", amqp.prefix);
    let queue_name = format!("{}-file-urls", amqp.prefix);

    let mut promises = vec![];
    for (i, url) in template.files.iter().enumerate() {
        let payload = DownloadMessage {
            url: &url,
            task_id: &task_id,
            job_id: i as u8,
        };
        promises.push(chan.basic_publish(
            &exchange_name,
            &queue_name,
            BasicPublishOptions::default(),
            serde_json::to_vec(&payload).unwrap(),
            BasicProperties::default(),
        ))
    }
    futures::future::join_all(promises).await;

    Ok(warp::reply::html(template.render().unwrap()))
}

async fn ensure_queue(rabbitmq: &amqp::AMQP) {
    let channel = rabbitmq.create_channel().await;

    let exchange_name = format!("{}-download-queue", rabbitmq.prefix);
    let queue_name = format!("{}-file-urls", rabbitmq.prefix);

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
            QueueDeclareOptions {
                durable: true,
                auto_delete: false,
                exclusive: false,
                nowait: false,
                passive: false,
            },
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
        .and_then(reply_download_progress);

    let filters = download_form.or(process_form);

    println!("Listened on :{}", config.port);
    warp::serve(filters).run(([0, 0, 0, 0], config.port)).await;
}
