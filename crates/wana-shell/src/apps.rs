//! The apps the launcher lists: a pinned file, one app per line, fields
//! separated by a TAB (names may contain spaces, arguments may not need
//! quoting): `name<TAB>/absolute/program<TAB>argument...`. Empty lines and
//! lines starting with `#` are skipped.

use std::path::Path;

/// Where the image installs the list.
pub const DEFAULT_FILE: &str = "/etc/wana/apps";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    pub name: String,
    /// Program (absolute path) and arguments.
    pub argv: Vec<String>,
}

pub fn parse(text: &str) -> Result<Vec<App>, String> {
    let mut apps = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.split('\t');
        let name = fields.next().unwrap_or("").trim();
        let argv: Vec<String> = fields.map(str::to_string).collect();
        let bad = |why: &str| Err(format!("line {}: {why}", n + 1));
        if name.is_empty() {
            return bad("empty name");
        }
        match argv.first() {
            None => return bad("no program (fields are separated by a TAB)"),
            Some(p) if !p.starts_with('/') => return bad("the program needs an absolute path"),
            _ => {}
        }
        if argv.iter().any(String::is_empty) {
            return bad("empty argument (two TABs in a row?)");
        }
        apps.push(App {
            name: name.to_string(),
            argv,
        });
    }
    Ok(apps)
}

pub fn load(path: &Path) -> Result<Vec<App>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse(&text).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_names_with_spaces_and_arguments() {
        let apps = parse("# comment\n\nنافذة تجريبية\t/usr/bin/a\t--hold\t3\nب\t/bin/b\n").unwrap();
        assert_eq!(
            apps,
            vec![
                App {
                    name: "نافذة تجريبية".into(),
                    argv: vec!["/usr/bin/a".into(), "--hold".into(), "3".into()],
                },
                App {
                    name: "ب".into(),
                    argv: vec!["/bin/b".into()],
                },
            ]
        );
    }

    #[test]
    fn rejects_bad_lines_with_their_number() {
        assert_eq!(
            parse("x /bin/a").unwrap_err(),
            "line 1: no program (fields are separated by a TAB)"
        );
        assert!(parse("#\nx\tbin/a")
            .unwrap_err()
            .starts_with("line 2: the program needs"));
        assert!(parse("\t/bin/a").is_err());
        assert!(parse("x\t/bin/a\t\t1").is_err());
    }

    #[test]
    fn the_shipped_lists_parse() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("data");
        for f in ["apps", "apps.test"] {
            let apps = load(&dir.join(f)).unwrap();
            assert!(apps.len() >= 2, "{f}");
        }
    }
}
