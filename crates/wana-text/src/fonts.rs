//! The font directory: `SHA256SUMS` lists every font Wana's UI may load,
//! and each file must match it. A font that is missing, changed, or not
//! listed is refused (fonts are untrusted input to complex parsers; only the
//! pinned files shipped in the image are used).

use crate::sha256;
use std::path::{Path, PathBuf};

/// Where the image installs the fonts (package wana-fonts).
pub const DEFAULT_DIR: &str = "/usr/share/fonts/wana";

/// One `SHA256SUMS` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub sha256: String,
}

/// Parses `sha256sum` output: `<64 hex>  <name>` (or ` *<name>`). Names
/// must be plain file names (no directories).
pub fn parse_sums(text: &str) -> Result<Vec<Entry>, String> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let bad = || format!("SHA256SUMS line {}: {line:?}", n + 1);
        let (hash, rest) = line.split_once(' ').ok_or_else(bad)?;
        let name = rest.trim_start_matches([' ', '*']);
        if hash.len() != 64
            || !hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || name.is_empty()
            || name.contains('/')
            || name.starts_with('.')
        {
            return Err(bad());
        }
        out.push(Entry {
            name: name.to_owned(),
            sha256: hash.to_owned(),
        });
    }
    if out.is_empty() {
        return Err("SHA256SUMS lists no fonts".into());
    }
    Ok(out)
}

/// A font file that matched its listed hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified {
    pub path: PathBuf,
    pub sha256: String,
    pub bytes: usize,
}

/// Checks every file listed in `dir/SHA256SUMS`. All must match.
pub fn verify_dir(dir: &Path) -> Result<Vec<Verified>, String> {
    let sums = dir.join("SHA256SUMS");
    let text = std::fs::read_to_string(&sums).map_err(|e| format!("{}: {e}", sums.display()))?;
    let mut out = Vec::new();
    for e in parse_sums(&text)? {
        let path = dir.join(&e.name);
        let data = std::fs::read(&path).map_err(|err| format!("{}: {err}", path.display()))?;
        let got = sha256::hex(&sha256::digest(&data));
        if got != e.sha256 {
            return Err(format!(
                "{}: sha256 {got}, SHA256SUMS says {} (refusing a changed font)",
                path.display(),
                e.sha256
            ));
        }
        out.push(Verified {
            path,
            sha256: got,
            bytes: data.len(),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: &str = "02d9310b6b55b3bf8a5084fced9106ccd914650d730cbe8ff3b57f691d2931f6";

    #[test]
    fn sums_are_parsed_strictly() {
        let e = parse_sums(&format!("{H}  A.ttf\n{H} *B.ttf\n\n")).unwrap();
        assert_eq!(e.len(), 2);
        assert_eq!((e[0].name.as_str(), e[1].name.as_str()), ("A.ttf", "B.ttf"));
        for bad in [
            format!("{H}  ../etc/passwd"),
            format!("{H}  .hidden"),
            format!("{}  A.ttf", &H[1..]),
            format!("{}  A.ttf", H.to_uppercase()),
            "nothing".to_string(),
            String::new(),
        ] {
            assert!(parse_sums(&bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn changed_file_is_refused() {
        let dir = std::env::temp_dir().join(format!("wana-text-sums-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("f.ttf"), b"abc").unwrap();
        let abc = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        std::fs::write(dir.join("SHA256SUMS"), format!("{abc}  f.ttf\n")).unwrap();
        let ok = verify_dir(&dir).unwrap();
        assert_eq!((ok[0].bytes, ok[0].sha256.as_str()), (3, abc));
        std::fs::write(dir.join("f.ttf"), b"abd").unwrap();
        let err = verify_dir(&dir).unwrap_err();
        assert!(err.contains("refusing a changed font"), "{err}");
        std::fs::remove_file(dir.join("f.ttf")).unwrap();
        assert!(verify_dir(&dir).is_err(), "missing file");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
