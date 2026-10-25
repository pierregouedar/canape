//! Steam's cache of app information, `appcache/appinfo.vdf`: one binary
//! KeyValues block per app the client knows (the owned ones among them),
//! with its name (`common/name`) and install folder (`config/installdir`).
//!
//! Layout (little-endian): magic, universe, and from the `0x07564429`
//! format on the offset of a string table that holds every key; then per
//! app its id, the size of the rest of its entry, 60 bytes of state and
//! hashes, and the binary KeyValues. An app id of 0 ends the list. Only the
//! wanted entry is read: the others are skipped by their size.

use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use crate::vdf::Value;

/// Keys written inline, before the string table.
const WITH_INLINE_KEYS: u32 = 0x0756_4428;
/// Keys as indexes into the string table.
const WITH_STRING_TABLE: u32 = 0x0756_4429;
/// Info state, last update, access token, text hash, change number and
/// binary hash, before an entry's KeyValues.
const ENTRY_HEADER: usize = 4 + 4 + 8 + 20 + 4 + 20;
/// Nesting deeper than this is not appinfo.
const MAX_DEPTH: usize = 32;

/// What installing an app needs from its cached information.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    pub name: Option<String>,
    pub installdir: Option<String>,
}

/// The cached information of `appid` in the Steam folder `steam`, if Steam
/// has it in a format read here.
pub fn app(steam: &Path, appid: u32) -> Option<App> {
    let file = std::fs::File::open(steam.join("appcache").join("appinfo.vdf")).ok()?;
    let value = find(&mut BufReader::with_capacity(64 * 1024, file), appid).ok()??;
    let info = value.get("appinfo")?;
    let text = |path: &[&str]| match info.path(path)? {
        Value::Str(s) if !s.trim().is_empty() => Some(s.trim().to_owned()),
        _ => None,
    };
    Some(App {
        name: text(&["common", "name"]),
        installdir: text(&["config", "installdir"]),
    })
}

fn u32_le(r: &mut impl Read) -> io::Result<u32> {
    let mut b = [0; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

/// The KeyValues of `appid`, `None` when the cache does not hold it or is
/// in an unknown format.
pub fn find<R: BufRead + Seek>(r: &mut R, appid: u32) -> io::Result<Option<Value>> {
    let magic = u32_le(r)?;
    let _universe = u32_le(r)?;
    let table = match magic {
        WITH_INLINE_KEYS => None,
        WITH_STRING_TABLE => {
            let mut b = [0; 8];
            r.read_exact(&mut b)?;
            Some(u64::from_le_bytes(b))
        }
        _ => return Ok(None),
    };
    loop {
        let id = u32_le(r)?;
        if id == 0 {
            return Ok(None);
        }
        let size = u32_le(r)?;
        if id != appid {
            r.seek_relative(i64::from(size))?;
            continue;
        }
        let mut entry = vec![0; size as usize];
        r.read_exact(&mut entry)?;
        let keys = match table {
            Some(offset) => {
                r.seek(SeekFrom::Start(offset))?;
                Some(strings(r)?)
            }
            None => None,
        };
        let Some(mut data) = entry.get(ENTRY_HEADER..) else {
            return Ok(None);
        };
        return Ok(object(&mut data, keys.as_deref(), 0).map(Value::Obj));
    }
}

/// The string table: a count, then that many NUL-terminated strings.
fn strings(r: &mut impl BufRead) -> io::Result<Vec<String>> {
    let count = u32_le(r)?;
    let mut out = Vec::with_capacity(count.min(1 << 20) as usize);
    let mut buf = Vec::new();
    for _ in 0..count {
        buf.clear();
        r.read_until(0, &mut buf)?;
        if buf.pop() != Some(0) {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        out.push(String::from_utf8_lossy(&buf).into_owned());
    }
    Ok(out)
}

fn take<'a>(data: &mut &'a [u8], n: usize) -> Option<&'a [u8]> {
    if data.len() < n {
        return None;
    }
    let (head, rest) = data.split_at(n);
    *data = rest;
    Some(head)
}

fn cstr(data: &mut &[u8]) -> Option<String> {
    let end = data.iter().position(|&b| b == 0)?;
    let text = String::from_utf8_lossy(&data[..end]).into_owned();
    *data = &data[end + 1..];
    Some(text)
}

fn number<const N: usize>(data: &mut &[u8]) -> Option<[u8; N]> {
    take(data, N)?.try_into().ok()
}

/// One binary KeyValues object, up to its end marker. Numbers become text,
/// as in the text format.
fn object(data: &mut &[u8], keys: Option<&[String]>, depth: usize) -> Option<Vec<(String, Value)>> {
    if depth > MAX_DEPTH {
        return None;
    }
    let mut entries = Vec::new();
    loop {
        let kind = *take(data, 1)?.first()?;
        // End of object: 8, or 11 in some writers.
        if kind == 8 || kind == 11 {
            return Some(entries);
        }
        let key = match keys {
            Some(keys) => keys
                .get(u32::from_le_bytes(number(data)?) as usize)?
                .clone(),
            None => cstr(data)?,
        };
        let value = match kind {
            0 => Value::Obj(object(data, keys, depth + 1)?),
            1 => Value::Str(cstr(data)?),
            // Int32, pointer, color.
            2 | 4 | 6 => Value::Str(i32::from_le_bytes(number(data)?).to_string()),
            3 => Value::Str(f32::from_le_bytes(number(data)?).to_string()),
            7 => Value::Str(u64::from_le_bytes(number(data)?).to_string()),
            10 => Value::Str(i64::from_le_bytes(number(data)?).to_string()),
            _ => return None,
        };
        entries.push((key, value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Binary KeyValues of an app, keys inline or as table indexes.
    fn kv(appid: u32, name: &str, dir: &str, table: Option<&mut Vec<String>>) -> Vec<u8> {
        let mut out = Vec::new();
        let mut table = table;
        let mut key = |out: &mut Vec<u8>, k: &str| match table.as_deref_mut() {
            Some(t) => {
                let i = t.iter().position(|s| s == k).unwrap_or_else(|| {
                    t.push(k.to_owned());
                    t.len() - 1
                });
                out.extend((i as u32).to_le_bytes());
            }
            None => {
                out.extend(k.as_bytes());
                out.push(0);
            }
        };
        let text = |out: &mut Vec<u8>, v: &str| {
            out.extend(v.as_bytes());
            out.push(0);
        };
        out.push(0);
        key(&mut out, "appinfo");
        out.push(2);
        key(&mut out, "appid");
        out.extend(appid.to_le_bytes());
        out.push(0);
        key(&mut out, "common");
        out.push(1);
        key(&mut out, "name");
        text(&mut out, name);
        out.push(7);
        key(&mut out, "gameid");
        out.extend(u64::from(appid).to_le_bytes());
        out.push(8);
        out.push(0);
        key(&mut out, "config");
        out.push(1);
        key(&mut out, "installdir");
        text(&mut out, dir);
        out.push(8);
        out.push(8);
        out.push(8);
        out
    }

    fn file(magic: u32, apps: &[(u32, &str, &str)]) -> Vec<u8> {
        let mut table = Vec::new();
        let tabled = magic == WITH_STRING_TABLE;
        let mut out = Vec::new();
        out.extend(magic.to_le_bytes());
        out.extend(1u32.to_le_bytes());
        if tabled {
            out.extend(0u64.to_le_bytes());
        }
        for &(appid, name, dir) in apps {
            let body = kv(appid, name, dir, tabled.then_some(&mut table));
            out.extend(appid.to_le_bytes());
            out.extend(((ENTRY_HEADER + body.len()) as u32).to_le_bytes());
            out.extend([0xAB; ENTRY_HEADER]);
            out.extend(body);
        }
        out.extend(0u32.to_le_bytes());
        if tabled {
            let offset = out.len() as u64;
            out[8..16].copy_from_slice(&offset.to_le_bytes());
            out.extend((table.len() as u32).to_le_bytes());
            for s in &table {
                out.extend(s.as_bytes());
                out.push(0);
            }
        }
        out
    }

    fn lookup(bytes: Vec<u8>, appid: u32) -> Option<(String, String)> {
        let value = find(&mut Cursor::new(bytes), appid).unwrap()?;
        let info = value.get("appinfo")?;
        Some((
            info.path(&["common"])?.str("name")?.to_owned(),
            info.path(&["config"])?.str("installdir")?.to_owned(),
        ))
    }

    #[test]
    fn finds_the_app_in_both_formats() {
        let apps = [
            (400, "Portal", "Portal"),
            (105600, "Terraria", "Terraria"),
            (620, "Portal 2", "Portal 2"),
        ];
        for magic in [WITH_INLINE_KEYS, WITH_STRING_TABLE] {
            let bytes = file(magic, &apps);
            assert_eq!(
                lookup(bytes.clone(), 105600),
                Some(("Terraria".into(), "Terraria".into()))
            );
            assert_eq!(
                lookup(bytes.clone(), 620),
                Some(("Portal 2".into(), "Portal 2".into()))
            );
            assert_eq!(lookup(bytes.clone(), 1), None);
            let value = find(&mut Cursor::new(bytes), 400).unwrap().unwrap();
            assert_eq!(
                value.path(&["appinfo", "common"]).unwrap().str("gameid"),
                Some("400")
            );
        }
    }

    #[test]
    fn unknown_or_broken_files() {
        let mut other = file(WITH_INLINE_KEYS, &[(400, "Portal", "Portal")]);
        other[0] = 0x27;
        assert_eq!(find(&mut Cursor::new(other), 400).unwrap(), None);
        let mut cut = file(WITH_INLINE_KEYS, &[(400, "Portal", "Portal")]);
        cut.truncate(cut.len() - 12);
        assert!(find(&mut Cursor::new(cut), 400).is_err());
        assert!(find(&mut Cursor::new(Vec::new()), 400).is_err());
    }
}
