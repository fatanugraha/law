use actix_multipart::{Field, Multipart};
use actix_web::{web, App, HttpResponse, HttpServer, Result};
use askama::Template;
use futures::StreamExt;
use reqwest::blocking::{Body, Client};
use sanitize_filename::sanitize;
use std::fs::{create_dir_all, remove_dir_all, File};
use std::io::Write;
use std::path::Path;
use uuid::Uuid;

struct Config {
    upload_url: String,
    ws_url: String,
    vhost: String,
    password: String,
    username: String,
}

static mut CONFIG: Option<Config> = None;

#[derive(Template)]
#[template(path = "upload-progress.html", escape = "none")]
struct SuccessUploadTemplate<'a> {
    routing_key: &'a str,
    config: &'a Config,
}

#[derive(Template)]
#[template(path = "upload-error.html")]
struct ErrorUploadTemplate<'a> {
    reason: &'a str,
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate {}

fn template_response(template: impl Template) -> Result<HttpResponse> {
    Ok(HttpResponse::Ok()
        .content_type("text/html")
        .body(template.render().unwrap()))
}

async fn stream_to_file(field: &mut Field, filename: String) -> std::fs::File {
    let cloned_filename = filename.clone();
    let f = web::block(move || {
        let filepath = Path::new(&cloned_filename);
        create_dir_all(filepath.parent().unwrap()).unwrap();
        File::create(cloned_filename)
    })
    .await
    .unwrap();

    let mut buf_writer = std::io::BufWriter::new(f);
    while let Some(chunk) = field.next().await {
        let data = chunk.unwrap();
        buf_writer = web::block(move || buf_writer.write_all(&data).map(|_| buf_writer))
            .await
            .unwrap()
    }
    buf_writer.flush().unwrap();

    File::open(filename).unwrap()
}

async fn upload(mut payload: Multipart) -> Result<HttpResponse> {
    let routing_key = Uuid::new_v4().hyphenated().to_string();

    while let Some(item) = payload.next().await {
        let mut field = item?;
        let content_type = field.content_disposition().unwrap();

        // discard all fields except "file"
        if content_type.get_name() != Some("file") {
            continue;
        }

        let filename = match content_type.get_filename() {
            Some(name) => sanitize(name),
            None => String::from("default.bin"),
        };

        let filepath = Path::new("./tmp").join(&routing_key).join(&filename);

        let f = stream_to_file(&mut field, filepath.to_str().unwrap().to_owned()).await;

        let cloned_routing_key = routing_key.clone();
        let res = web::block(move || {
            let url: String;
            unsafe {
                url = CONFIG.as_ref().unwrap().upload_url.to_owned();
            }

            let res = Client::new()
                .post(&url)
                .query(&[("filename", filename)])
                .header("X-ROUTING-KEY", cloned_routing_key)
                .body(Body::from(f))
                .send();
            remove_dir_all(filepath.parent().unwrap().to_str().unwrap()).unwrap();
            res
        })
        .await;

        return match res {
            Ok(_) => template_response(SuccessUploadTemplate {
                routing_key: &routing_key,
                config: get_config(),
            }),
            Err(e) => template_response(ErrorUploadTemplate {
                reason: &format!("{}", e),
            }),
        };
    }

    template_response(ErrorUploadTemplate {
        reason: &String::from("malformed request"),
    })
}

async fn index() -> Result<HttpResponse> {
    template_response(IndexTemplate {})
}

fn get_os_var(key: &str) -> String {
    std::env::var_os(key).unwrap().to_str().unwrap().to_owned()
}

fn get_config() -> &'static Config {
    unsafe { (&CONFIG).as_ref().unwrap() }
}

#[actix_rt::main]
async fn main() -> std::io::Result<()> {
    unsafe {
        CONFIG = Some(Config {
            upload_url: get_os_var("UPLOAD_URL"),
            vhost: get_os_var("VHOST"),
            ws_url: get_os_var("WS_URL"),
            username: get_os_var("RABBITMQ_USERNAME"),
            password: get_os_var("RABBITMQ_PASSWORD"),
        });
    }

    HttpServer::new(|| {
        App::new()
            .route("/", web::get().to(index))
            .route("/upload", web::post().to(upload))
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}
