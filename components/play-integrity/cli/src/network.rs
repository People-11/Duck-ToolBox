// SPDX-License-Identifier: GPL-3.0-only
// Modified 2026-10-23: bounded HTTPS transport with embedded CA roots.
// Modified 2026-10-26: synchronous HTTP/1.1 client (ureq, rustls with ring) instead of an
// async runtime, for a smaller engine.
use crate::files;
use anyhow::{Result, bail};
use std::{path::Path, time::Duration};

const BODY_LIMIT: u64 = 4 * 1024 * 1024;

pub fn fetch(args: &[String]) -> Result<()> {
    let mut output = None;
    let mut log = None;
    let mut headers = Vec::new();
    let mut head = false;
    let mut url = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "-O" | "-o" | "-T" | "--header" => {
                let flag = &args[index];
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| anyhow::anyhow!("missing {flag} argument"))?;
                match flag.as_str() {
                    "-O" => output = Some(value.clone()),
                    "-o" => log = Some(value.clone()),
                    "--header" => headers.push(value.clone()),
                    _ => {}
                }
            }
            "--spider" => head = true,
            "-q" | "-S" => {}
            value if value.starts_with('-') => bail!("unsupported fetch option: {value}"),
            value => url = Some(value.to_owned()),
        }
        index += 1;
    }
    let url = url.ok_or_else(|| anyhow::anyhow!("missing URL"))?;
    if !url.starts_with("https://") {
        bail!("only HTTPS downloads are permitted");
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .https_only(true)
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_global(Some(Duration::from_secs(20)))
        .build()
        .into();
    let mut request = if head {
        agent.head(&url)
    } else {
        agent.get(&url)
    };
    for header in headers {
        let (key, value) = header
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("invalid header"))?;
        request = request.header(key.trim(), value.trim());
    }
    let mut response = request.call()?;
    let mut report = format!("HTTP {}\n", response.status());
    for (key, value) in response.headers() {
        let key = if key.as_str() == "last-modified" {
            "Last-Modified"
        } else {
            key.as_str()
        };
        report.push_str(&format!("{key}: {}\n", value.to_str().unwrap_or_default()));
    }
    if let Some(path) = log {
        files::atomic(Path::new(&path), report.as_bytes())?;
    } else {
        eprint!("{report}");
    }
    if head {
        return Ok(());
    }
    let body = response
        .body_mut()
        .with_config()
        .limit(BODY_LIMIT)
        .read_to_vec()
        .map_err(|error| match error {
            ureq::Error::BodyExceedsLimit(_) => anyhow::anyhow!("download exceeds 4 MiB"),
            error => error.into(),
        })?;
    if let Some(path) = output {
        files::atomic(Path::new(&path), &body)?;
    } else {
        use std::io::Write;
        std::io::stdout().write_all(&body)?;
    }
    Ok(())
}
