# onec-download-rs

Rust CLI/library for downloading release artifacts from `releases.1c.ru`.

## Features

- Auth via `login.1c.ru` ticket flow
- Cookie-based session handling with redirect following
- HTML parsing for projects, versions, release files, and final download links
- Artifact filtering by OS, architecture, distributive type, and offline flag
- Auto-detection of current OS and default architecture `x64`
- File download with output to a target directory

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
If you pass `--verbose`, the CLI prints detailed progress logs to `stderr`.

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

Verbose compatible form:

```bash
cargo run -- \
  --project Platform83 \
  --version 8.3.25.1286 \
  --type full \
  --output ./downloads
```
