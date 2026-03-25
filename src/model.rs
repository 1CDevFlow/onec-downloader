use clap::ValueEnum;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub name: String,
    pub url: String,
    pub files: Vec<ReleaseFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseFile {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseDescription {
    pub project: String,
    pub version: String,
    pub filter: ArtifactFilter,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArtifactFilter {
    pub os_name: Option<OsName>,
    pub architecture: Option<ArchitectureName>,
    pub package_type: Option<DistributiveType>,
    pub offline: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OsName {
    Win,
    Mac,
    Linux,
    Deb,
    Rpm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ArchitectureName {
    X86,
    X64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum DistributiveType {
    Full,
    ThinClient,
    Server,
    Client,
    ClientOrServer,
}
