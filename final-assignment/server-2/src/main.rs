#[macro_use]
extern crate envconfig_derive;
extern crate envconfig;

use envconfig::Envconfig;
use futures_util::StreamExt;
use lapin::{options::*, types::FieldTable, BasicProperties, Connection, ConnectionProperties};
use regex::Regex;
use reqwest;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::task;
use tokio_amqp::*;

#[derive(Envconfig)]
struct Config {
    #[envconfig(from = "AMQP_ADDR", default = "amqp://127.0.0.1:5672//")]
    pub amqp_addr: String,

    #[envconfig(from = "AMQP_PREFIX", default = "1606862753")]
    pub amqp_prefix: String,

    #[envconfig(from = "COMPRESSOR_ADDR", default = "http://127.0.0.1:8001")]
    pub compressor_addr: String,

    #[envconfig(from = "DOWNLOAD_DIR", default = "files")]
    pub download_dir: String,
}

#[derive(Deserialize, Clone)]
struct DownloadJob {
    urls: Vec<String>,
    task_id: String,
}

struct UploadJob {
    files: Vec<(String, u64)>,
    task_id: String,
}

#[derive(Serialize)]
struct Progress {
    id: u8,
    r#type: String,
    progress: u64,
    total: i64,
}

struct AMQP {
    channel: lapin::Channel,
    prefix: String,
}

impl AMQP {
    async fn report_progress(&self, task_id: &str, progress: &Progress) {
        self.channel
            .basic_publish(
                &format!("{}_TOPIC", self.prefix),
                &format!("{}.{}", self.prefix, task_id),
                BasicPublishOptions::default(),
                serde_json::to_vec(&progress).unwrap(),
                BasicProperties::default(),
            )
            .await
            .unwrap();
    }
}

async fn download_file(url: &str, dir: &str, id: u8, task_id: &str, amqp: &AMQP) -> (String, u64) {
    let filename_re = Regex::new(r#"filename="?([^";]+)"?(?:;|$)"#).unwrap();

    let result = reqwest::get(url).await.unwrap();

    // getting filename
    let mut filename = "file.bin".to_owned();
    if let Some(val) = result.headers().get(reqwest::header::CONTENT_DISPOSITION) {
        if let Some(cap) = filename_re.captures(val.to_str().unwrap()) {
            filename = cap[1].to_owned();
        }
    }

    let full_name = format!("{}/file-{}_{}", dir, id, filename);
    let fd = tokio::fs::File::create(&full_name).await.unwrap();
    let mut writer = tokio::io::BufWriter::new(fd);

    // get filesize
    let filesize = result
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .map(|x| x.to_str().unwrap())
        .unwrap_or("-1")
        .parse::<i64>()
        .unwrap_or(-1);

    let mut last_reported = 0;
    let threshold = 100 * 1024; // report every 100KiB
    let mut dl = Progress {
        r#type: "download".to_owned(),
        progress: 0,
        total: filesize,
        id: id,
    };

    let mut stream = result.bytes_stream();
    while let Some(item) = stream.next().await {
        let item = item.unwrap();
        dl.progress += item.len() as u64;

        if dl.progress - last_reported > threshold {
            last_reported = dl.progress;

            // TODO: should not block, cancel pending future
            amqp.report_progress(&task_id, &dl).await;
        }
        &writer.write_all(&item).await;
    }

    amqp.report_progress(&task_id, &dl).await;
    writer.flush().await.unwrap();

    (full_name, dl.progress)
}

async fn download_files(
    message: &DownloadJob,
    download_dir: &str,
    amqp: &AMQP,
) -> Vec<(String, u64)> {
    // prepare directory
    let dir = format!("{}/{}", download_dir, &message.task_id);
    tokio::fs::create_dir_all(&dir).await.unwrap();

    let mut promises = vec![];
    for (i, url) in message.urls.iter().enumerate() {
        promises.push(download_file(&url, &dir, i as u8, &message.task_id, amqp))
    }
    futures::future::join_all(promises).await
}

async fn compress_files(message: &UploadJob, url: &str) {
    let client = reqwest::Client::new();
    let mut form = vec![("task_id".to_owned(), message.task_id.to_owned())];

    for (i, (file, sz)) in message.files.iter().enumerate() {
        let key = format!("file-{}", i);
        form.push((key, file.to_owned()));

        let key = format!("file-{}-size", i);
        form.push((key, sz.to_string()));
    }
    client.post(url).form(&form).send().await;
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
        let compress_addr = config.compressor_addr.clone();
        let prefix = config.amqp_prefix.clone();
        let dir = config.download_dir.clone();

        let download_handle = task::spawn(async move {
            let (channel, delivery) = delivery.unwrap();

            channel
                .basic_ack(delivery.delivery_tag, BasicAckOptions::default())
                .await
                .expect("ack");

            let amqp = AMQP { channel, prefix };
            let message: DownloadJob = serde_json::from_slice(&delivery.data).unwrap();
            let files = download_files(&message, &dir, &amqp).await;
            let upload_message: UploadJob = UploadJob {
                task_id: message.task_id.to_string(),
                files: files,
            };
            compress_files(&upload_message, &compress_addr).await;
        });

        println!("downloading...");
        download_handle.await.unwrap();
        println!("download done.");
    }
}
