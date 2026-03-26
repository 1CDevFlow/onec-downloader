use std::collections::HashSet;

use regex::Regex;
use scraper::{ElementRef, Html, Selector};

use crate::model::{ReleaseFile, Version};

fn select_attr(content: &str, selector: &str) -> Vec<(String, String)> {
    let document = Html::parse_document(content);
    let selector = Selector::parse(selector).unwrap();

    document
        .select(&selector)
        .filter_map(|element| {
            let href = element.value().attr("href")?;
            let text = element.text().collect::<String>().trim().to_owned();
            Some((text, href.to_owned()))
        })
        .collect()
}

pub fn versions(content: &str) -> Vec<Version> {
    let document = Html::parse_document(content);
    let selectors = ["td.versionColumn > a", ".versionColumn a", "a[href]"];

    let mut seen = HashSet::new();
    let mut versions = Vec::new();

    for selector in selectors {
        let selector = Selector::parse(selector).unwrap();
        for element in document.select(&selector) {
            if let Some(version) = version_from_link(&element) {
                let key = (version.name.clone(), version.url.clone());
                if seen.insert(key) {
                    versions.push(version);
                }
            }
        }
    }

    versions
}

pub fn release_files(content: &str) -> Vec<ReleaseFile> {
    select_attr(content, ".files-container .formLine a")
        .into_iter()
        .map(|(name, url)| ReleaseFile { name, url })
        .collect()
}

pub fn file_download_links(content: &str) -> Vec<String> {
    let document = Html::parse_document(content);
    let selector = Selector::parse(".downloadDist a").unwrap();

    document
        .select(&selector)
        .filter_map(|element| element.value().attr("href").map(ToOwned::to_owned))
        .collect()
}

fn version_from_link(element: &ElementRef<'_>) -> Option<Version> {
    let href = element.value().attr("href")?;
    let text = element
        .text()
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    let name = extract_version(&text).or_else(|| extract_version(href))?;
    Some(Version {
        name,
        url: href.to_owned(),
        files: Vec::new(),
    })
}

fn extract_version(value: &str) -> Option<String> {
    let regex = Regex::new(r"\b\d+\.\d+(?:\.\d+(?:\.\d+)?)?\b").unwrap();
    regex.find(value).map(|m| m.as_str().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_versions() {
        let html = r#"
            <table>
                <tr><td class="versionColumn"><a href="/v1">8.3.10.2580</a></td></tr>
                <tr><td class="versionColumn"><a href="/v2">8.3.25.1286</a></td></tr>
            </table>
        "#;

        let versions = versions(html);
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].name, "8.3.10.2580");
        assert_eq!(versions[1].url, "/v2");
    }

    #[test]
    fn parses_versions_with_nested_markup_and_suffix() {
        let html = r#"
            <table>
                <tr>
                    <td class="versionColumn">
                        <a href="/v1"><span>8.3.27.2074</span><span> Рекомендуемая</span></a>
                    </td>
                </tr>
            </table>
        "#;

        let versions = versions(html);
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].name, "8.3.27.2074");
        assert_eq!(versions[0].url, "/v1");
    }

    #[test]
    fn parses_versions_from_fallback_links() {
        let html = r#"
            <div>
                <a href="/project/Platform83/version/8.3.27.2074">Скачать 8.3.27.2074</a>
            </div>
        "#;

        let versions = versions(html);
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].name, "8.3.27.2074");
    }

    #[test]
    fn parses_two_part_versions() {
        let html = r#"
            <table>
                <tr><td class="versionColumn"><a href="/v1">2020.1</a></td></tr>
            </table>
        "#;

        let versions = versions(html);
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].name, "2020.1");
        assert_eq!(versions[0].url, "/v1");
    }

    #[test]
    fn prefers_full_version_over_prefix_match() {
        let html = r#"
            <table>
                <tr><td class="versionColumn"><a href="/v1">8.3.27.2074</a></td></tr>
            </table>
        "#;

        let versions = versions(html);
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].name, "8.3.27.2074");
    }
}
