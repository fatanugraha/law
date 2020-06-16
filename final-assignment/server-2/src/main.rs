#[macro_use]
extern crate envconfig_derive;
extern crate envconfig;
use futures_util::StreamExt;
use reqwest;

use tokio::io::AsyncWriteExt;

use envconfig::Envconfig;
use lapin::{
    options::*, types::FieldTable, BasicProperties, Connection,
    ConnectionProperties,
};
use serde::{Deserialize, Serialize};
use tokio::task;
use tokio_amqp::*;

#[derive(Envconfig)]
struct Config {
    #[envconfig(from = "AMQP_ADDR", default = "amqp://127.0.0.1:5672//")]
    pub amqp_addr: String,

    #[envconfig(from = "AMQP_PREFIX", default = "1606862753")]
    pub amqp_prefix: String,
}

#[derive(Deserialize, Debug)]
struct DownloadJob<'a> {
    urls: Vec<&'a str>,
    task_id: &'a str,
}

use regex::Regex;

#[derive(Serialize)]
struct Progress<'a> {
    id: u8,
    r#type: &'a str,
    progress: u64,
    total: i64,
}

async fn report_download_progress(
    channel: &lapin::Channel,
    task_id: &str,
    id: u8,
    written: u64,
    filesize: i64,
) {
    let payload = Progress {
        id: id,
        r#type: "download",
        progress: written,
        total: filesize,
    };
    channel.basic_publish(
        "1606862753_TOPIC",
        &format!("1606862753.{}", task_id),
        BasicPublishOptions::default(),
        serde_json::to_vec(&payload).unwrap(),
        BasicProperties::default(),
    ).await.unwrap();
    println!("{} -> {}/{}", id, written, filesize);
}

async fn download_file(
    url: &str,
    task_id: &str,
    id: u8,
    dir: &str,
    channel: &lapin::Channel,
) -> Result<(), reqwest::Error> {
    let filename_re = Regex::new(r#"filename="?([^";]+)"?(?:;|$)"#).unwrap();
    let result = reqwest::get(url).await?;
    let mut filename = "file.bin".to_owned();

    if let Some(val) = result.headers().get(reqwest::header::CONTENT_DISPOSITION) {
        if let Some(cap) = filename_re.captures(val.to_str().unwrap()) {
            filename = cap[1].to_owned();
        }
    }
    let filesize = result
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .map(|x| x.to_str().unwrap())
        .unwrap_or("-1")
        .parse::<i64>()
        .unwrap_or(-1);

    let fd = tokio::fs::File::create(format!("{}/file-{}_{}", dir, id, filename))
        .await
        .unwrap();

    let mut writer = tokio::io::BufWriter::new(fd);
    let mut stream = result.bytes_stream();

    let mut written = 0;
    let mut last_reported = 0;
    let threshold = 100 * 1024; // report every 100KiB

    while let Some(item) = stream.next().await {
        let item = item.unwrap();
        written += item.len() as u64;
        if written - last_reported > threshold {
            last_reported = written;

            // should not block, but okay
            report_download_progress(channel, &task_id, id, written, filesize).await;
        }
        &writer.write_all(&item).await;
    }

    if written != last_reported {
        report_download_progress(channel, &task_id, id, written, filesize).await;
    }

    writer.flush().await.unwrap();
    Ok(())
}

async fn download_files(message: DownloadJob<'_>, channel: &lapin::Channel) {
    println!("{}", message.task_id);

    let dir = format!("tmp/{}", &message.task_id);
    tokio::fs::create_dir_all(&dir).await.unwrap();

    let mut promises = vec![];
    for (i, url) in message.urls.iter().enumerate() {
        promises.push(download_file(&url, message.task_id, i as u8, &dir, channel))
    }

    futures::future::join_all(promises).await;
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
    let mut consumer = channel
        .basic_consume(
            &format!("{}.file-urls", &config.amqp_prefix),
            "server-2",
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await
        .unwrap();

    while let Some(delivery) = consumer.next().await {
        let download_handle = task::spawn(async move {
            let (channel, delivery) = delivery.unwrap();

            channel
                .basic_ack(delivery.delivery_tag, BasicAckOptions::default())
                .await
                .expect("ack");

            let message: DownloadJob = serde_json::from_slice(&delivery.data).unwrap();
            download_files(message, &channel).await;
        });

        println!("downloading");
        download_handle.await.unwrap();
        println!("download done");
    }
}
