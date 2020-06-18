#[macro_use]
extern crate envconfig_derive;
extern crate envconfig;

use envconfig::Envconfig;
use std::sync::Arc;
use warp::Filter;
mod amqp;
use lapin::{options::*, BasicProperties};
use serde::Serialize;
use std::collections::HashMap;
use std::io::Write;
use std::time::SystemTime;
use tokio::io::AsyncReadExt;
use zip;

#[derive(Envconfig, Clone)]
struct Config {
    #[envconfig(from = "AMQP_ADDR", default = "amqp://127.0.0.1:5672//")]
    pub amqp_addr: String,

    #[envconfig(from = "AMQP_PREFIX", default = "1606862753")]
    pub amqp_prefix: String,

    #[envconfig(from = "PORT", default = "8000")]
    pub port: u16,

    #[envconfig(from = "COMPRESS_DIR", default = "compressed")]
    pub compress_dir: String,

    #[envconfig(from = "FILE_SERVER_HOST", default = "http://localhost:8002")]
    pub file_server_host: String,

    #[envconfig(from = "HASH_SECRET", default = "secret123")]
    pub hash_secret: String,
}

#[derive(Serialize)]
struct Progress {
    r#type: String,
    progress: u64,
    total: i64,
}
#[derive(Serialize)]
struct Complete {
    r#type: String,
    url: String,
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

async fn compress_svc(
    form: HashMap<String, String>,
    amqp: Arc<amqp::AMQP>,
    config: Config,
) -> Result<impl warp::reply::Reply, std::convert::Infallible> {
    let task_id = match form.get("task_id") {
        None => {
            return Ok(warp::reply::with_status(
                "Bad Request",
                warp::http::StatusCode::BAD_REQUEST,
            ))
        }
        Some(v) => v,
    };

    let mut files = vec![];
    let mut total_size: u64 = 0;
    for i in 0..10 {
        match form.get(&format!("file-{}", i)) {
            None => {
                return Ok(warp::reply::with_status(
                    "Bad Request",
                    warp::http::StatusCode::BAD_REQUEST,
                ))
            }
            Some(v) => files.push(v),
        }

        match form.get(&format!("file-{}-size", i)) {
            None => {
                return Ok(warp::reply::with_status(
                    "Bad Request",
                    warp::http::StatusCode::BAD_REQUEST,
                ))
            }
            Some(v) => total_size += v.parse::<u64>().unwrap(),
        };
    }

    let full_name = format!("{}/{}.zip", config.compress_dir, &task_id);
    tokio::fs::create_dir_all(config.compress_dir)
        .await
        .unwrap();

    // TODO: use non blocking method
    let fd = std::fs::File::create(full_name).unwrap();
    let mut zip = zip::ZipWriter::new(fd);

    let chunk_size = 1 * 1024 * 1024; // 1MiB
    let mut chunk: Vec<u8> = Vec::with_capacity(chunk_size); // read chunk of 4kB
    chunk.resize(chunk_size, 0);

    let mut last_reported: u64 = 0;
    let threshold = total_size / 20;

    let mut progress = Progress {
        r#type: "compress".into(),
        progress: 0,
        total: total_size as i64,
    };

    let channel = amqp.create_channel().await;

    for filepath in files.iter() {
        let filename = std::path::Path::new(filepath)
            .file_name()
            .unwrap()
            .to_str()
            .unwrap();
        zip.start_file(filename, zip::write::FileOptions::default())
            .unwrap();

        let mut in_fd = tokio::fs::File::open(filepath).await.unwrap();
        loop {
            let mut handle = (&mut in_fd).take(chunk_size as u64);
            let count = handle.read(&mut chunk).await.unwrap();
            if count == 0 {
                break;
            }

            zip.write(&chunk[..count]).unwrap();
            progress.progress += count as u64;

            if progress.progress - last_reported > threshold {
                last_reported = progress.progress;

                channel
                    .basic_publish(
                        &format!("{}_TOPIC", amqp.prefix),
                        &format!("{}.{}", amqp.prefix, task_id),
                        BasicPublishOptions::default(),
                        serde_json::to_vec(&progress).unwrap(),
                        BasicProperties::default(),
                    )
                    .await
                    .unwrap();
            }
        }
    }
    // generate secure hash
    let n = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();
    let expiry = n.as_secs() + 3600; // expired in an hour
    let url_path = format!("files/{}.zip", task_id);
    let payload = format!("{}/{} {}", expiry, url_path, config.hash_secret);
    let digest =
        base64::encode_config::<[u8; 16]>(md5::compute(payload).into(), base64::URL_SAFE_NO_PAD);

    let result = Complete {
        r#type: "complete".into(),
        url: format!(
            "{}/{}?md5={}&expires={}",
            config.file_server_host, url_path, digest, expiry
        ),
    };

    channel
        .basic_publish(
            &format!("{}_TOPIC", amqp.prefix),
            &format!("{}.{}", amqp.prefix, task_id),
            BasicPublishOptions::default(),
            serde_json::to_vec(&result).unwrap(),
            BasicProperties::default(),
        )
        .await
        .unwrap();

    Ok(warp::reply::with_status("OK", warp::http::StatusCode::OK))
}

#[tokio::main]
async fn main() {
    let config = Config::init().unwrap();

    let amqp = amqp::AMQP::init(&config.amqp_addr, &config.amqp_prefix).await;

    let rabbitmq = Arc::new(amqp);

    let compress_endpoint = warp::post()
        .and(warp::filters::path::end())
        .and(warp::body::form())
        .and(with_rabbitmq(rabbitmq))
        .and(with_config(config.clone()))
        .and_then(compress_svc);

    println!("Listened on :{}", config.port);
    warp::serve(compress_endpoint)
        .run(([0, 0, 0, 0], config.port))
        .await;
}
