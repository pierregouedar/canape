//! Xbox app / Microsoft Store PC games. **Untested on Windows**: written
//! from the documented package formats, without an Xbox game to try it on.
//!
//! The Xbox app installs games under an `XboxGames` folder (one per drive,
//! listed in `<drive>:\.GamingRoot`), as `XboxGames\<Game>\Content\` with a
//! `MicrosoftGame.config` (display name, logos) and the package's
//! `appxmanifest.xml` (identity, application id). A game is started through
//! its AUMID, `shell:AppsFolder\<PackageFamilyName>!<AppId>`; the family
//! name is the package name plus a hash of its publisher, computed here so
//! that no package API (WinRT) is needed.
//!
//! Limits: Store games installed elsewhere than an `XboxGames` folder (in
//! `WindowsApps`) are not listed.

use std::path::{Path, PathBuf};

use crate::xml;
use crate::{Game, Launch, Source};

/// The 13-character publisher id of package family names: the first 8
/// bytes of SHA-256 over the UTF-16LE publisher, in Crockford-like base 32.
pub fn publisher_id(publisher: &str) -> String {
    const ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";
    let utf16: Vec<u8> = publisher
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let hash = crate::sha256::digest(&utf16);
    // 64 bits plus one zero bit make 13 groups of 5.
    let bits = (u128::from(u64::from_be_bytes(hash[..8].try_into().unwrap()))) << 1;
    (0..13)
        .map(|i| ALPHABET[((bits >> (5 * (12 - i))) & 31) as usize] as char)
        .collect()
}

/// `<name>_<publisher id>`.
pub fn family_name(name: &str, publisher: &str) -> String {
    format!("{name}_{}", publisher_id(publisher))
}

/// Folders named in a `.GamingRoot` file (`RGBX`, a count, then
/// NUL-terminated UTF-16 paths relative to the drive root).
pub fn parse_gaming_root(bytes: &[u8]) -> Vec<String> {
    let body = bytes
        .strip_prefix(b"RGBX")
        .map_or(bytes, |b| b.get(4..).unwrap_or_default());
    let units: Vec<u16> = body
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    units
        .split(|&u| u == 0)
        .map(String::from_utf16_lossy)
        .map(|s| s.trim().trim_start_matches(['\\', '/']).to_owned())
        .filter(|s| !s.is_empty() && !s.chars().any(char::is_control))
        .collect()
}

/// `XboxGames` folders of every drive.
#[cfg(windows)]
pub fn roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for drive in crate::win::drives() {
        let root = PathBuf::from(format!("{drive}:\\"));
        let listed = std::fs::read(root.join(".GamingRoot"))
            .map(|bytes| parse_gaming_root(&bytes))
            .unwrap_or_default();
        let folders = if listed.is_empty() {
            vec!["XboxGames".to_string()]
        } else {
            listed
        };
        for folder in folders {
            let dir = root.join(folder);
            if dir.is_dir() && !roots.contains(&dir) {
                roots.push(dir);
            }
        }
    }
    roots
}

/// What the two manifests of one game folder say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    pub name: String,
    pub publisher: String,
    pub app_id: String,
    pub title: Option<String>,
    /// Logo paths relative to `Content`, best first.
    pub logos: Vec<String>,
}

/// Reads `MicrosoftGame.config` and/or `appxmanifest.xml`; the identity
/// must be in one of them.
pub fn parse_package(config: Option<&str>, manifest: Option<&str>) -> Option<Package> {
    let from = |name: &str, attr: &str| {
        let get = |text: Option<&str>| xml::attribute(text?, name, attr).filter(|v| !v.is_empty());
        get(manifest).or_else(|| get(config))
    };
    let name = from("Identity", "Name")?;
    let publisher = from("Identity", "Publisher")?;
    let app_id = manifest
        .and_then(|m| xml::attribute(m, "Application", "Id"))
        .or_else(|| config.and_then(|c| xml::attribute(c, "Executable", "Id")))
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "Game".into());
    let visual = |attr: &str| config.and_then(|c| xml::attribute(c, "ShellVisuals", attr));
    let title = visual("DefaultDisplayName")
        .or_else(|| manifest.and_then(|m| xml::attribute(m, "VisualElements", "DisplayName")))
        .filter(|t| !t.trim().is_empty() && !t.starts_with("ms-resource:"));
    let mut logos: Vec<String> = ["Square480x480Logo", "Square150x150Logo", "StoreLogo"]
        .iter()
        .filter_map(|attr| visual(attr))
        .collect();
    logos.extend(manifest.and_then(|m| xml::attribute(m, "VisualElements", "Square150x150Logo")));
    logos.retain(|l| !l.is_empty());
    Some(Package {
        name,
        publisher,
        app_id,
        title,
        logos,
    })
}

/// `logo` under `content`, or its scaled variant (`Logo.png` may exist as
/// `Logo.scale-200.png`), the largest scale first.
fn find_logo(content: &Path, logo: &str) -> Option<PathBuf> {
    let path = logo
        .split(['\\', '/'])
        .filter(|p| !p.is_empty())
        .fold(content.to_path_buf(), |p, part| p.join(part));
    if path.is_file() {
        return Some(path);
    }
    let (dir, file) = (path.parent()?, path.file_name()?.to_str()?);
    let (stem, ext) = file.rsplit_once('.')?;
    let mut variants: Vec<(u32, PathBuf)> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let middle = name.strip_prefix(stem)?.strip_suffix(ext)?;
            let scale = middle
                .strip_prefix(".scale-")?
                .strip_suffix('.')?
                .parse()
                .ok()?;
            Some((scale, entry.path()))
        })
        .collect();
    variants.sort();
    variants.pop().map(|(_, p)| p)
}

/// Games in one `XboxGames` folder.
pub fn scan(root: &Path) -> Vec<Game> {
    let mut folders: Vec<PathBuf> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    folders.sort();
    let mut games = Vec::new();
    for folder in folders {
        let content = folder.join("Content");
        let read = |name: &str| {
            std::fs::read(content.join(name))
                .ok()
                .map(|b| String::from_utf8_lossy(&b).into_owned())
        };
        let (config, manifest) = (read("MicrosoftGame.config"), read("appxmanifest.xml"));
        let Some(package) = parse_package(config.as_deref(), manifest.as_deref()) else {
            continue;
        };
        let family = family_name(&package.name, &package.publisher);
        let title = package.title.clone().unwrap_or_else(|| {
            folder
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| package.name.clone())
        });
        games.push(Game {
            id: format!("xbox:{family}"),
            title,
            source: Source::Xbox,
            cover: package.logos.iter().find_map(|l| find_logo(&content, l)),
            launch: Launch::Uri {
                uri: format!(r"shell:AppsFolder\{family}!{}", package.app_id),
            },
            last_played: None,
            size: None,
            playtime: None,
        });
    }
    games
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::fixture;

    #[test]
    fn publisher_ids_match_windows() {
        // Every inbox Microsoft package ends with this id.
        assert_eq!(
            publisher_id(
                "CN=Microsoft Corporation, O=Microsoft Corporation, L=Redmond, S=Washington, C=US"
            ),
            "8wekyb3d8bbwe"
        );
        // Canapé's own identity package, as registered on the Ally.
        assert_eq!(
            family_name("Canape", "CN=Canape Dev"),
            "Canape_84zrj1c9kn010"
        );
    }

    #[test]
    fn gaming_root_lists_folders() {
        let mut bytes = b"RGBX".to_vec();
        bytes.extend(1u32.to_le_bytes());
        for unit in "\\XboxGames\0".encode_utf16() {
            bytes.extend(unit.to_le_bytes());
        }
        assert_eq!(parse_gaming_root(&bytes), ["XboxGames"]);
        assert!(parse_gaming_root(b"").is_empty());
    }

    #[test]
    fn scan_fixture() {
        let games = scan(&fixture("xbox"));
        assert_eq!(games.len(), 2, "{games:?}");
        let (a, b) = (&games[0], &games[1]);
        assert_eq!(a.title, "Contoso Racing");
        assert_eq!(
            a.id,
            format!(
                "xbox:Contoso.Racing_{}",
                publisher_id("CN=Contoso, O=Contoso, C=US")
            )
        );
        assert_eq!(
            a.launch,
            Launch::Uri {
                uri: format!(
                    r"shell:AppsFolder\Contoso.Racing_{}!RacingApp",
                    publisher_id("CN=Contoso, O=Contoso, C=US")
                )
            }
        );
        // Scaled variant of the logo.
        assert!(
            a.cover
                .as_ref()
                .unwrap()
                .ends_with("Square480x480Logo.scale-200.png")
        );
        // ms-resource display name: the folder name; app id from the config.
        assert_eq!(b.title, "Fabrikam Quest");
        assert!(matches!(&b.launch, Launch::Uri { uri } if uri.ends_with("!QuestApp")));
        assert_eq!(b.cover, None);
    }
}
