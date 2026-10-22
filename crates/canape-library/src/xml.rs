//! Just enough XML for app manifests: the attributes of the first element
//! with a given name. Namespaces prefixes are ignored (`uap:VisualElements`
//! matches `VisualElements`), comments skipped, entities decoded.

/// Attributes (name, value) of the first `<name …>` element in `text`.
pub fn attributes(text: &str, name: &str) -> Option<Vec<(String, String)>> {
    let mut rest = text;
    while let Some(at) = rest.find('<') {
        rest = &rest[at + 1..];
        if let Some(after) = rest.strip_prefix("!--") {
            rest = after.split_once("-->").map_or("", |(_, r)| r);
            continue;
        }
        let tag_end = rest
            .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
            .unwrap_or(rest.len());
        let tag = &rest[..tag_end];
        let local = tag.rsplit(':').next().unwrap_or(tag);
        if local != name {
            continue;
        }
        return Some(parse_attributes(&rest[tag_end..]));
    }
    None
}

/// Value of attribute `attr` of the first `<name …>` element.
pub fn attribute(text: &str, name: &str, attr: &str) -> Option<String> {
    attributes(text, name)?
        .into_iter()
        .find(|(k, _)| k.rsplit(':').next() == Some(attr))
        .map(|(_, v)| v)
}

fn parse_attributes(mut s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    loop {
        s = s.trim_start();
        if s.is_empty() || s.starts_with('>') || s.starts_with('/') {
            return out;
        }
        let Some(eq) = s.find('=') else { return out };
        let key = s[..eq].trim().to_owned();
        s = s[eq + 1..].trim_start();
        let Some(quote) = s.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            return out;
        };
        let Some(end) = s[1..].find(quote) else {
            return out;
        };
        out.push((key, decode(&s[1..1 + end])));
        s = &s[end + 2..];
    }
}

fn decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest.find(';') else { break };
        let entity = &rest[1..end];
        let c = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match c {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_element_attributes() {
        let text = r#"<?xml version="1.0"?>
            <!-- <Identity Name="Commented"/> -->
            <Package xmlns:uap="x"><Identity Name="A&amp;B" Publisher='CN=X, O=&quot;Y&quot;' Version="1.0.0.0"/>
            <uap:VisualElements DisplayName="Caf&#233; &#x2122;">"#;
        assert_eq!(attribute(text, "Identity", "Name").as_deref(), Some("A&B"));
        assert_eq!(
            attribute(text, "Identity", "Publisher").as_deref(),
            Some(r#"CN=X, O="Y""#)
        );
        assert_eq!(
            attribute(text, "VisualElements", "DisplayName").as_deref(),
            Some("Café ™")
        );
        assert_eq!(attribute(text, "Identity", "Missing"), None);
        assert_eq!(attribute(text, "Nope", "Name"), None);
        // `Identity` must not match `IdentityExtra`.
        assert_eq!(
            attribute("<IdentityExtra Name=\"x\"/>", "Identity", "Name"),
            None
        );
        assert_eq!(decode("a & b &bogus; c"), "a & b &bogus; c");
    }
}
