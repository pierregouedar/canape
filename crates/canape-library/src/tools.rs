//! Helper programs Canapé downloads on demand rather than bundling:
//! Legendary (GPL-3, run as a separate program) for Epic games. Fetched
//! from its official GitHub releases into Canapé's own folder, checked
//! against the SHA-256 digest GitHub publishes for the file.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::online::Http;

/// Latest Legendary release, from the GitHub API.
pub const LEGENDARY_RELEASE: &str = "https://api.github.com/repos/derrod/legendary/releases/latest";
/// Only files from Legendary's own releases are accepted.
const LEGENDARY_DOWNLOADS: &str = "https://github.com/derrod/legendary/releases/download/";
/// Name of the downloaded executable in Canapé's folder.
const LEGENDARY_ASSET: &str = "legendary.exe";
/// Names of the Windows executable among a release's files, newest naming
/// first (it was `legendary.exe` up to 0.20).
const LEGENDARY_RELEASE_ASSETS: [&str; 2] = ["legendary_windows_x64.exe", LEGENDARY_ASSET];
/// Largest executable accepted (the PyInstaller build is about 20 MB).
const MAX_TOOL_BYTES: usize = 80 * 1024 * 1024;

/// Where `legendary.exe` lives once downloaded.
pub fn legendary_path(tools_dir: &Path) -> PathBuf {
    tools_dir.join(LEGENDARY_ASSET)
}

#[derive(Deserialize)]
struct Release {
    #[serde(default)]
    tag_name: String,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    digest: Option<String>,
}

/// A file of a release: its version, link and SHA-256 (hex).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Download {
    pub version: String,
    pub url: String,
    pub sha256: String,
}

/// The Legendary executable of a GitHub release answer. Without a
/// published digest, or with a link outside Legendary's releases, nothing
/// is downloaded.
pub fn parse_release(json: &[u8]) -> Option<Download> {
    let mut release: Release = serde_json::from_slice(json).ok()?;
    let asset = LEGENDARY_RELEASE_ASSETS.iter().find_map(|name| {
        let i = release.assets.iter().position(|a| a.name == *name)?;
        Some(release.assets.swap_remove(i))
    })?;
    let sha256 = asset.digest?.strip_prefix("sha256:")?.to_ascii_lowercase();
    let valid = sha256.len() == 64 && sha256.bytes().all(|b| b.is_ascii_hexdigit());
    (valid && asset.browser_download_url.starts_with(LEGENDARY_DOWNLOADS)).then_some(Download {
        version: release.tag_name,
        url: asset.browser_download_url,
        sha256,
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Downloads the latest Legendary into `tools_dir`, checked against its
/// digest. Returns the executable's path.
pub fn fetch_legendary(http: &dyn Http, tools_dir: &Path) -> io::Result<PathBuf> {
    let release = http.get(LEGENDARY_RELEASE, None)?;
    let download = parse_release(&release)
        .ok_or_else(|| io::Error::other("no checked Legendary release found"))?;
    let bytes = http.get_limited(&download.url, None, MAX_TOOL_BYTES)?;
    let actual = hex(&crate::sha256::digest(&bytes));
    if actual != download.sha256 {
        return Err(io::Error::other(format!(
            "legendary.exe {}: SHA-256 {actual} instead of {}",
            download.version, download.sha256
        )));
    }
    fs::create_dir_all(tools_dir)?;
    let path = legendary_path(tools_dir);
    let part = path.with_extension("part");
    fs::write(&part, &bytes)?;
    fs::rename(&part, &path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    struct Fake(HashMap<String, Vec<u8>>, RefCell<Vec<String>>);

    impl Http for Fake {
        fn get_limited(&self, url: &str, _: Option<&str>, _: usize) -> io::Result<Vec<u8>> {
            self.1.borrow_mut().push(url.to_owned());
            self.0
                .get(url)
                .cloned()
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "404"))
        }
    }

    fn release(url: &str, digest: &str) -> Vec<u8> {
        format!(
            r#"{{"tag_name":"0.20.37","assets":[
                {{"name":"legendary","browser_download_url":"{LEGENDARY_DOWNLOADS}0.20.37/legendary","digest":null}},
                {{"name":"legendary.exe","browser_download_url":"{url}","digest":"{digest}"}}]}}"#
        )
        .into_bytes()
    }

    #[test]
    fn releases_need_a_digest_and_the_official_link() {
        let url = format!("{LEGENDARY_DOWNLOADS}0.20.37/legendary.exe");
        let digest = format!("sha256:{}", "AB".repeat(32));
        let found = parse_release(&release(&url, &digest)).unwrap();
        assert_eq!(found.sha256, "ab".repeat(32));
        assert_eq!(found.version, "0.20.37");
        assert!(parse_release(&release("https://evil.example/legendary.exe", &digest)).is_none());
        assert!(parse_release(&release(&url, "sha256:1234")).is_none());
        assert!(parse_release(&release(&url, "md5:abcd")).is_none());
        assert!(parse_release(b"{}").is_none());
    }

    #[test]
    fn the_new_windows_asset_name_is_preferred() {
        let digest = format!("sha256:{}", "cd".repeat(32));
        let json = format!(
            r#"{{"tag_name":"0.21.1","assets":[
                {{"name":"legendary_linux_x64","browser_download_url":"{LEGENDARY_DOWNLOADS}0.21.1/legendary_linux_x64","digest":"{digest}"}},
                {{"name":"legendary_windows_arm64.exe","browser_download_url":"{LEGENDARY_DOWNLOADS}0.21.1/legendary_windows_arm64.exe","digest":"{digest}"}},
                {{"name":"legendary_windows_x64.exe","browser_download_url":"{LEGENDARY_DOWNLOADS}0.21.1/legendary_windows_x64.exe","digest":"{digest}"}}]}}"#
        );
        let found = parse_release(json.as_bytes()).unwrap();
        assert!(found.url.ends_with("/legendary_windows_x64.exe"));
        assert_eq!(found.version, "0.21.1");
    }

    #[test]
    fn the_download_is_checked() {
        let dir = std::env::temp_dir().join(format!("canape-tools-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let url = format!("{LEGENDARY_DOWNLOADS}0.20.37/legendary.exe");
        let exe = b"MZ fake executable".to_vec();
        let good = format!("sha256:{}", hex(&crate::sha256::digest(&exe)));

        let mut answers = HashMap::new();
        answers.insert(LEGENDARY_RELEASE.to_owned(), release(&url, &good));
        answers.insert(url.clone(), exe.clone());
        let http = Fake(answers, RefCell::new(Vec::new()));
        let path = fetch_legendary(&http, &dir).unwrap();
        assert_eq!(fs::read(&path).unwrap(), exe);

        // A file that does not match its digest is never written.
        let _ = fs::remove_dir_all(&dir);
        let bad = format!("sha256:{}", "0".repeat(64));
        let mut answers = HashMap::new();
        answers.insert(LEGENDARY_RELEASE.to_owned(), release(&url, &bad));
        answers.insert(url, exe);
        let http = Fake(answers, RefCell::new(Vec::new()));
        assert!(fetch_legendary(&http, &dir).is_err());
        assert!(!legendary_path(&dir).exists());
    }
}
