//! Helper programs Canapé downloads on demand rather than bundling, both
//! GPL-3 and run as separate programs: Legendary for Epic games, gogdl for
//! GOG games. Fetched from their official GitHub releases into Canapé's own
//! folder and checked against a SHA-256 digest: the one GitHub publishes
//! for Legendary's latest release, the one written here for gogdl's pinned
//! release.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::online::Http;

/// Latest Legendary release, from the GitHub API.
pub const LEGENDARY_RELEASE: &str =
    "https://api.github.com/repos/legendary-gl/legendary/releases/latest";
/// Only files from Legendary's own releases are accepted: the project's
/// current home, and its former one, which GitHub redirects to it.
const LEGENDARY_DOWNLOADS: [&str; 2] = [
    "https://github.com/legendary-gl/legendary/releases/download/",
    "https://github.com/derrod/legendary/releases/download/",
];
/// Name of the downloaded executable in Canapé's folder.
const LEGENDARY_ASSET: &str = "legendary.exe";
/// Names of the Windows executable among a release's files, newest naming
/// first (it was `legendary.exe` up to 0.20).
const LEGENDARY_RELEASE_ASSETS: [&str; 2] = ["legendary_windows_x64.exe", LEGENDARY_ASSET];
/// Largest executable accepted (the PyInstaller build is about 20 MB).
const MAX_TOOL_BYTES: usize = 80 * 1024 * 1024;

/// The gogdl release Canapé uses: its commands and output are what
/// `gogdl.rs` and `install.rs` expect.
pub const GOGDL_VERSION: &str = "1.3.1";
/// Its Windows executable.
const GOGDL_URL: &str = "https://github.com/Heroic-Games-Launcher/heroic-gogdl/releases/download/v1.3.1/gogdl_windows_x86_64.exe";
/// SHA-256 of that file, as published on the release.
const GOGDL_SHA256: &str = "1866e0ead43a4f5a02f1d750c79a69a1d285c09dc05b6fdce3b28c84bb2a4c5f";

/// Where gogdl lives once downloaded; the version is in the name, so a new
/// pinned release is downloaded again.
pub fn gogdl_path(tools_dir: &Path) -> PathBuf {
    tools_dir.join(format!("gogdl-{GOGDL_VERSION}.exe"))
}

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
    let from_legendary = LEGENDARY_DOWNLOADS
        .iter()
        .any(|prefix| asset.browser_download_url.starts_with(prefix));
    (valid && from_legendary).then_some(Download {
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
    let path = legendary_path(tools_dir);
    fetch_checked(http, &download.url, &download.sha256, &path)
        .map_err(|err| io::Error::other(format!("legendary.exe {}: {err}", download.version)))?;
    Ok(path)
}

/// Downloads gogdl's pinned release into `tools_dir`, checked against its
/// digest. Returns the executable's path.
pub fn fetch_gogdl(http: &dyn Http, tools_dir: &Path) -> io::Result<PathBuf> {
    let path = gogdl_path(tools_dir);
    fetch_checked(http, GOGDL_URL, GOGDL_SHA256, &path)
        .map_err(|err| io::Error::other(format!("gogdl {GOGDL_VERSION}: {err}")))?;
    Ok(path)
}

/// Downloads `url` to `path` if its SHA-256 is `sha256` (hex); otherwise
/// writes nothing.
fn fetch_checked(http: &dyn Http, url: &str, sha256: &str, path: &Path) -> io::Result<()> {
    let bytes = http.get_limited(url, None, MAX_TOOL_BYTES)?;
    let actual = hex(&crate::sha256::digest(&bytes));
    if actual != sha256 {
        return Err(io::Error::other(format!(
            "SHA-256 {actual} instead of {sha256}"
        )));
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let part = path.with_extension("part");
    fs::write(&part, &bytes)?;
    fs::rename(&part, path)
}

#[cfg(test)]
mod tests {
    const OLD: &str = "https://github.com/derrod/legendary/releases/download/";
    const NEW: &str = "https://github.com/legendary-gl/legendary/releases/download/";
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

        fn post(&self, url: &str, _: &[(&str, &str)], _: &[u8]) -> io::Result<Vec<u8>> {
            panic!("no POST expected: {url}")
        }
    }

    fn release(url: &str, digest: &str) -> Vec<u8> {
        format!(
            r#"{{"tag_name":"0.20.37","assets":[
                {{"name":"legendary","browser_download_url":"{OLD}0.20.37/legendary","digest":null}},
                {{"name":"legendary.exe","browser_download_url":"{url}","digest":"{digest}"}}]}}"#
        )
        .into_bytes()
    }

    #[test]
    fn releases_need_a_digest_and_the_official_link() {
        let url = format!("{OLD}0.20.37/legendary.exe");
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
                {{"name":"legendary_linux_x64","browser_download_url":"{NEW}0.21.1/legendary_linux_x64","digest":"{digest}"}},
                {{"name":"legendary_windows_arm64.exe","browser_download_url":"{NEW}0.21.1/legendary_windows_arm64.exe","digest":"{digest}"}},
                {{"name":"legendary_windows_x64.exe","browser_download_url":"{NEW}0.21.1/legendary_windows_x64.exe","digest":"{digest}"}}]}}"#
        );
        let found = parse_release(json.as_bytes()).unwrap();
        assert!(found.url.ends_with("/legendary_windows_x64.exe"));
        assert_eq!(found.version, "0.21.1");
    }

    #[test]
    fn the_download_is_checked() {
        let dir = std::env::temp_dir().join(format!("canape-tools-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let url = format!("{OLD}0.20.37/legendary.exe");
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

    #[test]
    fn gogdl_is_pinned() {
        assert!(GOGDL_URL.contains(&format!("/v{GOGDL_VERSION}/")));
        assert!(GOGDL_URL.starts_with(
            "https://github.com/Heroic-Games-Launcher/heroic-gogdl/releases/download/"
        ));
        assert_eq!(GOGDL_SHA256.len(), 64);
        let dir = std::env::temp_dir().join(format!("canape-gogdl-tool-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        // Any other file than the pinned one is refused.
        let mut answers = HashMap::new();
        answers.insert(GOGDL_URL.to_owned(), b"MZ not the release".to_vec());
        let http = Fake(answers, RefCell::new(Vec::new()));
        assert!(fetch_gogdl(&http, &dir).is_err());
        assert!(!gogdl_path(&dir).exists());
        assert!(gogdl_path(&dir).ends_with("gogdl-1.3.1.exe"));
    }
}
