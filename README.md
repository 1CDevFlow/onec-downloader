# onec-download-rs

Rust CLI/library for downloading release artifacts from `releases.1c.ru`.

## Features

- Auth via `login.1c.ru` form flow
- Cookie-based session handling with redirect following
- HTML parsing for projects, versions, release files, and final download links
- Artifact filtering by OS, architecture, distributive type, and offline flag
- Auto-detection of current OS and default architecture `x64`
- File download with output to a target directory
- Print matched release files without downloading via `--print-files`
- Optional archive extraction via `--extract`

## Usage

Set credentials:

```bash
export ONEC_USERNAME="your-login"
export ONEC_PASSWORD="your-password"
```

Short package-manager-like form:

```bash
cargo run -- Platform83@8.3.25.1286 \
  --verbose \
  --type full \
  --output ./downloads
```

In this form `--os` is auto-detected from the current environment, and `--arch` defaults to `x64`.
If you pass `--os` or `--arch` explicitly, those values are used as-is.
For legacy `Platform83` Linux releases before `8.3.20`, `linux + full` is normalized to `deb + client-or-server`, because those versions ship separate client/server packages instead of a single full installer.
By default, the CLI prints concise stage and download progress messages to `stderr`.
If you pass `--verbose`, the CLI prints compact progress logs to `stderr`.
If you pass `--trace`, the CLI prints full HTTP/auth diagnostics to `stderr`.
If you pass `--print-files`, the CLI prints matched release file names and URLs to `stdout` without downloading anything.
If you pass `--extract`, each downloaded archive is unpacked into its own sibling directory.

Offline EDT release:

```bash
cargo run -- DevelopmentTools10@2023.1.2 \
  --offline \
  --output ./downloads
```

Explicit override:

```bash
cargo run -- Platform83@8.3.25.1286 \
  --os deb \
  --arch x86 \
  --verbose \
  --type client-or-server \
  --output ./downloads
```

Full diagnostic trace:

```bash
cargo run -- Platform83@8.3.25.1286 \
  --trace \
  --type full \
  --output ./downloads
```

Print matched files only:

```bash
cargo run -- Platform83@8.3.27.2074 \
  --type full \
  --print-files
```

Download and unpack archives:

```bash
cargo run -- Platform83@8.3.25.1286 \
  --extract \
  --type full \
  --output ./downloads
```

Verbose compatible form:

```bash
cargo run -- \
  --project Platform83 \
  --version 8.3.25.1286 \
  --type full \
  --output ./downloads
```
