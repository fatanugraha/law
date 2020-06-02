use askama::Template;
use std::collections::HashMap;
use warp::Filter; // bring trait in scope

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

fn reply_download_progress(form: HashMap<String, String>) -> impl warp::reply::Reply {
    let mut template = DownloadProgress { files: vec![] };
    for i in 0..FILE_NUM {
        if let Some(file_url) = form.get(&format!("file-{}", i)) {
            template.files.push(file_url)
        } else {
            return warp::reply::html(format!("file-{} is required", i))
        }
    }

    warp::reply::html(template.render().unwrap())
}

#[tokio::main]
async fn main() {
    let download_form = warp::get()
        .and(warp::filters::path::end())
        .map(reply_download_form);

    let process_form = warp::post()
        .and(warp::filters::path::end())
        .and(warp::body::form())
        .map(reply_download_progress);

    let filters = download_form.or(process_form);
    warp::serve(filters).run(([0, 0, 0, 0], 8000)).await;
}
