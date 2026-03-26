use std::cell::RefCell;
use std::fs;
use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use scraper::{Html, Selector};
use terminal_size::{Width, terminal_size};
use ureq::http::StatusCode;
use ureq::http::header::{CONTENT_DISPOSITION, CONTENT_LENGTH, HeaderMap};
use ureq::{Agent, Body, ResponseExt, http};
use url::Url;

use crate::filter::filter_files;
use crate::model::{ArtifactFilter, ReleaseDescription, ReleaseFile, Version};
use crate::parse;

const RELEASES_URL: &str = "https://releases.1c.ru";
const PROJECTS_URL: &str = "/project/";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36";

pub struct OnecClient {
    login: String,
    password: String,
    http: Agent,
    verbose: bool,
    trace: bool,
    quiet: bool,
    progress: RefCell<ProgressDisplay>,
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
            trace: false,
            quiet: false,
            progress: RefCell::new(ProgressDisplay::new()),
        })
    }

    pub fn with_logging(mut self, verbose: bool, trace: bool) -> Self {
        self.verbose = verbose;
        self.trace = trace;
        self.progress.get_mut().enabled = !verbose && !trace && std::io::stderr().is_terminal();
        self
    }

    pub fn with_quiet(mut self, quiet: bool) -> Self {
        self.quiet = quiet;
        if quiet {
            self.progress.get_mut().enabled = false;
        }
        self
    }

    pub fn auth(&self) -> Result<()> {
        self.auth_by_form()?;
        self.progress.borrow_mut().set_auth_done();
        self.progress.borrow_mut().render()?;
        self.stage("authentication completed");
        self.log("authorization completed via form flow");
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

    pub fn matching_release_files(
        &self,
        release: &ReleaseDescription,
    ) -> Result<Vec<ReleaseFile>> {
        self.log(&format!(
            "starting candidate selection: project={}, version={}",
            release.project, release.version
        ));
        self.auth()?;
        let version = self.version_info(&release.project, &release.version)?;
        self.matching_files_for_version(&version, &release.filter)
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
        let mut version_info = resolve_requested_version(&versions, version).with_context(|| {
            if available_versions.is_empty() {
                format!("version {version} for {project} not found; no versions were parsed from the project page")
            } else {
                format!(
                    "version {version} for {project} not found; parsed versions include: {available_versions}"
                )
            }
        })?;

        self.announce_resolved_version(project, version, &version_info.name);
        self.log(&format!(
            "matched version {}; loading release files from {}",
            version_info.name,
            version_info.url
        ));
        version_info.files = self.version_files(&version_info.url)?;
        self.log(&format!(
            "parsed {} release files for version {}",
            version_info.files.len(),
            version_info.name
        ));
        Ok(version_info)
    }

    fn announce_resolved_version(&self, project: &str, requested: &str, resolved: &str) {
        let message = if requested.eq_ignore_ascii_case("latest") || requested != resolved {
            format!(
                "selected version {resolved} for {project} (requested {requested})"
            )
        } else {
            format!("selected version {resolved} for {project}")
        };

        if self.trace {
            self.emit_log("trace", "catalog", &message);
        } else if self.verbose {
            self.emit_log("info", "catalog", &message);
        } else if self.progress.borrow().enabled {
            self.progress.borrow_mut().set_selected_version(message);
            let _ = self.progress.borrow_mut().render();
        } else {
            eprintln!("{}", format_stage(&message));
        }
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
        let files = self.matching_files_for_version(version, artifact_filter)?;
        self.progress
            .borrow_mut()
            .set_files(files.iter().map(|file| file.name.clone()).collect());
        self.progress.borrow_mut().render()?;
        self.stage(&format!("found {} files for release", files.len()));

        fs::create_dir_all(destination.as_ref())
            .with_context(|| format!("failed to create {}", destination.as_ref().display()))?;

        let mut downloaded = Vec::new();

        for file in files {
            self.log(&format!("loading download page for {}", file.name));
            let links = parse::file_download_links(&self.get_text(&file.url)?);
            if links.is_empty() {
                self.log(&format!("no download links found for {}", file.name));
                self.progress
                    .borrow_mut()
                    .mark_failed(&file.name, "no links".to_owned());
                self.progress.borrow_mut().render()?;
                continue;
            }

            for link in links {
                self.log(&format!("attempting download from {link}"));
                if let Some(path) = self.download_file(&file.name, &link, destination.as_ref())? {
                    self.log(&format!("downloaded {}", path.display()));
                    downloaded.push(path);
                    break;
                }
            }
        }

        Ok(downloaded)
    }

    fn matching_files_for_version(
        &self,
        version: &Version,
        artifact_filter: &ArtifactFilter,
    ) -> Result<Vec<ReleaseFile>> {
        let files = filter_files(&version.files, artifact_filter);
        self.log(&format!(
            "filtered {} matching files out of {}",
            files.len(),
            version.files.len()
        ));
        if files.is_empty() {
            let available_files = version
                .files
                .iter()
                .take(10)
                .map(|file| file.name.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            if available_files.is_empty() {
                bail!("no files found for filter {:?}; version contains no files", artifact_filter);
            }

            bail!(
                "no files found for filter {:?}; available files include: {}",
                artifact_filter,
                available_files
            );
        }

        Ok(files)
    }

    pub fn get_text(&self, url: &str) -> Result<String> {
        let mut response = self.get(url)?;
        Ok(response.body_mut().read_to_string()?)
    }

    fn get(&self, url: &str) -> Result<Response> {
        let full_url = absolute_url(url)?;
        self.log(&format!("GET {}", full_url.as_str()));
        let mut response = self.follow_get(full_url.as_str())?;

        if is_login_uri(response.get_uri()) {
            self.log(&format!(
                "request resolved to login page for {}; refreshing auth",
                full_url.as_str()
            ));
            self.auth_by_form()
                .context("re-authorization via form did not establish a releases.1c.ru session")?;
            response = self.follow_get(full_url.as_str())?;
        }

        if response.status() == StatusCode::UNAUTHORIZED {
            self.log(&format!(
                "received 401 for {}; refreshing auth",
                full_url.as_str()
            ));
            self.auth_by_form().context(
                "401 re-authorization via form did not establish a releases.1c.ru session",
            )?;
            response = self.follow_get(full_url.as_str())?;
        }

        self.ensure_not_login_page(&response)?;
        self.ensure_ok(response)
    }

    fn follow_get(&self, url: &str) -> Result<Response> {
        let mut current = absolute_url(url)?;
        loop {
            self.trace(&format!(
                "GET headers: User-Agent={}, Accept=*/*",
                USER_AGENT
            ));
            let response = self
                .http
                .get(current.as_str())
                .header("User-Agent", USER_AGENT)
                .header("Accept", "*/*")
                .call()
                .with_context(|| format!("GET {current} failed"))?;
            self.log_response_meta("GET response", &response);

            if response.status().is_redirection() {
                let location = redirect_location(response.headers(), &current)?;
                self.trace(&format!("redirect: {current} -> {location}"));
                current = location;
                continue;
            }

            return Ok(response);
        }
    }

    fn auth_by_form(&self) -> Result<()> {
        self.log(&format!("loading login form from {RELEASES_URL}"));
        let login_page = self.follow_get(RELEASES_URL)?;
        let login_page_url = login_page.get_uri().to_string();
        let mut login_page = self.ensure_ok(login_page)?;
        let html = login_page
            .body_mut()
            .read_to_string()
            .context("failed to read login form")?;

        let (form_url, body) =
            build_login_form_request(&html, &login_page_url, &self.login, &self.password)?;
        self.trace(&format!("login page final url: {login_page_url}"));
        self.trace(&format!(
            "login page snippet: {}",
            truncate_for_log(&html, 400)
        ));
        self.log(&format!("submitting login form to {form_url}"));
        self.trace(&format!(
            "POST headers: Content-Type=application/x-www-form-urlencoded, Referer={}, Accept=*/*, User-Agent={}",
            login_page_url, USER_AGENT
        ));
        self.trace(&format!("POST body: {}", redact_form_body(&body)));
        let response = self
            .http
            .post(&form_url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("Referer", &login_page_url)
            .header("Accept", "*/*")
            .header("User-Agent", USER_AGENT)
            .send(body)
            .with_context(|| format!("POST {form_url} failed"))?;
        self.log_response_meta("POST response", &response);
        let mut response = self.follow_form_post_redirects(response, &form_url)?;
        response = self.ensure_ok(response)?;
        if is_login_uri(response.get_uri()) {
            let html = response.body_mut().read_to_string().unwrap_or_default();
            self.trace(&format!(
                "login response snippet: {}",
                truncate_for_log(&html, 800)
            ));
            let details = extract_login_error(&html).unwrap_or_else(|| {
                "login page returned without an explicit error message".to_owned()
            });
            bail!("form auth redirected back to login: {details}");
        }
        self.verify_session()
    }

    fn verify_session(&self) -> Result<()> {
        self.log(&format!("verifying session with {RELEASES_URL}"));
        let response = self.follow_get(RELEASES_URL)?;
        self.ensure_not_login_page(&response)
            .context("session verification failed")?;
        self.ensure_ok(response)?;
        Ok(())
    }

    fn ensure_ok(&self, mut response: Response) -> Result<Response> {
        if response.status() == StatusCode::OK {
            return Ok(response);
        }

        let status = response.status();
        let body = response.body_mut().read_to_string().unwrap_or_default();
        Err(anyhow!("response error: status={status}, body={body}"))
    }

    fn download_file(
        &self,
        file_label: &str,
        url: &str,
        output_dir: &Path,
    ) -> Result<Option<PathBuf>> {
        let mut response = self.get(url)?;
        let file_name = extract_file_name(&response)
            .with_context(|| format!("failed to extract file name for {url}"))?;
        let destination = output_dir.join(file_name);
        let total_bytes = response
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok());

        if destination.try_exists()? {
            self.log(&format!("skipping existing file {}", destination.display()));
            self.progress.borrow_mut().mark_done(
                file_label,
                format!("already exists {}", short_path(&destination)),
            );
            self.progress.borrow_mut().render()?;
            self.stage(&format!("already downloaded {}", destination.display()));
            return Ok(Some(destination));
        }

        self.log(&format!("writing file {}", destination.display()));
        self.progress
            .borrow_mut()
            .mark_active(file_label, progress_status(0, total_bytes));
        self.progress.borrow_mut().render()?;
        self.stage(&format!(
            "downloading {}{}",
            destination.display(),
            total_bytes
                .map(|size| format!(" ({})", human_size(size)))
                .unwrap_or_default()
        ));
        let mut file = fs::File::create(&destination)
            .with_context(|| format!("failed to create {}", destination.display()))?;
        let mut reader = response.body_mut().as_reader();
        let mut buffer = [0_u8; 64 * 1024];
        let mut downloaded = 0_u64;
        let mut last_reported = 0_u64;

        loop {
            let read = reader
                .read(&mut buffer)
                .context("failed to read response body")?;
            if read == 0 {
                break;
            }

            file.write_all(&buffer[..read])
                .context("failed to write response body")?;
            downloaded += read as u64;

            if should_report_progress(downloaded, last_reported, total_bytes) {
                self.progress
                    .borrow_mut()
                    .update_progress(file_label, render_progress_line(downloaded, total_bytes));
                self.progress.borrow_mut().render()?;
                self.file_progress(&destination, downloaded, total_bytes);
                last_reported = downloaded;
            }
        }
        file.flush().context("failed to flush response body")?;
        self.progress
            .borrow_mut()
            .mark_done(file_label, progress_status(downloaded, total_bytes));
        self.progress.borrow_mut().render()?;
        self.file_progress(&destination, downloaded, total_bytes);
        self.stage(&format!("downloaded {}", destination.display()));

        Ok(Some(destination))
    }

    fn stage(&self, message: &str) {
        if self.quiet {
            return;
        }
        if !self.verbose && !self.trace {
            let progress = self.progress.borrow_mut();
            if !progress.enabled {
                eprintln!("{}", format_stage(message));
            }
        }
    }

    fn log(&self, message: &str) {
        if self.quiet {
            return;
        }
        if self.verbose {
            self.emit_log("info", scope_for_message(message), message);
        }
    }

    fn trace(&self, message: &str) {
        if self.quiet {
            return;
        }
        if self.trace {
            self.emit_log("trace", scope_for_message(message), message);
        }
    }

    fn emit_log(&self, level: &str, scope: &str, message: &str) {
        eprintln!("[onec-download-rs] {:<5} {:<8} | {}", level, scope, message);
    }

    fn file_progress(&self, path: &Path, downloaded: u64, total: Option<u64>) {
        if self.quiet {
            return;
        }
        let message = match total {
            Some(total) if total > 0 => format!(
                "{} {} {:>3}%  {}/{}",
                short_path(path),
                progress_bar(downloaded, total, 24),
                ((downloaded.saturating_mul(100)) / total).min(100),
                human_size(downloaded),
                human_size(total)
            ),
            _ => format!(
                "{} {}  {}",
                short_path(path),
                indeterminate_bar(downloaded),
                human_size(downloaded)
            ),
        };

        if self.verbose || self.trace {
            self.log(&message);
        } else {
            let progress = self.progress.borrow();
            if !progress.enabled {
                eprintln!("{}", format_progress(&message));
            }
        }
    }

    fn ensure_not_login_page(&self, response: &Response) -> Result<()> {
        if is_login_uri(response.get_uri()) {
            bail!("unexpected redirect to login page: {}", response.get_uri());
        }

        Ok(())
    }

    fn follow_form_post_redirects(
        &self,
        response: Response,
        current_url: &str,
    ) -> Result<Response> {
        let mut response = response;
        let mut current = absolute_url(current_url)?;

        loop {
            if !response.status().is_redirection() {
                return Ok(response);
            }

            let location = redirect_location(response.headers(), &current)?;
            self.trace(&format!("form redirect: {current} -> {location}"));
            current = location;
            response = self
                .http
                .get(current.as_str())
                .header("User-Agent", USER_AGENT)
                .header("Accept", "*/*")
                .call()
                .with_context(|| format!("GET {current} failed"))?;
            self.log_response_meta("form redirect response", &response);
        }
    }

    fn log_response_meta(&self, label: &str, response: &Response) {
        if !self.trace {
            return;
        }

        let location = response
            .headers()
            .get("location")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("<none>");
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("<none>");
        let set_cookie_count = response.headers().get_all("set-cookie").iter().count();

        self.trace(&format!(
            "{label}: status={}, uri={}, location={}, content-type={}, set-cookie-count={}",
            response.status(),
            response.get_uri(),
            location,
            content_type,
            set_cookie_count
        ));
    }
}

impl Drop for OnecClient {
    fn drop(&mut self) {
        if self.progress.get_mut().enabled {
            let _ = self.progress.get_mut().finish();
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

fn is_login_uri(uri: &http::Uri) -> bool {
    match uri.to_string().parse::<Url>() {
        Ok(url) => {
            url.host_str() == Some("login.1c.ru")
                && (url.path() == "/login" || url.path().starts_with("/login/"))
        }
        Err(_) => false,
    }
}

fn build_login_form_request(
    html: &str,
    base_url: &str,
    username: &str,
    password: &str,
) -> Result<(String, String)> {
    let document = Html::parse_document(html);
    let form_selector = Selector::parse("form").unwrap();
    let input_selector = Selector::parse("input").unwrap();

    let form = document
        .select(&form_selector)
        .next()
        .ok_or_else(|| anyhow!("authentication form not found"))?;

    let action = form.value().attr("action").unwrap_or("");
    let form_url = Url::parse(base_url)
        .context("invalid login page URL")?
        .join(action)
        .with_context(|| format!("invalid login form action {action}"))?;

    let mut serializer = url::form_urlencoded::Serializer::new(String::new());

    for input in form.select(&input_selector) {
        let Some(name) = input.value().attr("name") else {
            continue;
        };

        let input_type = input.value().attr("type").unwrap_or("");
        let is_checkbox = input_type.eq_ignore_ascii_case("checkbox");
        if is_checkbox && input.value().attr("checked").is_none() {
            continue;
        }

        let value = match name {
            "username" => username,
            "password" => password,
            _ if is_checkbox => input.value().attr("value").unwrap_or("on"),
            _ => input.value().attr("value").unwrap_or(""),
        };
        serializer.append_pair(name, value);
    }

    Ok((form_url.to_string(), serializer.finish()))
}

fn extract_login_error(html: &str) -> Option<String> {
    let document = Html::parse_document(html);
    let selectors = [
        "#emptyUsernameOrPasswordMessage",
        ".text-error",
        ".centerMessage",
        ".alert",
        ".alert-danger",
    ];

    for selector in selectors {
        let selector = Selector::parse(selector).unwrap();
        for element in document.select(&selector) {
            let text = element
                .text()
                .collect::<Vec<_>>()
                .join(" ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            if !text.is_empty() {
                return Some(text);
            }
        }
    }

    None
}

fn redact_form_body(body: &str) -> String {
    body.split('&')
        .map(|part| {
            let Some((key, value)) = part.split_once('=') else {
                return part.to_owned();
            };

            if matches!(key, "password" | "username") {
                format!("{key}={}", redact_secret(value))
            } else {
                format!("{key}={value}")
            }
        })
        .collect::<Vec<_>>()
        .join("&")
}

fn redact_secret(value: &str) -> String {
    if value.is_empty() {
        return "<empty>".to_owned();
    }

    "<redacted>".to_owned()
}

fn truncate_for_log(value: &str, limit: usize) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.len() <= limit {
        return normalized;
    }

    format!("{}...", &normalized[..limit])
}

fn should_report_progress(downloaded: u64, last_reported: u64, total: Option<u64>) -> bool {
    match total {
        Some(total) if total > 0 => {
            let current_percent = (downloaded.saturating_mul(100) / total).min(100);
            let previous_percent = (last_reported.saturating_mul(100) / total).min(100);
            current_percent >= previous_percent + 10
        }
        _ => downloaded.saturating_sub(last_reported) >= 10 * 1024 * 1024,
    }
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;

    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn format_stage(message: &str) -> String {
    format!("==> {message}")
}

fn format_progress(message: &str) -> String {
    format!(" -> {message}")
}

fn progress_bar(downloaded: u64, total: u64, width: usize) -> String {
    if total == 0 || width == 0 {
        return "[]".to_owned();
    }

    let filled = ((downloaded.saturating_mul(width as u64)) / total).min(width as u64) as usize;
    let empty = width.saturating_sub(filled);
    format!("[{}{}]", "#".repeat(filled), ".".repeat(empty))
}

fn indeterminate_bar(downloaded: u64) -> String {
    let phase = ((downloaded / (5 * 1024 * 1024)) % 4) as usize;
    let variants = ["[#...]", "[##..]", "[###.]", "[####]"];
    variants[phase].to_owned()
}

fn short_path(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_else(|| path.as_os_str().to_str().unwrap_or("<file>"))
        .to_owned()
}

fn progress_status(downloaded: u64, total: Option<u64>) -> String {
    match total {
        Some(total) if total > 0 => format!("{} / {}", human_size(downloaded), human_size(total)),
        _ => human_size(downloaded),
    }
}

fn render_progress_line(downloaded: u64, total: Option<u64>) -> String {
    match total {
        Some(total) if total > 0 => format!(
            "Downloading {} {:>3}% {}",
            docker_bar(downloaded, total, 20),
            ((downloaded.saturating_mul(100)) / total).min(100),
            progress_status(downloaded, Some(total))
        ),
        _ => format!(
            "Downloading {} {}",
            indeterminate_bar(downloaded),
            human_size(downloaded)
        ),
    }
}

fn docker_bar(downloaded: u64, total: u64, width: usize) -> String {
    if total == 0 || width == 0 {
        return "[>                   ]".to_owned();
    }

    let filled = ((downloaded.saturating_mul(width as u64)) / total).min(width as u64) as usize;
    let mut chars = vec![' '; width];
    for ch in chars.iter_mut().take(filled.saturating_sub(1)) {
        *ch = '=';
    }
    if filled > 0 && filled < width {
        chars[filled - 1] = '>';
    } else if filled >= width {
        chars[width - 1] = '=';
    }
    format!("[{}]", chars.into_iter().collect::<String>())
}

struct ProgressDisplay {
    enabled: bool,
    auth_done: bool,
    selected_version: Option<String>,
    files: Vec<FileDisplay>,
    rendered_lines: usize,
    width: usize,
}

struct FileDisplay {
    name: String,
    state: FileState,
    detail: String,
}

enum FileState {
    Pending,
    Active,
    Done,
    Failed,
}

impl ProgressDisplay {
    fn new() -> Self {
        Self {
            enabled: false,
            auth_done: false,
            selected_version: None,
            files: Vec::new(),
            rendered_lines: 0,
            width: progress_width(),
        }
    }

    fn set_auth_done(&mut self) {
        self.auth_done = true;
    }

    fn set_selected_version(&mut self, version: String) {
        self.selected_version = Some(version);
    }

    fn set_files(&mut self, files: Vec<String>) {
        self.files = files
            .into_iter()
            .map(|name| FileDisplay {
                name,
                state: FileState::Pending,
                detail: "Waiting".to_owned(),
            })
            .collect();
    }

    fn mark_active(&mut self, name: &str, detail: String) {
        if let Some(file) = self.files.iter_mut().find(|file| file.name == name) {
            file.state = FileState::Active;
            file.detail = detail;
        }
    }

    fn update_progress(&mut self, name: &str, detail: String) {
        self.mark_active(name, detail);
    }

    fn mark_done(&mut self, name: &str, detail: String) {
        if let Some(file) = self.files.iter_mut().find(|file| file.name == name) {
            file.state = FileState::Done;
            file.detail = detail;
        }
    }

    fn mark_failed(&mut self, name: &str, detail: String) {
        if let Some(file) = self.files.iter_mut().find(|file| file.name == name) {
            file.state = FileState::Failed;
            file.detail = detail;
        }
    }

    fn render(&mut self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let lines = self.lines();
        let mut stderr = std::io::stderr().lock();

        if self.rendered_lines > 0 {
            write!(stderr, "\x1b[{}F", self.rendered_lines)?;
        }

        let max_lines = self.rendered_lines.max(lines.len());
        for index in 0..max_lines {
            write!(stderr, "\x1b[2K\r")?;
            if let Some(line) = lines.get(index) {
                writeln!(stderr, "{line}")?;
            } else {
                writeln!(stderr)?;
            }
        }
        stderr.flush()?;
        self.rendered_lines = lines.len();
        Ok(())
    }

    fn finish(&mut self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let mut stderr = std::io::stderr().lock();
        writeln!(stderr)?;
        stderr.flush()?;
        self.enabled = false;
        self.rendered_lines = 0;
        Ok(())
    }

    fn lines(&self) -> Vec<String> {
        let mut lines = Vec::with_capacity(self.files.len() + 2);
        lines.push(truncate_line(
            &format!(
                "{} authentication {}",
                green("✓"),
                if self.auth_done {
                    "completed"
                } else {
                    "pending"
                }
            ),
            self.width,
        ));
        if let Some(selected_version) = &self.selected_version {
            lines.push(truncate_line(
                &format!("{} {}", cyan("→"), selected_version),
                self.width,
            ));
        }
        lines.extend(
            self.files
                .iter()
                .map(|file| truncate_line(&render_file_line(file), self.width)),
        );
        lines
    }
}

fn render_file_line(file: &FileDisplay) -> String {
    let icon = match file.state {
        FileState::Pending => "○".to_owned(),
        FileState::Active => cyan("●"),
        FileState::Done => green("✓"),
        FileState::Failed => red("✗"),
    };
    format!("{icon} {}  {}", file.name, file.detail)
}

fn green(value: &str) -> String {
    format!("\x1b[32m{value}\x1b[0m")
}

fn cyan(value: &str) -> String {
    format!("\x1b[36m{value}\x1b[0m")
}

fn red(value: &str) -> String {
    format!("\x1b[31m{value}\x1b[0m")
}

fn progress_width() -> usize {
    if let Some((Width(width), _)) = terminal_size() {
        return usize::from(width).max(60);
    }

    std::env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value >= 60)
        .unwrap_or(80)
}

fn truncate_line(line: &str, width: usize) -> String {
    let plain = strip_ansi(line);
    if plain.chars().count() <= width {
        return line.to_owned();
    }

    plain
        .chars()
        .take(width.saturating_sub(1))
        .collect::<String>()
        + "…"
}

fn strip_ansi(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            let _ = chars.next();
            for next in chars.by_ref() {
                if ('@'..='~').contains(&next) {
                    break;
                }
            }
            continue;
        }

        out.push(ch);
    }

    out
}

fn scope_for_message(message: &str) -> &'static str {
    if message.contains("login")
        || message.contains("authorization")
        || message.contains("auth")
        || message.contains("session")
    {
        return "auth";
    }

    if message.starts_with("GET ")
        || message.starts_with("POST ")
        || message.contains("redirect")
        || message.contains("response")
        || message.contains("headers:")
    {
        return "http";
    }

    if message.contains("version")
        || message.contains("project page")
        || message.contains("release files")
        || message.contains("parsed ")
    {
        return "catalog";
    }

    if message.contains("download")
        || message.contains("output=")
        || message.contains("writing file")
        || message.contains("skipping existing file")
    {
        return "download";
    }

    "run"
}

fn resolve_requested_version(versions: &[Version], requested: &str) -> Option<Version> {
    if requested.eq_ignore_ascii_case("latest") {
        return versions
            .iter()
            .filter_map(|item| Some((parse_version_parts(&item.name)?, item)))
            .max_by(|(left_parts, left_item), (right_parts, right_item)| {
                left_parts
                    .cmp(right_parts)
                    .then_with(|| left_item.name.cmp(&right_item.name))
            })
            .map(|(_, item)| item.clone());
    }

    if let Some(exact) = versions.iter().find(|item| item.name == requested) {
        return Some(exact.clone());
    }

    let requested_parts = parse_version_parts(requested)?;
    if !(2..=3).contains(&requested_parts.len()) {
        return None;
    }

    versions
        .iter()
        .filter_map(|item| {
            let parts = parse_version_parts(&item.name)?;
            if parts.starts_with(&requested_parts) {
                Some((parts, item))
            } else {
                None
            }
        })
        .max_by(|(left_parts, left_item), (right_parts, right_item)| {
            left_parts
                .cmp(right_parts)
                .then_with(|| left_item.name.cmp(&right_item.name))
        })
        .map(|(_, item)| item.clone())
}

fn parse_version_parts(version: &str) -> Option<Vec<u32>> {
    let parts = version
        .split('.')
        .map(|part| part.parse::<u32>().ok())
        .collect::<Option<Vec<_>>>()?;

    if parts.is_empty() {
        return None;
    }

    Some(parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ArchitectureName, ArtifactFilter, DistributiveType, OsName};
    use ureq::http::HeaderValue;

    fn is_legacy_linux_platform_version(version: &str) -> bool {
        let mut parts = version.split('.').filter_map(|part| part.parse::<u32>().ok());
        matches!(
            (parts.next(), parts.next(), parts.next()),
            (Some(8), Some(3), Some(patch)) if patch < 20
        )
    }

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

    #[test]
    fn detects_login_uri() {
        let uri = "https://login.1c.ru/login?service=https%3A%2F%2Freleases.1c.ru"
            .parse()
            .unwrap();
        assert!(is_login_uri(&uri));
    }

    #[test]
    fn ignores_non_login_uri() {
        let uri = "https://releases.1c.ru/project/Platform83?allUpdates=true"
            .parse()
            .unwrap();
        assert!(!is_login_uri(&uri));
    }

    #[test]
    fn builds_login_form_request() {
        let html = r#"
            <form method="post" action="/login">
                <input type="hidden" name="execution" value="abc"/>
                <input type="text" name="username" value=""/>
                <input type="password" name="password" value=""/>
                <input type="hidden" name="_eventId" value="submit"/>
            </form>
        "#;

        let (url, body) =
            build_login_form_request(html, "https://login.1c.ru/login?service=x", "user", "pass")
                .unwrap();

        assert_eq!(url, "https://login.1c.ru/login");
        assert!(body.contains("username=user"));
        assert!(body.contains("password=pass"));
        assert!(body.contains("execution=abc"));
        assert!(body.contains("_eventId=submit"));
    }

    #[test]
    fn extracts_login_error_message() {
        let html = r#"
            <div id="emptyUsernameOrPasswordMessage" class="text-error centerMessage">
                Неверный логин или пароль
            </div>
        "#;

        assert_eq!(
            extract_login_error(html).as_deref(),
            Some("Неверный логин или пароль")
        );
    }

    #[test]
    fn redacts_form_body_credentials() {
        let body = "username=user&password=secret&execution=abc";
        assert_eq!(
            redact_form_body(body),
            "username=<redacted>&password=<redacted>&execution=abc"
        );
    }

    #[test]
    fn resolves_exact_requested_version() {
        let versions = vec![
            Version {
                name: "8.3.27.2074".into(),
                url: "/a".into(),
                files: Vec::new(),
            },
            Version {
                name: "8.3.27.2100".into(),
                url: "/b".into(),
                files: Vec::new(),
            },
        ];

        let resolved = resolve_requested_version(&versions, "8.3.27.2074").unwrap();
        assert_eq!(resolved.name, "8.3.27.2074");
    }

    #[test]
    fn resolves_latest_patch_for_partial_version() {
        let versions = vec![
            Version {
                name: "8.3.27.2074".into(),
                url: "/a".into(),
                files: Vec::new(),
            },
            Version {
                name: "8.3.27.2100".into(),
                url: "/b".into(),
                files: Vec::new(),
            },
            Version {
                name: "8.3.25.1633".into(),
                url: "/c".into(),
                files: Vec::new(),
            },
        ];

        let resolved = resolve_requested_version(&versions, "8.3.27").unwrap();
        assert_eq!(resolved.name, "8.3.27.2100");
    }

    #[test]
    fn does_not_resolve_invalid_partial_version() {
        let versions = vec![Version {
            name: "8.3.27.2100".into(),
            url: "/a".into(),
            files: Vec::new(),
        }];

        assert!(resolve_requested_version(&versions, "8").is_none());
        assert!(resolve_requested_version(&versions, "8.3.27.2100.1").is_none());
    }

    #[test]
    fn resolves_latest_keyword_to_latest_version() {
        let versions = vec![
            Version {
                name: "2024.1.1".into(),
                url: "/a".into(),
                files: Vec::new(),
            },
            Version {
                name: "2025.2.3".into(),
                url: "/b".into(),
                files: Vec::new(),
            },
        ];

        let resolved = resolve_requested_version(&versions, "latest").unwrap();
        assert_eq!(resolved.name, "2025.2.3");
    }

    #[test]
    #[ignore = "live network test; requires ONEC_USERNAME and ONEC_PASSWORD"]
    fn live_platform83_full_filter_returns_single_match_for_sample_versions() {
        let username = std::env::var("ONEC_USERNAME")
            .expect("ONEC_USERNAME must be set for live tests");
        let password = std::env::var("ONEC_PASSWORD")
            .expect("ONEC_PASSWORD must be set for live tests");

        let client = OnecClient::new(username, password)
            .unwrap()
            .with_quiet(true);

        client.auth().unwrap();

        let page = client.project_page("Platform83").unwrap();
        let versions = parse::versions(&page);
        assert!(
            versions.len() >= 5,
            "expected Platform83 project page to contain at least 5 versions"
        );

        let sample_indexes = [
            0,
            versions.len() / 4,
            versions.len() / 2,
            (versions.len() * 3) / 4,
            versions.len() - 1,
        ];
        let mut sampled_versions = Vec::new();
        for index in sample_indexes {
            let version = versions[index].name.clone();
            if !sampled_versions.contains(&version) {
                sampled_versions.push(version);
            }
        }

        let mut failed_versions = Vec::new();

        for version in &sampled_versions {
            let version_info = client.version_info("Platform83", version).unwrap();

            let linux_filter = if is_legacy_linux_platform_version(version) {
                ArtifactFilter {
                    os_name: Some(OsName::Deb),
                    architecture: Some(ArchitectureName::X64),
                    package_type: Some(DistributiveType::ClientOrServer),
                    offline: false,
                }
            } else {
                ArtifactFilter {
                    os_name: Some(OsName::Linux),
                    architecture: Some(ArchitectureName::X64),
                    package_type: Some(DistributiveType::Full),
                    offline: false,
                }
            };

            let expected_linux_matches = if is_legacy_linux_platform_version(version) {
                2
            } else {
                1
            };

            match client.matching_files_for_version(&version_info, &linux_filter) {
                Ok(files) if files.len() == expected_linux_matches => {}
                Ok(files) => failed_versions.push(format!(
                    "Linux {version}: expected exactly {expected_linux_matches} file(s), got {} ({})",
                    files.len(),
                    files
                        .iter()
                        .map(|file| file.name.as_str())
                        .collect::<Vec<_>>()
                        .join("; ")
                )),
                Err(error) => failed_versions.push(format!("Linux {version}: {error:#}")),
            }

            let windows_x64_filter = ArtifactFilter {
                os_name: Some(OsName::Win),
                architecture: Some(ArchitectureName::X64),
                package_type: Some(DistributiveType::Full),
                offline: false,
            };

            match client.matching_files_for_version(&version_info, &windows_x64_filter) {
                Ok(files) if files.len() == 1 => {}
                Ok(files) => failed_versions.push(format!(
                    "Win x64 {version}: expected exactly 1 file, got {} ({})",
                    files.len(),
                    files
                        .iter()
                        .map(|file| file.name.as_str())
                        .collect::<Vec<_>>()
                        .join("; ")
                )),
                Err(_) => {
                    let windows_x86_filter = ArtifactFilter {
                        os_name: Some(OsName::Win),
                        architecture: Some(ArchitectureName::X86),
                        package_type: Some(DistributiveType::Full),
                        offline: false,
                    };
                    match client.matching_files_for_version(&version_info, &windows_x86_filter) {
                        Ok(files) if files.len() == 1 => {}
                        Ok(files) => failed_versions.push(format!(
                            "Win x86 {version}: expected exactly 1 file, got {} ({})",
                            files.len(),
                            files
                                .iter()
                                .map(|file| file.name.as_str())
                                .collect::<Vec<_>>()
                                .join("; ")
                        )),
                        Err(error) => {
                            failed_versions.push(format!("Win {version}: {error:#}"));
                        }
                    }
                }
            }
        }

        assert!(
            failed_versions.is_empty(),
            "full filter did not return a single file for {} sampled version(s): {}",
            failed_versions.len(),
            failed_versions
                .iter()
                .take(5)
                .cloned()
                .collect::<Vec<_>>()
                .join(" | ")
        );
    }
}
