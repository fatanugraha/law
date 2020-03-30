use actix_files::NamedFile;
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

#[derive(Template)]
#[template(path = "upload-progress.html")]
struct SuccessUploadTemplate<'a> {
    routing_key: &'a str,
}

#[derive(Template)]
#[template(path = "upload-error.html")]
struct ErrorUploadTemplate<'a> {
    reason: &'a str,
}

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
            let res = Client::new()
                .post("http://localhost:8081/compress")
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

async fn index() -> Result<NamedFile> {
    Ok(NamedFile::open("templates/index.html")?)
}

#[actix_rt::main]
async fn main() -> std::io::Result<()> {
    HttpServer::new(|| {
        App::new()
            .route("/", web::get().to(index))
            .route("/upload", web::post().to(upload))
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}
