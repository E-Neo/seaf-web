use clap::{
    crate_authors, crate_description, crate_name, crate_version, App, AppSettings, Arg, ArgMatches,
    SubCommand,
};
use lazy_static::lazy_static;
use regex::Regex;
use reqwest::blocking::Client;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

const BASE_URL: &str = "https://cloud.tsinghua.edu.cn";

fn get_first_page(client: &Client, token: &str) -> reqwest::Result<Html> {
    Ok(Html::parse_document(
        &client
            .get(&format!("{}/u/d/{}/", BASE_URL, token))
            .send()?
            .text()?,
    ))
}

fn extract_token(document: &Html) -> Result<&str, Box<dyn std::error::Error>> {
    let form = document
        .select(&Selector::parse("form#share-passwd-form").unwrap())
        .next()
        .ok_or("no such form")?;
    let input_selector = Selector::parse("input").unwrap();
    let mut inputs = form.select(&input_selector);
    Ok(inputs
        .next()
        .ok_or("no such input")?
        .value()
        .attr("value")
        .ok_or("no csrfmiddlewaretoken")?)
}

#[derive(Serialize)]
struct SharePasswdForm<'a> {
    csrfmiddlewaretoken: &'a str,
    token: &'a str,
    password: &'a str,
}

fn post_password(
    client: &Client,
    token: &str,
    csrfmiddlewaretoken: &str,
    password: &str,
) -> Result<Html, Box<dyn std::error::Error>> {
    Ok(Html::parse_document(
        &client
            .post(&format!("{}/u/d/{}/", BASE_URL, token))
            .form(&SharePasswdForm {
                csrfmiddlewaretoken,
                token,
                password,
            })
            .send()?
            .text()?,
    ))
}

fn extract_repo_id(document: &Html) -> Result<&str, Box<dyn std::error::Error>> {
    lazy_static! {
        static ref RE: Regex = Regex::new(concat!(
            r"'/ajax/u/d/[0-9a-f]{20}/upload/\?r=",
            r"([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})'"
        ))
        .unwrap();
    }
    for script in document.select(&Selector::parse("script").unwrap()) {
        for text in script.text() {
            if let Some(caps) = RE.captures(text) {
                return Ok(caps.get(1).unwrap().into());
            }
        }
    }
    Err("invalid token/password".into())
}

#[derive(Deserialize)]
struct UploadUrl {
    url: String,
}

fn get_upload_url(
    client: &Client,
    token: &str,
    repo_id: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let url = format!(
        "{}/ajax/u/d/{}/upload/?r={}&_={}",
        BASE_URL,
        token,
        repo_id,
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis()
    );
    let upload_url: UploadUrl = client
        .get(&url)
        .header("X-Requested-With", "XMLHttpRequest")
        .send()?
        .json()?;
    Ok(upload_url.url)
}

#[derive(Deserialize)]
struct FileUploadResp {
    name: String,
    id: String,
    size: usize,
}

fn upload_file(
    client: &Client,
    url: &str,
    file_path: &str,
) -> Result<FileUploadResp, Box<dyn std::error::Error>> {
    let form = reqwest::blocking::multipart::Form::new()
        .text("parent_dir", "/")
        .file("file", file_path)?;
    let mut resps: Vec<FileUploadResp> = client.post(url).multipart(form).send()?.json()?;
    resps.pop().ok_or("file upload failed".into())
}

fn handle_upload(matches: &ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    let token = matches.value_of("TOKEN").unwrap();
    let file_path = matches.value_of("FILEPATH").unwrap();
    if !Path::new(file_path).exists() {
        return Err("no such file".into());
    }
    let client = Client::builder().timeout(None).cookie_store(true).build()?;
    let document = get_first_page(&client, token)?;
    let password = rpassword::prompt_password_stdout("Password: ").unwrap();
    let document = post_password(&client, token, extract_token(&document)?, &password)?;
    let upload_url = get_upload_url(&client, token, extract_repo_id(&document)?)?;
    let resp = upload_file(&client, &upload_url, file_path)?;
    println!("{} {} {}", resp.id, resp.name, resp.size);
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let matches = App::new(crate_name!())
        .setting(AppSettings::ArgRequiredElseHelp)
        .version(crate_version!())
        .about(crate_description!())
        .author(crate_authors!())
        .subcommand(
            SubCommand::with_name("upload")
                .about("Uploads local file to cloud")
                .arg(Arg::with_name("TOKEN").required(true))
                .arg(Arg::with_name("FILEPATH").required(true)),
        )
        .get_matches();
    if let Some(matches) = matches.subcommand_matches("upload") {
        handle_upload(&matches)?;
    }
    Ok(())
}
