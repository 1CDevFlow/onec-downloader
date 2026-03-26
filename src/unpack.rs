use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

pub fn unpack_archives(archives: &[PathBuf], verbose: bool, trace: bool) -> Result<Vec<PathBuf>> {
    let mut extracted = Vec::with_capacity(archives.len());

    for archive in archives {
        let destination = extraction_dir(archive)?;
        fs::create_dir_all(&destination)
            .with_context(|| format!("failed to create {}", destination.display()))?;

        render_extracting(verbose, trace, archive, &destination);
        run_unpack_command(archive, &destination, verbose, trace)
            .with_context(|| format!("failed to extract {}", archive.display()))?;
        render_extracted(verbose, trace, archive, &destination);

        extracted.push(destination);
    }

    Ok(extracted)
}

fn run_unpack_command(
    archive: &Path,
    destination: &Path,
    verbose: bool,
    trace: bool,
) -> Result<()> {
    let archive_name = archive.display().to_string();

    let mut command = if archive_name.ends_with(".zip") {
        let mut command = Command::new("unzip");
        command.arg("-o").arg(archive).arg("-d").arg(destination);
        command
    } else if archive_name.ends_with(".tar.gz") || archive_name.ends_with(".tgz") {
        let mut command = Command::new("tar");
        command.arg("-xzf").arg(archive).arg("-C").arg(destination);
        command
    } else if archive_name.ends_with(".tar") {
        let mut command = Command::new("tar");
        command.arg("-xf").arg(archive).arg("-C").arg(destination);
        command
    } else if archive_name.ends_with(".rar") {
        let mut command = Command::new("7z");
        let output_arg = OsString::from(format!("-o{}", destination.display()));
        command.arg("x").arg(archive).arg(output_arg).arg("-y");
        command
    } else {
        bail!("unsupported archive format: {}", archive.display());
    };

    if trace {
        command.stdout(Stdio::inherit()).stderr(Stdio::inherit());
    } else {
        command.stdout(Stdio::null()).stderr(Stdio::null());
    }

    if verbose {
        emit_stage(
            verbose,
            trace,
            &format!("running extractor for {}", short_path(archive)),
        );
    }

    let status = command
        .status()
        .with_context(|| format!("failed to start extractor for {}", archive.display()))?;
    if status.success() {
        return Ok(());
    }

    if archive_name.ends_with(".rar")
        && status.code() == Some(2)
        && directory_has_files(destination)?
    {
        emit_stage(
            verbose,
            trace,
            &format!(
                "extractor reported warnings for {}, but files were extracted",
                short_path(archive)
            ),
        );
        return Ok(());
    }

    if !status.success() {
        bail!(
            "extractor failed for {} with status {}",
            archive.display(),
            status
        );
    }

    Ok(())
}

fn directory_has_files(path: &Path) -> Result<bool> {
    let mut entries = fs::read_dir(path)
        .with_context(|| format!("failed to inspect extraction directory {}", path.display()))?;
    Ok(entries.next().transpose()?.is_some())
}

fn emit_stage(verbose: bool, trace: bool, message: &str) {
    if trace {
        eprintln!("[onec-download-rs] trace extract   | {message}");
    } else if verbose {
        eprintln!("[onec-download-rs] info  extract   | {message}");
    } else {
        eprintln!("{message}");
    }
}

fn render_extracting(verbose: bool, trace: bool, archive: &Path, destination: &Path) {
    emit_stage(
        verbose,
        trace,
        &format!(
            "{} extracting {} -> {}",
            cyan_dot(),
            short_path(archive),
            short_path(destination)
        ),
    );
}

fn render_extracted(verbose: bool, trace: bool, archive: &Path, destination: &Path) {
    if trace || verbose {
        emit_stage(
            verbose,
            trace,
            &format!(
                "extracted {} -> {}",
                short_path(archive),
                short_path(destination)
            ),
        );
        return;
    }

    eprint!("\x1b[1A\r\x1b[2K");
    eprintln!(
        "{} {} -> {}",
        green_check(),
        short_path(archive),
        short_path(destination)
    );
}

fn extraction_dir(archive: &Path) -> Result<PathBuf> {
    let file_name = archive
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow::anyhow!("archive path has no valid file name"))?;

    let base_name = strip_archive_extension(file_name)
        .ok_or_else(|| anyhow::anyhow!("unsupported archive format: {}", archive.display()))?;

    let parent = archive.parent().unwrap_or_else(|| Path::new("."));
    Ok(parent.join(base_name))
}

fn strip_archive_extension(file_name: &str) -> Option<&str> {
    file_name
        .strip_suffix(".tar.gz")
        .or_else(|| file_name.strip_suffix(".tgz"))
        .or_else(|| file_name.strip_suffix(".tar"))
        .or_else(|| file_name.strip_suffix(".zip"))
        .or_else(|| file_name.strip_suffix(".rar"))
}

fn short_path(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_else(|| path.as_os_str().to_str().unwrap_or("<path>"))
        .to_owned()
}

fn green_check() -> &'static str {
    "\x1b[32m✓\x1b[0m"
}

fn cyan_dot() -> &'static str {
    "\x1b[36m●\x1b[0m"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_supported_archive_extensions() {
        assert_eq!(strip_archive_extension("a.zip"), Some("a"));
        assert_eq!(strip_archive_extension("a.rar"), Some("a"));
        assert_eq!(strip_archive_extension("a.tar"), Some("a"));
        assert_eq!(strip_archive_extension("a.tar.gz"), Some("a"));
        assert_eq!(strip_archive_extension("a.tgz"), Some("a"));
    }

    #[test]
    fn builds_destination_directory_next_to_archive() {
        let archive = Path::new("/tmp/downloads/server64_8_3_27_2074.zip");
        let destination = extraction_dir(archive).unwrap();
        assert_eq!(
            destination,
            PathBuf::from("/tmp/downloads/server64_8_3_27_2074")
        );
    }

    #[test]
    fn detects_non_empty_directory() {
        let base = std::env::temp_dir().join(format!("onec-download-rs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        assert!(!directory_has_files(&base).unwrap());

        fs::write(base.join("file.txt"), "ok").unwrap();
        assert!(directory_has_files(&base).unwrap());

        let _ = fs::remove_dir_all(&base);
    }
}
