use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Parser;
use onec_download_rs::OnecClient;
use onec_download_rs::model::{
    ArchitectureName, ArtifactFilter, DistributiveType, OsName, ReleaseDescription,
};
use onec_download_rs::unpack::unpack_archives;

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "Download 1C release artifacts from releases.1c.ru",
    disable_version_flag = true
)]
struct Cli {
    #[arg(
        value_name = "PROJECT[@VERSION]",
        help = "Compact package spec, e.g. Platform83@8.3.25.1286"
    )]
    spec: Option<String>,

    #[arg(long)]
    project: Option<String>,

    #[arg(long)]
    version: Option<String>,

    #[arg(long, value_enum)]
    os: Option<OsName>,

    #[arg(long, value_enum)]
    arch: Option<ArchitectureName>,

    #[arg(long = "type", value_enum)]
    package_type: Option<DistributiveType>,

    #[arg(long, default_value_t = false)]
    offline: bool,

    #[arg(long, default_value = ".")]
    output: PathBuf,

    #[arg(long, default_value_t = false)]
    extract: bool,

    #[arg(long, default_value_t = false, help = "Print matched release files and exit")]
    print_files: bool,

    #[arg(long, default_value_t = false)]
    verbose: bool,

    #[arg(long, default_value_t = false)]
    trace: bool,

    #[arg(long, env = "ONEC_USERNAME", hide_env_values = true)]
    username: String,

    #[arg(long, env = "ONEC_PASSWORD", hide_env_values = true)]
    password: String,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.username.is_empty() || cli.password.is_empty() {
        bail!("ONEC_USERNAME and ONEC_PASSWORD must be set");
    }

    let request = build_release_request(&cli)?;
    let client = OnecClient::new(cli.username, cli.password)?
        .with_logging(cli.verbose, cli.trace)
        .with_quiet(cli.print_files);

    if cli.print_files {
        let files = client.matching_release_files(&request)?;
        for file in files {
            println!("{}\t{}", file.name, file.url);
        }
        return Ok(());
    }

    let downloaded = client
        .download_release(&request, &cli.output)
        .with_context(|| format!("download failed into {}", cli.output.display()))?;

    let results = if cli.extract {
        unpack_archives(&downloaded, cli.verbose, cli.trace)?
    } else {
        downloaded
    };

    if cli.verbose || cli.trace {
        for path in results {
            println!("{}", path.display());
        }
    }

    Ok(())
}

fn build_release_request(cli: &Cli) -> Result<ReleaseDescription> {
    let (spec_project, spec_version) = parse_spec(cli.spec.as_deref())?;
    let project = cli
        .project
        .clone()
        .or(spec_project)
        .context("project must be provided via PROJECT@VERSION or --project")?;
    let version = cli
        .version
        .clone()
        .or(spec_version)
        .context("version must be provided via PROJECT@VERSION or --version")?;

    Ok(normalize_release_request(ReleaseDescription {
        project,
        version,
        filter: ArtifactFilter {
            os_name: cli.os.or_else(detect_os),
            architecture: cli.arch.or(Some(ArchitectureName::X64)),
            package_type: cli.package_type,
            offline: cli.offline,
        },
    }))
}

fn parse_spec(spec: Option<&str>) -> Result<(Option<String>, Option<String>)> {
    let Some(spec) = spec else {
        return Ok((None, None));
    };

    let spec = spec.trim();
    if spec.is_empty() {
        bail!("package spec must not be empty");
    }

    match spec.split_once('@') {
        Some((project, version)) => {
            let project = project.trim();
            let version = version.trim();
            if project.is_empty() || version.is_empty() {
                bail!("package spec must look like PROJECT@VERSION");
            }
            Ok((Some(project.to_owned()), Some(version.to_owned())))
        }
        None => Ok((Some(spec.to_owned()), None)),
    }
}

fn detect_os() -> Option<OsName> {
    match std::env::consts::OS {
        "linux" => Some(OsName::Linux),
        "windows" => Some(OsName::Win),
        "macos" => Some(OsName::Mac),
        _ => None,
    }
}

fn normalize_release_request(mut request: ReleaseDescription) -> ReleaseDescription {
    request.project = normalize_project_alias(&request.project, &request.version);

    if request.project == "Platform83"
        && request.filter.os_name == Some(OsName::Linux)
        && request.filter.package_type == Some(DistributiveType::Client)
    {
        request.filter.package_type = Some(DistributiveType::ThinClient);
    }

    if request.project == "Platform83"
        && request.filter.os_name == Some(OsName::Linux)
        && request.filter.package_type == Some(DistributiveType::Full)
        && is_legacy_linux_platform_version(&request.version)
    {
        request.filter.os_name = Some(OsName::Deb);
        request.filter.package_type = Some(DistributiveType::ClientOrServer);
    }

    request
}

fn normalize_project_alias(project: &str, version: &str) -> String {
    if project != "Platform" {
        return project.to_owned();
    }

    match parse_version_prefix(version) {
        Some([8, minor]) => format!("Platform8{minor}"),
        _ => project.to_owned(),
    }
}

fn is_legacy_linux_platform_version(version: &str) -> bool {
    parse_version_key(version)
        .map(|version_key| version_key < [8, 3, 20, 0])
        .unwrap_or(false)
}

fn parse_version_prefix(version: &str) -> Option<[u32; 2]> {
    let mut parts = version.split('.').map(|part| part.parse::<u32>().ok());
    let parsed = [parts.next()??, parts.next()??];
    Some(parsed)
}

fn parse_version_key(version: &str) -> Option<[u32; 4]> {
    let mut parts = version.split('.').map(|part| part.parse::<u32>().ok());
    let parsed = [
        parts.next()??,
        parts.next()??,
        parts.next()??,
        parts.next()??,
    ];

    if parts.next().is_some() {
        return None;
    }

    Some(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_project_and_version_from_spec() {
        let (project, version) = parse_spec(Some("Platform83@8.3.25.1286")).unwrap();
        assert_eq!(project.as_deref(), Some("Platform83"));
        assert_eq!(version.as_deref(), Some("8.3.25.1286"));
    }

    #[test]
    fn parses_project_only_from_spec() {
        let (project, version) = parse_spec(Some("Platform83")).unwrap();
        assert_eq!(project.as_deref(), Some("Platform83"));
        assert_eq!(version, None);
    }

    #[test]
    fn explicit_values_override_defaults() {
        let cli = Cli {
            spec: Some("Platform83@8.3.25.1286".into()),
            project: None,
            version: None,
            os: Some(OsName::Deb),
            arch: Some(ArchitectureName::X86),
            package_type: Some(DistributiveType::Full),
            offline: false,
            output: PathBuf::from("."),
            extract: false,
            print_files: false,
            verbose: false,
            trace: false,
            username: "user".into(),
            password: "pass".into(),
        };

        let request = build_release_request(&cli).unwrap();
        assert_eq!(request.filter.os_name, Some(OsName::Deb));
        assert_eq!(request.filter.architecture, Some(ArchitectureName::X86));
    }

    #[test]
    fn defaults_architecture_to_x64() {
        let cli = Cli {
            spec: Some("Platform83@8.3.25.1286".into()),
            project: None,
            version: None,
            os: None,
            arch: None,
            package_type: None,
            offline: false,
            output: PathBuf::from("."),
            extract: false,
            print_files: false,
            verbose: false,
            trace: false,
            username: "user".into(),
            password: "pass".into(),
        };

        let request = build_release_request(&cli).unwrap();
        assert_eq!(request.filter.architecture, Some(ArchitectureName::X64));
    }

    #[test]
    fn rewrites_legacy_linux_full_to_deb_client_or_server() {
        let cli = Cli {
            spec: Some("Platform83@8.3.19.1770".into()),
            project: None,
            version: None,
            os: Some(OsName::Linux),
            arch: Some(ArchitectureName::X64),
            package_type: Some(DistributiveType::Full),
            offline: false,
            output: PathBuf::from("."),
            extract: false,
            print_files: false,
            verbose: false,
            trace: false,
            username: "user".into(),
            password: "pass".into(),
        };

        let request = build_release_request(&cli).unwrap();
        assert_eq!(request.filter.os_name, Some(OsName::Deb));
        assert_eq!(
            request.filter.package_type,
            Some(DistributiveType::ClientOrServer)
        );
    }

    #[test]
    fn keeps_new_linux_full_as_is() {
        let cli = Cli {
            spec: Some("Platform83@8.3.20.1000".into()),
            project: None,
            version: None,
            os: Some(OsName::Linux),
            arch: Some(ArchitectureName::X64),
            package_type: Some(DistributiveType::Full),
            offline: false,
            output: PathBuf::from("."),
            extract: false,
            print_files: false,
            verbose: false,
            trace: false,
            username: "user".into(),
            password: "pass".into(),
        };

        let request = build_release_request(&cli).unwrap();
        assert_eq!(request.filter.os_name, Some(OsName::Linux));
        assert_eq!(request.filter.package_type, Some(DistributiveType::Full));
    }

    #[test]
    fn rewrites_platform83_linux_client_to_thin_client() {
        let cli = Cli {
            spec: Some("Platform83@8.3.27.2074".into()),
            project: None,
            version: None,
            os: Some(OsName::Linux),
            arch: Some(ArchitectureName::X64),
            package_type: Some(DistributiveType::Client),
            offline: false,
            output: PathBuf::from("."),
            extract: false,
            print_files: false,
            verbose: false,
            trace: false,
            username: "user".into(),
            password: "pass".into(),
        };

        let request = build_release_request(&cli).unwrap();
        assert_eq!(request.filter.os_name, Some(OsName::Linux));
        assert_eq!(request.filter.package_type, Some(DistributiveType::ThinClient));
    }

    #[test]
    fn rewrites_platform_alias_for_83_branch() {
        let cli = Cli {
            spec: Some("Platform@8.3.27".into()),
            project: None,
            version: None,
            os: None,
            arch: None,
            package_type: None,
            offline: false,
            output: PathBuf::from("."),
            extract: false,
            print_files: false,
            verbose: false,
            trace: false,
            username: "user".into(),
            password: "pass".into(),
        };

        let request = build_release_request(&cli).unwrap();
        assert_eq!(request.project, "Platform83");
        assert_eq!(request.version, "8.3.27");
    }

    #[test]
    fn rewrites_platform_alias_for_85_branch() {
        let cli = Cli {
            spec: Some("Platform@8.5.1".into()),
            project: None,
            version: None,
            os: None,
            arch: None,
            package_type: None,
            offline: false,
            output: PathBuf::from("."),
            extract: false,
            print_files: false,
            verbose: false,
            trace: false,
            username: "user".into(),
            password: "pass".into(),
        };

        let request = build_release_request(&cli).unwrap();
        assert_eq!(request.project, "Platform85");
        assert_eq!(request.version, "8.5.1");
    }

    #[test]
    fn rewrites_platform_alias_for_80_branch() {
        let cli = Cli {
            spec: Some("Platform@8.0.1".into()),
            project: None,
            version: None,
            os: None,
            arch: None,
            package_type: None,
            offline: false,
            output: PathBuf::from("."),
            extract: false,
            print_files: false,
            verbose: false,
            trace: false,
            username: "user".into(),
            password: "pass".into(),
        };

        let request = build_release_request(&cli).unwrap();
        assert_eq!(request.project, "Platform80");
        assert_eq!(request.version, "8.0.1");
    }

    #[test]
    fn rewrites_platform_alias_for_81_branch() {
        let cli = Cli {
            spec: Some("Platform@8.1.2".into()),
            project: None,
            version: None,
            os: None,
            arch: None,
            package_type: None,
            offline: false,
            output: PathBuf::from("."),
            extract: false,
            print_files: false,
            verbose: false,
            trace: false,
            username: "user".into(),
            password: "pass".into(),
        };

        let request = build_release_request(&cli).unwrap();
        assert_eq!(request.project, "Platform81");
        assert_eq!(request.version, "8.1.2");
    }
}
