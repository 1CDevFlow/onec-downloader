use crate::model::{ArchitectureName, ArtifactFilter, DistributiveType, OsName, ReleaseFile};

fn is_x64_name(name: &str) -> bool {
    name.contains("(64-bit)") || name.contains("(64 бит)") || contains_bit_marker(name, "64")
}

fn is_x86_name(name: &str) -> bool {
    name.contains("(32-bit)") || name.contains("(32 бит)") || contains_bit_marker(name, "32")
}

fn contains_bit_marker(name: &str, bits: &str) -> bool {
    let mut rest = name;

    while let Some(offset) = rest.find(bits) {
        let absolute = name.len() - rest.len() + offset;
        let after_bits = absolute + bits.len();

        if is_word_boundary(name, absolute) {
            let tail = &name[after_bits..];
            let trimmed = tail.trim_start_matches(char::is_whitespace);
            if let Some(after_bit) = trimmed.strip_prefix("Bit") {
                let bit_end = name.len() - after_bit.len();
                if is_word_boundary(name, bit_end) {
                    return true;
                }
            }
        }

        rest = &name[after_bits..];
    }

    false
}

fn is_word_boundary(value: &str, byte_index: usize) -> bool {
    let previous = value[..byte_index].chars().next_back();
    let next = value[byte_index..].chars().next();
    previous.map(is_word_char).unwrap_or(false) != next.map(is_word_char).unwrap_or(false)
}

fn is_word_char(ch: char) -> bool {
    ch == '_' || ch.is_alphanumeric()
}

fn is_rpm_name(name: &str) -> bool {
    name.contains("RPM") && is_linux_package_target(name)
}

fn is_deb_name(name: &str) -> bool {
    name.contains("DEB") && is_linux_package_target(name)
}

fn is_linux_name(name: &str) -> bool {
    name.contains("ОС Linux") || has_linux_for_suffix(name) || has_linux_systems_suffix(name)
}

fn is_linux_package_target(name: &str) -> bool {
    name.contains("ОС Linux")
        || has_linux_for_offline_suffix(name)
        || has_linux_systems_suffix(name)
}

fn has_linux_for_suffix(name: &str) -> bool {
    let Some(tail) = name.split_once("для Linux").map(|(_, tail)| tail) else {
        return false;
    };

    tail.is_empty()
        || is_offline_suffix(tail)
        || is_bit_suffix(tail)
        || is_parenthesized_bit_suffix(tail)
}

fn has_linux_for_offline_suffix(name: &str) -> bool {
    let Some(tail) = name.split_once("для Linux").map(|(_, tail)| tail) else {
        return false;
    };

    tail.is_empty() || is_offline_suffix(tail)
}

fn has_linux_systems_suffix(name: &str) -> bool {
    let Some(tail) = name.split_once("Linux-систем").map(|(_, tail)| tail) else {
        return false;
    };

    tail.is_empty() || is_offline_suffix(tail)
}

fn is_offline_suffix(tail: &str) -> bool {
    let tail = tail.trim_start_matches(char::is_whitespace);
    tail.starts_with("без интернета") || tail.starts_with("оффлайн")
}

fn is_bit_suffix(tail: &str) -> bool {
    let tail = tail.trim_start_matches(char::is_whitespace);
    let digit_count = tail.chars().take_while(|ch| ch.is_ascii_digit()).count();
    if digit_count == 0 {
        return false;
    }

    let tail = &tail[digit_count..];
    tail.trim_start_matches(char::is_whitespace)
        .starts_with("Bit")
}

fn is_parenthesized_bit_suffix(tail: &str) -> bool {
    let tail = tail.trim_start_matches(char::is_whitespace);
    let Some(tail) = tail.strip_prefix('(') else {
        return false;
    };
    let digit_count = tail.chars().take_while(|ch| ch.is_ascii_digit()).count();
    if digit_count == 0 {
        return false;
    }

    tail[digit_count..].starts_with("-bit)")
}

fn is_windows_name(name: &str) -> bool {
    name.contains("ОС Windows") || name.ends_with("для Windows") || name.contains("для Windows +")
}

fn is_osx_name(name: &str) -> bool {
    name.ends_with("OS X") || name.ends_with("для macOS")
}

fn is_client_name(name: &str) -> bool {
    name.starts_with("Клиент")
}

fn is_server_name(name: &str) -> bool {
    name.starts_with("Cервер") || name.starts_with("Сервер")
}

fn is_thin_client_name(name: &str) -> bool {
    name.starts_with("Тонкий клиент")
}

fn is_full_name(name: &str) -> bool {
    name.starts_with("Технологическая платформа")
}

fn is_combined_client_package(name: &str) -> bool {
    name.split_once('+')
        .map(|(_, tail)| tail.trim_start().starts_with("Тонкий клиент"))
        .unwrap_or(false)
}

fn is_offline_name(name: &str) -> bool {
    name.contains("без интернета") || name.contains("оффлайн")
}

fn is_client_or_server_name(name: &str) -> bool {
    is_client_name(name) || is_server_name(name)
}

fn is_sha_name(name: &str) -> bool {
    name.contains("Контрольная сумма") || name.contains("sha")
}

pub fn filter_files(files: &[ReleaseFile], artifact_filter: &ArtifactFilter) -> Vec<ReleaseFile> {
    files
        .iter()
        .filter(|file| matches_all(&file.name, artifact_filter))
        .cloned()
        .collect()
}

fn matches_all(name: &str, artifact_filter: &ArtifactFilter) -> bool {
    if is_sha_name(name) {
        return false;
    }

    if let Some(os_name) = artifact_filter.os_name {
        let matched = match os_name {
            OsName::Win => is_windows_name(name),
            OsName::Mac => is_osx_name(name),
            OsName::Linux => is_linux_name(name) && !is_deb_name(name) && !is_rpm_name(name),
            OsName::Deb => is_deb_name(name),
            OsName::Rpm => is_rpm_name(name),
        };
        if !matched {
            return false;
        }
    }

    if let Some(architecture) = artifact_filter.architecture {
        let is_x64 = is_x64_name(name);
        let is_x86 = is_x86_name(name);
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
            DistributiveType::Full => is_full_name(name),
            DistributiveType::ThinClient => is_thin_client_name(name),
            DistributiveType::Server => is_server_name(name),
            DistributiveType::Client => is_client_name(name),
            DistributiveType::ClientOrServer => is_client_or_server_name(name),
        };
        if !matched {
            return false;
        }

        if package_type == DistributiveType::Full
            && artifact_filter.os_name == Some(OsName::Win)
            && is_combined_client_package(name)
        {
            return false;
        }
    }

    if artifact_filter.offline {
        is_offline_name(name)
    } else {
        !is_offline_name(name)
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
