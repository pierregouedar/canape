//! Who may open the pipe: the pure parts (package family name, SDDL), tested
//! on any OS. Windows turns the family name into an AppContainer SID in
//! `windows.rs`.

/// The Game Bar widget's package identity (`widget/CanapeWidget/Package.appxmanifest`).
pub const WIDGET_PACKAGE_NAME: &str = "Canape.Widget";
pub const WIDGET_PUBLISHER: &str = "CN=Canape Dev";

/// Full control for SYSTEM and administrators; read/write for interactive
/// users (the FSE shell) and for the AppContainer `app` (the Game Bar
/// widget: its package SID, or `AC`, every AppContainer, as a fallback). An
/// AppContainer passes the check only when both its user (`IU`) and its
/// package SID are granted. The low-integrity label lets the widget, which
/// opens the pipe with `CreateFileFromAppW`, write to it. Clients cannot
/// create pipe instances, so no one else can serve this name.
pub fn pipe_sddl(app: &str) -> String {
    format!("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)(A;;GRGW;;;{app})S:(ML;;NW;;;LW)")
}

/// `<name>_<publisher id>`, the name Windows derives the package's
/// AppContainer SID from.
pub fn package_family_name(name: &str, publisher: &str) -> String {
    format!("{name}_{}", publisher_id(publisher))
}

/// The 13-character publisher hash of a package family name: SHA-256 of
/// the publisher in UTF-16LE, first 8 bytes, read as 65 bits (one zero bit
/// appended) in Crockford base32, lower case.
pub fn publisher_id(publisher: &str) -> String {
    const ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";
    let utf16: Vec<u8> = publisher
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let digest = sha256(&utf16);
    let bits = u64::from_be_bytes(digest[..8].try_into().expect("8 bytes")) as u128;
    let bits = bits << 1;
    (0..13)
        .map(|i| ALPHABET[((bits >> (5 * (12 - i))) & 31) as usize] as char)
        .collect()
}

/// SHA-256 (FIPS 180-4), small and slow: it hashes one publisher name at
/// service startup.
fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for block in message.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes(word.try_into().expect("4 bytes"));
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (state, value) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *state = state.wrapping_add(value);
        }
    }
    let mut digest = [0u8; 32];
    for (bytes, word) in digest.chunks_exact_mut(4).zip(h) {
        bytes.copy_from_slice(&word.to_be_bytes());
    }
    digest
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn sha256_known_answers() {
        assert_eq!(
            hex(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // Two blocks.
        assert_eq!(
            hex(&sha256(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    /// The family names observed on the Ally: `Canape_84zrj1c9kn010` (the
    /// shell's identity, docs/m2-report.md) and `Canape.Widget_84zrj1c9kn010`.
    #[test]
    fn widget_family_name_matches_the_ally() {
        assert_eq!(publisher_id(WIDGET_PUBLISHER), "84zrj1c9kn010");
        assert_eq!(
            package_family_name(WIDGET_PACKAGE_NAME, WIDGET_PUBLISHER),
            "Canape.Widget_84zrj1c9kn010"
        );
    }

    #[test]
    fn sddl_names_the_app() {
        let sid = "S-1-15-2-1-2-3-4-5-6-7";
        assert_eq!(
            pipe_sddl(sid),
            "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)(A;;GRGW;;;S-1-15-2-1-2-3-4-5-6-7)S:(ML;;NW;;;LW)"
        );
        assert!(pipe_sddl("AC").contains("(A;;GRGW;;;AC)"));
    }
}
