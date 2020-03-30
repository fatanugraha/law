#![feature(proc_macro_hygiene, decl_macro)]

#[macro_use]
extern crate rocket;
#[macro_use]
extern crate rocket_contrib;

use reqwest::StatusCode;
use rocket_contrib::json::JsonValue;
use rocket_contrib::serve::StaticFiles;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
struct Repository {
    full_name: String,
    html_url: String,
    description: Option<String>,
    stargazers_count: i32,
    open_issues_count: i32,
    forks_count: i32,
}

#[derive(Deserialize)]
struct Error {
    message: String,
}

fn get_github_repo_info(username: &String, repository: &String) -> Result<Repository, String> {
    let url = format!("https://api.github.com/repos/{}/{}", username, repository);

    let resp = reqwest::blocking::Client::new()
        .get(&url)
        .header("User-Agent", "reqwest")
        .send();

    let resp = match resp {
        Ok(r) => r,
        Err(_e) => return Err(String::from("Can't reach Github API")),
    };

    match resp.status() {
        StatusCode::OK => {
            let r: Repository = resp.json().unwrap();
            Ok(r)
        }
        StatusCode::NOT_FOUND => return Err(String::from("User or Repoistory not found")),
        StatusCode::FORBIDDEN => {
            let r: Error = resp.json().unwrap();
            return Err(r.message);
        }
        s => return Err(format!("Received status code {}", s)),
    }
}

#[get("/<user>/<repo>")]
fn get_repo_info(user: String, repo: String) -> JsonValue {
    match get_github_repo_info(&user, &repo) {
        Ok(repository) => json!({"status": "success", "data": repository}),
        Err(_error) => json!({"status": "error", "error": _error}),
    }
}

fn main() {
    rocket::ignite()
        .mount("/", StaticFiles::from("static"))
        .mount("/api", routes![get_repo_info])
        .launch();
}
