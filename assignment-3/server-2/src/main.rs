#[macro_use]
extern crate log;
use actix_web::{
    error::ResponseError, http::StatusCode, web, App, HttpRequest, HttpResponse, HttpServer, Result,
};
use async_trait::async_trait;
use flate2::Compression;
use flate2::GzBuilder;
use futures::executor::LocalPool;
use futures::StreamExt;
use lapin::{
    options::{BasicPublishOptions, ExchangeDeclareOptions, QueueBindOptions, QueueDeclareOptions},
    types::FieldTable,
    BasicProperties, Connection, ConnectionProperties,
};
use queryst::parse;
use serde::Serialize;
use std::fmt;
use std::fs::{create_dir_all, remove_dir_all, File};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use threadpool::ThreadPool;
use uuid::Uuid;

static mut SEND_CHANNEL: Option<Arc<Mutex<Sender<CompressionJob>>>> = None;

static N_WORKERS: usize = 4;
static TEMP_BASE_DIR: &str = "./tmp";
static RESULT_BASE_DIR: &str = "./results";
static CHUNK_SIZE: usize = 0x10000; // 64kB

#[async_trait]
trait ProgressPublisher {
    async fn publish_progress(&self, progress: &u8);
}

struct RabbitMQ {
    addr: String,
    exchange: String,
    routing_key: String,
    channel: Option<lapin::Channel>,
}

impl RabbitMQ {
    pub fn new(addr: &str, exchange: &str, routing_key: &str) -> RabbitMQ {
        RabbitMQ {
            addr: addr.to_owned(),
            exchange: exchange.to_owned(),
            routing_key: routing_key.to_owned(),
            channel: None,
        }
    }

    pub async fn connect(&mut self) {
        let conn = Connection::connect(&self.addr, ConnectionProperties::default())
            .await
            .unwrap();
        let channel = conn.create_channel().await.unwrap();

        channel
            .exchange_declare(
                &self.exchange,
                lapin::ExchangeKind::Direct,
                ExchangeDeclareOptions::default(),
                FieldTable::default(),
            )
            .await
            .unwrap();

        channel
            .queue_declare(
                &self.routing_key,
                QueueDeclareOptions::default(),
                FieldTable::default(),
            )
            .await
            .unwrap();

        channel
            .queue_bind(
                &self.routing_key,
                &self.exchange,
                &self.routing_key,
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await
            .unwrap();

        self.channel = Some(channel);
    }
}

#[async_trait]
impl ProgressPublisher for RabbitMQ {
    async fn publish_progress(&self, progress: &u8) {
        let mut payload: Vec<u8> = Vec::with_capacity(1);
        payload.push(*progress);

        self.channel
            .as_ref()
            .unwrap()
            .basic_publish(
                &self.exchange,
                &self.routing_key,
                BasicPublishOptions::default(),
                payload,
                BasicProperties::default(),
            )
            .await
            .unwrap();
    }
}

struct UploadedFile {
    fullpath: String,
    filename: String,
}

impl UploadedFile {
    pub fn new(base_dir: &str, filename: &str, id: Option<&str>) -> UploadedFile {
        let path = id
            .unwrap_or(&Uuid::new_v4().hyphenated().to_string())
            .to_owned();

        let fullpath = Path::new(&base_dir).join(&path).join(&filename);
        UploadedFile {
            fullpath: fullpath.to_str().unwrap().to_owned(),
            filename: filename.to_owned(),
        }
    }

    pub async fn create(&self) -> File {
        let path = Path::new(&self.fullpath).to_owned();
        web::block(|| {
            create_dir_all(path.parent().unwrap()).unwrap();
            File::create(path)
        })
        .await
        .unwrap()
    }

    pub async fn unlink(&self) {
        let path = Path::new(&self.fullpath).to_owned();
        web::block(move || remove_dir_all(path.parent().unwrap()))
            .await
            .unwrap();
    }

    pub async fn open(&self) -> File {
        let filepath = self.fullpath.to_owned();
        web::block(move || File::open(filepath)).await.unwrap()
    }

    pub async fn stream(&self, payload: &mut web::Payload) {
        let f = self.create().await;

        let mut writer = std::io::BufWriter::new(f);

        while let Some(chunk) = payload.next().await {
            let data = chunk.unwrap();
            writer = web::block(move || writer.write_all(&data).map(|_| writer))
                .await
                .unwrap()
        }
        writer.flush().unwrap();
    }

    pub async fn get_filesize(&self) -> u64 {
        let fullpath = self.fullpath.clone();
        web::block(|| std::fs::metadata(fullpath))
            .await
            .unwrap()
            .len()
    }
}

struct CompressionJob {
    file: UploadedFile,
    id: String,
}

impl CompressionJob {
    pub fn new(id: &str, file: UploadedFile) -> CompressionJob {
        CompressionJob {
            id: id.to_owned(),
            file: file,
        }
    }

    pub async fn run(&self, publisher: &dyn ProgressPublisher) {
        let in_fd = self.file.open().await;
        let mut reader = std::io::BufReader::new(in_fd);

        let mut out_filename = self.file.filename.clone();
        out_filename.push_str(&".gz");
        let result = UploadedFile::new(&RESULT_BASE_DIR, &out_filename, Some(&self.id));
        let out_fd = result.create().await;
        let writer = std::io::BufWriter::new(out_fd);

        let filesize = self.file.get_filesize().await as usize;

        let mut nread: usize = 0;
        let mut last_reported = 0;

        info!("compressing: {} started", &self.id);
        let mut e = GzBuilder::new()
            .filename(self.file.filename.clone())
            .write(writer, Compression::default());

        loop {
            let mut chunk = Vec::with_capacity(CHUNK_SIZE); // read chunk of 4kB
            chunk.resize(CHUNK_SIZE, 0);

            let mut handle = (&mut reader).take(CHUNK_SIZE as u64);
            let count = handle.read(&mut chunk).unwrap();
            e.write(&chunk).unwrap();
            nread += count as usize;

            // report compression progress
            // this should not block but whatever
            let progress = (nread * 10 / filesize) as u8;
            if last_reported != progress {
                last_reported = progress;
                publisher.publish_progress(&progress).await;
            }

            if nread == filesize {
                break;
            }
        }
        publisher.publish_progress(&(100 as u8)).await;
        info!("compressing: {} done", &self.id);

        self.file.unlink().await;
        e.finish().unwrap();
    }
}

#[derive(Debug, Serialize)]
struct CompressResult {
    status: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct ErrorResult {
    error: String,
}

fn json_response(obj: impl Serialize) -> HttpResponse {
    HttpResponse::Ok().json(obj)
}

impl fmt::Display for ErrorResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ErrorResult: {}", self.error)
    }
}

impl ResponseError for ErrorResult {
    fn error_response(&self) -> HttpResponse {
        HttpResponse::Ok().json(self)
    }

    fn status_code(&self) -> StatusCode {
        StatusCode::BAD_REQUEST
    }
}

async fn compress(
    req: HttpRequest,
    mut payload: web::Payload,
) -> Result<HttpResponse, ErrorResult> {
    // make sure ?filename is supplied
    let query_string = parse(req.query_string()).unwrap();
    let filename = match query_string.as_object().unwrap().get("filename") {
        Some(val) => val.as_str().unwrap(),
        None => {
            return Err(ErrorResult {
                error: "filename param is required".to_string(),
            })
        }
    };

    // make sure we have X-ROUTING-KEY
    let routing_key: &str = match req.headers().get("X-ROUTING-KEY") {
        Some(val) => val.to_str().unwrap(),
        None => {
            return Err(ErrorResult {
                error: "X-ROUTING-key header must be supplied".to_string(),
            })
        }
    };

    let temp_file = UploadedFile::new(&TEMP_BASE_DIR, &filename, None);
    temp_file.stream(&mut payload).await;

    // run compression on worker
    let job = CompressionJob::new(routing_key, temp_file);
    unsafe {
        let send_channel = SEND_CHANNEL.clone();
        web::block(move || {
            let mutex = send_channel.unwrap();
            let guard = mutex.lock().unwrap();
            guard.send(job)
        })
        .await
        .unwrap();
    }

    Ok(json_response(CompressResult {
        status: Some(String::from("ok")),
        error: None,
    }))
}

#[actix_rt::main]
async fn main() -> std::io::Result<()> {
    env_logger::init();
    // Configurations from env variables
    let port = std::env::var_os("PORT").unwrap_or("8081".into());
    let amqp_addr = std::env::var_os("AMQP_ADDR").unwrap();
    let amqp_exchange = std::env::var_os("AMQP_EXCHANGE").unwrap();

    // initialize channel
    let (tx, rx) = channel();
    unsafe {
        SEND_CHANNEL = Some(Arc::new(Mutex::new(tx)));
    }

    // master worker thread
    std::thread::spawn(move || {
        let pool = ThreadPool::new(N_WORKERS);

        loop {
            let job = rx.recv().unwrap();
            let addr = amqp_addr.to_str().unwrap().to_owned();
            let exchange = amqp_exchange.to_str().unwrap().to_owned();
            pool.execute(move || {
                let mut executor = LocalPool::new();
                executor.run_until(async {
                    let mut conn = RabbitMQ::new(&addr, &exchange, &job.id);
                    conn.connect().await;
                    job.run(&conn).await;
                })
            })
        }
    });

    let connstr = format!("0.0.0.0:{}", port.to_str().unwrap());

    println!("listening on {}", connstr);
    HttpServer::new(|| App::new().route("/compress", web::post().to(compress)))
        .bind(connstr)?
        .run()
        .await
}
