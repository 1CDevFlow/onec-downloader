use regex::Regex;

use crate::model::{ArchitectureName, ArtifactFilter, DistributiveType, OsName, ReleaseFile};

fn x64_pattern() -> Regex {
    Regex::new(r".*(\(64-bit\)|\(64 бит\)|\b64\s*Bit\b).*").unwrap()
}

fn x86_pattern() -> Regex {
    Regex::new(r".*(\(32-bit\)|\(32 бит\)|\b32\s*Bit\b).*").unwrap()
}

fn rpm_pattern() -> Regex {
    Regex::new(r".+RPM.+(ОС Linux|для Linux($|\s+(без интернета|оффлайн))|Linux-систем($|\s+(без интернета|оффлайн))).*").unwrap()
}

fn deb_pattern() -> Regex {
    Regex::new(r".+DEB.+(ОС Linux|для Linux($|\s+(без интернета|оффлайн))|Linux-систем($|\s+(без интернета|оффлайн))).*").unwrap()
}

fn linux_pattern() -> Regex {
    Regex::new(r".*(ОС Linux|для Linux($|\s+(без интернета|оффлайн)|\s+\d+\s*Bit|\s+\(\d+-bit\))|Linux-систем($|\s+(без интернета|оффлайн))).*").unwrap()
}

fn windows_pattern() -> Regex {
    Regex::new(r".*(ОС Windows|для Windows$|для Windows\s\+).*").unwrap()
}

fn osx_pattern() -> Regex {
    Regex::new(r".+(OS X|для macOS)$").unwrap()
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

fn combined_client_package_pattern() -> Regex {
    Regex::new(r".+\+\s*Тонкий клиент.+").unwrap()
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
            OsName::Linux => {
                linux_pattern().is_match(name)
                    && !deb_pattern().is_match(name)
                    && !rpm_pattern().is_match(name)
            }
            OsName::Deb => deb_pattern().is_match(name),
            OsName::Rpm => rpm_pattern().is_match(name),
        };
        if !matched {
            return false;
        }
    }

    if let Some(architecture) = artifact_filter.architecture {
        let is_x64 = x64_pattern().is_match(name);
        let is_x86 = x86_pattern().is_match(name);
        match architecture {
            ArchitectureName::X86 if is_x64 => return false,
            ArchitectureName::X64 if is_x86 => return false,
            ArchitectureName::X64 if !is_x64 && artifact_filter.package_type.is_some() => {
                return false;
            }
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

        if package_type == DistributiveType::Full
            && artifact_filter.os_name == Some(OsName::Win)
            && combined_client_package_pattern().is_match(name)
        {
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
    fn filters_windows_x86_and_x64_differently() {
        let files = vec![
            ReleaseFile {
                name: "Технологическая платформа 1С:Предприятия (64-bit) для Windows".into(),
                url: "/x64".into(),
            },
            ReleaseFile {
                name: "Технологическая платформа 1С:Предприятия для Windows".into(),
                url: "/x86".into(),
            },
        ];

        let x86_result = filter_files(
            &files,
            &ArtifactFilter {
                os_name: Some(OsName::Win),
                architecture: Some(ArchitectureName::X86),
                package_type: Some(DistributiveType::Full),
                offline: false,
            },
        );
        let x64_result = filter_files(
            &files,
            &ArtifactFilter {
                os_name: Some(OsName::Win),
                architecture: Some(ArchitectureName::X64),
                package_type: Some(DistributiveType::Full),
                offline: false,
            },
        );

        assert_eq!(x86_result.len(), 1);
        assert_eq!(x86_result[0].url, "/x86");
        assert_eq!(x64_result.len(), 1);
        assert_eq!(x64_result[0].url, "/x64");
    }

    #[test]
    fn filters_x64_files_with_bit_suffix() {
        let files = vec![
            ReleaseFile {
                name: "1C:Enterprise Development Tools для Linux 64 Bit".into(),
                url: "/x64".into(),
            },
            ReleaseFile {
                name: "1C:Enterprise Development Tools для Linux 32 Bit".into(),
                url: "/x86".into(),
            },
        ];

        let result = filter_files(
            &files,
            &ArtifactFilter {
                os_name: Some(OsName::Linux),
                architecture: Some(ArchitectureName::X64),
                package_type: None,
                offline: false,
            },
        );

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].url, "/x64");
    }

    #[test]
    fn allows_generic_x64_when_package_type_is_unspecified() {
        let files = vec![
            ReleaseFile {
                name: "Дистрибутив 1C:EDT для ОС Linux".into(),
                url: "/generic-linux".into(),
            },
            ReleaseFile {
                name: "Дистрибутив 1C:EDT для ОС Windows 32 Bit".into(),
                url: "/windows-x86".into(),
            },
        ];

        let result = filter_files(
            &files,
            &ArtifactFilter {
                os_name: Some(OsName::Linux),
                architecture: Some(ArchitectureName::X64),
                package_type: None,
                offline: false,
            },
        );

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].url, "/generic-linux");
    }

    #[test]
    fn filters_windows_full_without_combined_client_packages() {
        let files = vec![
            ReleaseFile {
                name: "Технологическая платформа 1С:Предприятия (64-bit) для Windows + Тонкий клиент для Windows, Linux и MacOS для автоматического обновления клиентов через веб-сервер".into(),
                url: "/with-all-clients".into(),
            },
            ReleaseFile {
                name: "Технологическая платформа 1С:Предприятия (64-bit) для Windows + Тонкий клиент для Windows и MacOS для автоматического обновления клиентов через веб-сервер".into(),
                url: "/with-clients".into(),
            },
            ReleaseFile {
                name: "Технологическая платформа 1С:Предприятия (64-bit) для Windows".into(),
                url: "/windows-only".into(),
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
        assert_eq!(result[0].url, "/windows-only");
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

    #[test]
    fn filters_linux_full_without_combined_client_packages() {
        let files = vec![
            ReleaseFile {
                name: "Технологическая платформа 1С:Предприятия (64-bit) для Linux + Тонкий клиент для Windows, Linux и MacOS для автоматического обновления клиентов через веб-сервер".into(),
                url: "/with-all-clients".into(),
            },
            ReleaseFile {
                name: "Технологическая платформа 1С:Предприятия (64-bit) для Linux + Тонкий клиент для Windows и MacOS для автоматического обновления клиентов через веб-сервер".into(),
                url: "/with-clients".into(),
            },
            ReleaseFile {
                name: "Технологическая платформа 1С:Предприятия (64-bit) для Linux".into(),
                url: "/server-only".into(),
            },
        ];

        let result = filter_files(
            &files,
            &ArtifactFilter {
                os_name: Some(OsName::Linux),
                architecture: Some(ArchitectureName::X64),
                package_type: Some(DistributiveType::Full),
                offline: false,
            },
        );

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].url, "/server-only");
    }

    #[test]
    fn filters_legacy_linux_client_or_server_as_two_packages() {
        let files = vec![
            ReleaseFile {
                name: "Клиент 1С:Предприятия (64-bit) для DEB-based Linux-систем".into(),
                url: "/client".into(),
            },
            ReleaseFile {
                name: "Сервер 1С:Предприятия (64-bit) для DEB-based Linux-систем".into(),
                url: "/server".into(),
            },
            ReleaseFile {
                name: "Тонкий клиент 1С:Предприятия (64-bit) для DEB-based Linux-систем".into(),
                url: "/thin".into(),
            },
        ];

        let result = filter_files(
            &files,
            &ArtifactFilter {
                os_name: Some(OsName::Deb),
                architecture: Some(ArchitectureName::X64),
                package_type: Some(DistributiveType::ClientOrServer),
                offline: false,
            },
        );

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].url, "/client");
        assert_eq!(result[1].url, "/server");
    }

    #[test]
    fn filters_generic_linux_thin_client_without_deb_or_rpm_variants() {
        let files = vec![
            ReleaseFile {
                name: "Тонкий клиент 1С:Предприятия (64-bit) для DEB-based Linux-систем".into(),
                url: "/deb".into(),
            },
            ReleaseFile {
                name: "Тонкий клиент 1С:Предприятия (64-bit) для Linux".into(),
                url: "/generic".into(),
            },
            ReleaseFile {
                name: "Тонкий клиент 1С:Предприятия (64-bit) для RPM-based Linux-систем".into(),
                url: "/rpm".into(),
            },
        ];

        let result = filter_files(
            &files,
            &ArtifactFilter {
                os_name: Some(OsName::Linux),
                architecture: Some(ArchitectureName::X64),
                package_type: Some(DistributiveType::ThinClient),
                offline: false,
            },
        );

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].url, "/generic");
    }

    #[test]
    fn filters_generic_linux_files_with_bit_suffix() {
        let files = vec![
            ReleaseFile {
                name: "1C:Enterprise Development Tools для Linux 64 Bit".into(),
                url: "/linux".into(),
            },
            ReleaseFile {
                name: "1C:Enterprise Development Tools для Windows 64 Bit".into(),
                url: "/windows".into(),
            },
        ];

        let result = filter_files(
            &files,
            &ArtifactFilter {
                os_name: Some(OsName::Linux),
                architecture: None,
                package_type: None,
                offline: false,
            },
        );

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].url, "/linux");
    }
}
