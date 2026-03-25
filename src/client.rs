use std::path::{Path, PathBuf};
use std::{fs, io};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use ureq::http::header::{CONTENT_DISPOSITION, HeaderMap};
use ureq::http::StatusCode;
use ureq::{Agent, Body, http};
use url::Url;

use crate::filter::filter_files;
use crate::model::{ArtifactFilter, ReleaseDescription, Version};
use crate::parse;

const RELEASES_URL: &str = "https://releases.1c.ru";
const PROJECTS_URL: &str = "/project/";
const LOGIN_URL: &str = "https://login.1c.ru";
const TICKET_URL: &str = "https://login.1c.ru/rest/public/ticket/get";

#[derive(Debug, Serialize)]
struct TicketRequest<'a> {
    login: &'a str,
    password: &'a str,
    #[serde(rename = "serviceNick")]
    service_nick: &'a str,
}

#[derive(Debug, Deserialize)]
struct TicketResponse {
    ticket: String,
}

#[derive(Clone)]
pub struct OnecClient {
    login: String,
    password: String,
    http: Agent,
    verbose: bool,
}

impl OnecClient {
    pub fn new(login: impl Into<String>, password: impl Into<String>) -> Result<Self> {
        let login = login.into();
        let password = password.into();
        if login.is_empty() || password.is_empty() {
            bail!("ONEC_USERNAME and ONEC_PASSWORD must be set");
        }

        let http: Agent = Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .build()
            .into();

        Ok(Self {
            login,
            password,
            http,
            verbose: false,
        })
    }

    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    pub fn auth(&self) -> Result<()> {
        self.log("requesting auth ticket");
        let continue_url = self.get_auth_token(RELEASES_URL)?;
        self.log(&format!("following auth redirect: {continue_url}"));
        self.follow_get(&continue_url)?;
        self.log("authorization completed");
        Ok(())
    }

    pub fn download_release(
        &self,
        release: &ReleaseDescription,
        destination: impl AsRef<Path>,
    ) -> Result<Vec<PathBuf>> {
        self.log(&format!(
            "starting download: project={}, version={}, output={}",
            release.project,
            release.version,
            destination.as_ref().display()
        ));
        self.auth()?;
        let version = self.version_info(&release.project, &release.version)?;
        self.download_matching_files(&version, &release.filter, destination)
    }

    pub fn version_info(&self, project: &str, version: &str) -> Result<Version> {
        self.log(&format!("loading project page for {project}"));
        let page = self.project_page(project)?;
        let versions = parse::versions(&page);
        self.log(&format!("parsed {} versions for {project}", versions.len()));
        let available_versions = versions
            .iter()
            .take(10)
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let mut version_info = versions
            .into_iter()
            .find(|item| item.name == version)
            .with_context(|| {
                if available_versions.is_empty() {
                    format!("version {version} for {project} not found; no versions were parsed from the project page")
                } else {
                    format!(
                        "version {version} for {project} not found; parsed versions include: {available_versions}"
                    )
                }
            })?;

        self.log(&format!(
            "matched version {version}; loading release files from {}",
            version_info.url
        ));
        version_info.files = self.version_files(&version_info.url)?;
        self.log(&format!(
            "parsed {} release files for version {version}",
            version_info.files.len()
        ));
        Ok(version_info)
    }

    pub fn project_page(&self, project: &str) -> Result<String> {
        self.get_text(&format!("{PROJECTS_URL}{project}?allUpdates=true"))
    }

    pub fn version_files(&self, version_url: &str) -> Result<Vec<crate::model::ReleaseFile>> {
        let page = self.get_text(version_url)?;
        Ok(parse::release_files(&page))
    }

    pub fn download_matching_files(
        &self,
        version: &Version,
        artifact_filter: &ArtifactFilter,
        destination: impl AsRef<Path>,
    ) -> Result<Vec<PathBuf>> {
        let files = filter_files(&version.files, artifact_filter);
        self.log(&format!(
            "filtered {} matching files out of {}",
            files.len(),
            version.files.len()
        ));
        if files.is_empty() {
            bail!("no files found for filter {:?}", artifact_filter);
        }

        fs::create_dir_all(destination.as_ref())
            .with_context(|| format!("failed to create {}", destination.as_ref().display()))?;

        let mut downloaded = Vec::new();

        for file in files {
            self.log(&format!("loading download page for {}", file.name));
            let links = parse::file_download_links(&self.get_text(&file.url)?);
            if links.is_empty() {
                self.log(&format!("no download links found for {}", file.name));
                continue;
            }

            for link in links {
                self.log(&format!("attempting download from {link}"));
                if let Some(path) = self.download_file(&link, destination.as_ref())? {
                    self.log(&format!("downloaded {}", path.display()));
                    downloaded.push(path);
                    break;
                }
            }
        }

        Ok(downloaded)
    }

    pub fn get_text(&self, url: &str) -> Result<String> {
        let mut response = self.get(url)?;
        Ok(response.body_mut().read_to_string()?)
    }

    fn get(&self, url: &str) -> Result<Response> {
        let full_url = absolute_url(url)?;
        self.log(&format!("GET {}", full_url.as_str()));
        let mut response = self.follow_get(full_url.as_str())?;

        if response.status() == StatusCode::UNAUTHORIZED {
            self.log(&format!(
                "received 401 for {}; refreshing auth",
                full_url.as_str()
            ));
            let continue_url = self.get_auth_token(full_url.as_str())?;
            response = self.follow_get(&continue_url)?;
        }

        self.ensure_ok(response)
    }

    fn follow_get(&self, url: &str) -> Result<Response> {
        let mut current = absolute_url(url)?;
        loop {
            let response = self
                .http
                .get(current.as_str())
                .call()
                .with_context(|| format!("GET {current} failed"))?;

            if response.status().is_redirection() {
                let location = redirect_location(response.headers(), &current)?;
                self.log(&format!("redirect: {current} -> {location}"));
                current = location;
                continue;
            }

            return Ok(response);
        }
    }

    fn get_auth_token(&self, service_url: &str) -> Result<String> {
        let body = TicketRequest {
            login: &self.login,
            password: &self.password,
            service_nick: service_url,
        };

        self.log(&format!("POST {TICKET_URL} for service {service_url}"));
        let mut response = self
            .http
            .post(TICKET_URL)
            .send_json(&body)
            .context("ticket request failed")?;

        response = self.ensure_ok(response)?;
        let data: TicketResponse = response.body_mut().read_json().context("invalid ticket response")?;
        self.log("received auth ticket");
        Ok(format!("{LOGIN_URL}/ticket/auth?token={}", data.ticket))
    }

    fn ensure_ok(&self, mut response: Response) -> Result<Response> {
        if response.status() == StatusCode::OK {
            return Ok(response);
        }

        let status = response.status();
        let body = response.body_mut().read_to_string().unwrap_or_default();
        Err(anyhow!("response error: status={status}, body={body}"))
    }

    fn download_file(&self, url: &str, output_dir: &Path) -> Result<Option<PathBuf>> {
        let mut response = self.get(url)?;
        let file_name = extract_file_name(&response)
            .with_context(|| format!("failed to extract file name for {url}"))?;
        let destination = output_dir.join(file_name);

        if destination.try_exists()? {
            self.log(&format!("skipping existing file {}", destination.display()));
            return Ok(Some(destination));
        }

        self.log(&format!("writing file {}", destination.display()));
        let mut file = fs::File::create(&destination)
            .with_context(|| format!("failed to create {}", destination.display()))?;
        let mut reader = response.body_mut().as_reader();
        io::copy(&mut reader, &mut file).context("failed to write response body")?;

        Ok(Some(destination))
    }

    fn log(&self, message: &str) {
        if self.verbose {
            eprintln!("[onec-download-rs] {message}");
        }
    }
}

type Response = http::Response<Body>;

fn absolute_url(url: &str) -> Result<Url> {
    if let Ok(parsed) = Url::parse(url) {
        return Ok(parsed);
    }

    Url::parse(RELEASES_URL)?
        .join(url)
        .with_context(|| format!("invalid URL {url}"))
}

fn redirect_location(headers: &HeaderMap, current: &Url) -> Result<Url> {
    let location = headers
        .get("location")
        .ok_or_else(|| anyhow!("redirect response missing Location header"))?
        .to_str()
        .context("redirect location is not valid UTF-8")?;
    current
        .join(location)
        .with_context(|| format!("invalid redirect target {location}"))
}

fn extract_file_name(response: &Response) -> Result<String> {
    let header = response
        .headers()
        .get(CONTENT_DISPOSITION)
        .ok_or_else(|| anyhow!("missing Content-Disposition header"))?
        .to_str()
        .context("Content-Disposition is not valid UTF-8")?;

    parse_content_disposition_filename(header)
}

fn parse_content_disposition_filename(header: &str) -> Result<String> {
    let prefix = "filename=";
    let start = header
        .find(prefix)
        .ok_or_else(|| anyhow!("Content-Disposition has no filename"))?
        + prefix.len();
    let mut file_name = header[start..].trim().to_owned();

    if file_name.starts_with('"') && file_name.ends_with('"') && file_name.len() >= 2 {
        file_name = file_name[1..file_name.len() - 1].to_owned();
    }

    Ok(file_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ureq::http::HeaderValue;

    #[test]
    fn extracts_filename() {
        assert_eq!(
            parse_content_disposition_filename("attachment; filename=\"setup.exe\"").unwrap(),
            "setup.exe"
        );
    }

    #[test]
    fn resolves_relative_redirect_location() {
        let mut headers = HeaderMap::new();
        headers.insert("location", HeaderValue::from_static("/next"));

        let location =
            redirect_location(&headers, &Url::parse("https://example.com/a").unwrap()).unwrap();

        assert_eq!(location.as_str(), "https://example.com/next");
    }
}
