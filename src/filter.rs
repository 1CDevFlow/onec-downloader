use regex::Regex;

use crate::model::{ArchitectureName, ArtifactFilter, DistributiveType, OsName, ReleaseFile};

fn x64_pattern() -> Regex {
    Regex::new(r".*(\(64-bit\)|\(64 бит\)).*").unwrap()
}

fn rpm_pattern() -> Regex {
    Regex::new(r".+RPM.+(ОС Linux|для Linux|Linux-систем).*").unwrap()
}

fn deb_pattern() -> Regex {
    Regex::new(r".+DEB.+(ОС Linux|для Linux|Linux-систем).*").unwrap()
}

fn linux_pattern() -> Regex {
    Regex::new(r".*(ОС Linux|для Linux|Linux-систем).*").unwrap()
}

fn windows_pattern() -> Regex {
    Regex::new(r".*(ОС Windows|для Windows).*").unwrap()
}

fn osx_pattern() -> Regex {
    Regex::new(r".+(OS X|для macOS).*").unwrap()
}

fn client_pattern() -> Regex {
    Regex::new(r"^Клиент.+").unwrap()
}

fn server_pattern() -> Regex {
    Regex::new(r"^[CС]ервер.+").unwrap()
}

fn thin_pattern() -> Regex {
    Regex::new(r"^Тонкий клиент.+").unwrap()
}

fn full_pattern() -> Regex {
    Regex::new(r"^Технологическая платформа.+").unwrap()
}

fn offline_pattern() -> Regex {
    Regex::new(r".+(без интернета|оффлайн).*").unwrap()
}

fn client_or_server_pattern() -> Regex {
    Regex::new(r"^(Клиент|Cервер|Сервер).+").unwrap()
}

fn sha_pattern() -> Regex {
    Regex::new(r".*(Контрольная сумма|sha).*").unwrap()
}

pub fn filter_files(files: &[ReleaseFile], artifact_filter: &ArtifactFilter) -> Vec<ReleaseFile> {
    files
        .iter()
        .filter(|file| matches_all(&file.name, artifact_filter))
        .cloned()
        .collect()
}

fn matches_all(name: &str, artifact_filter: &ArtifactFilter) -> bool {
    if sha_pattern().is_match(name) {
        return false;
    }

    if let Some(os_name) = artifact_filter.os_name {
        let matched = match os_name {
            OsName::Win => windows_pattern().is_match(name),
            OsName::Mac => osx_pattern().is_match(name),
            OsName::Linux => linux_pattern().is_match(name),
            OsName::Deb => deb_pattern().is_match(name),
            OsName::Rpm => rpm_pattern().is_match(name),
        };
        if !matched {
            return false;
        }
    }

    if let Some(architecture) = artifact_filter.architecture {
        let is_x64 = x64_pattern().is_match(name);
        match architecture {
            ArchitectureName::X86 if is_x64 => return false,
            ArchitectureName::X64 if !is_x64 => return false,
            _ => {}
        }
    }

    if let Some(package_type) = artifact_filter.package_type {
        let matched = match package_type {
            DistributiveType::Full => full_pattern().is_match(name),
            DistributiveType::ThinClient => thin_pattern().is_match(name),
            DistributiveType::Server => server_pattern().is_match(name),
            DistributiveType::Client => client_pattern().is_match(name),
            DistributiveType::ClientOrServer => client_or_server_pattern().is_match(name),
        };
        if !matched {
            return false;
        }
    }

    if artifact_filter.offline {
        offline_pattern().is_match(name)
    } else {
        !offline_pattern().is_match(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ArchitectureName, DistributiveType, OsName};

    #[test]
    fn filters_windows_full_x64() {
        let files = vec![
            ReleaseFile {
                name: "Технологическая платформа 8.3.10.2580 (64-bit) для Windows".into(),
                url: "/good".into(),
            },
            ReleaseFile {
                name: "Технологическая платформа 8.3.10.2580 для Windows".into(),
                url: "/bad-x86".into(),
            },
            ReleaseFile {
                name: "Контрольная сумма для Windows".into(),
                url: "/bad-sha".into(),
            },
        ];

        let result = filter_files(
            &files,
            &ArtifactFilter {
                os_name: Some(OsName::Win),
                architecture: Some(ArchitectureName::X64),
                package_type: Some(DistributiveType::Full),
                offline: false,
            },
        );

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].url, "/good");
    }

    #[test]
    fn filters_offline_linux_release() {
        let files = vec![
            ReleaseFile {
                name: "Дистрибутив EDT для Linux-систем без интернета".into(),
                url: "/offline".into(),
            },
            ReleaseFile {
                name: "Дистрибутив EDT для Linux-систем".into(),
                url: "/online".into(),
            },
        ];

        let result = filter_files(
            &files,
            &ArtifactFilter {
                os_name: Some(OsName::Linux),
                architecture: None,
                package_type: None,
                offline: true,
            },
        );

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].url, "/offline");
    }
}
