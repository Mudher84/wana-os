use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode};
use std::thread::sleep;
use std::time::{Duration, Instant};
use wana_log::{error, info, warn, Subsystem};

const LOG: Subsystem = Subsystem::Init;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Restart {
    Never,
    OnFailure,
    Always,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Service {
    name: String,
    exec: PathBuf,
    args: Vec<String>,
    after: Vec<String>,
    restart: Restart,
    uid: u32,
    gid: u32,
}

fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

fn parse(path: &Path) -> Result<Service, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut name = None;
    let mut exec = None;
    let mut args = Vec::new();
    let mut after = Vec::new();
    let mut restart = Restart::Never;
    let mut uid = 0u32;
    let mut gid = 0u32;
    let mut seen = BTreeSet::new();

    for (line_no, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| format!("{}:{}: expected key=value", path.display(), line_no + 1))?;
        let (key, value) = (key.trim(), value.trim());
        if key != "arg" && !seen.insert(key.to_string()) {
            return Err(format!("{}:{}: duplicate {key}", path.display(), line_no + 1));
        }
        match key {
            "name" if valid_name(value) => name = Some(value.to_string()),
            "name" => return Err(format!("{}:{}: invalid service name", path.display(), line_no + 1)),
            "exec" => {
                let p = PathBuf::from(value);
                if !p.is_absolute() || value.split('/').any(|p| p == "..") {
                    return Err(format!("{}:{}: exec must be an absolute safe path", path.display(), line_no + 1));
                }
                exec = Some(p);
            }
            "arg" => args.push(value.to_string()),
            "after" => {
                after = value
                    .split(',')
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(|v| {
                        if valid_name(v) {
                            Ok(v.to_string())
                        } else {
                            Err(format!("{}:{}: invalid dependency {v:?}", path.display(), line_no + 1))
                        }
                    })
                    .collect::<Result<_, _>>()?;
            }
            "restart" => {
                restart = match value {
                    "never" => Restart::Never,
                    "on-failure" => Restart::OnFailure,
                    "always" => Restart::Always,
                    _ => return Err(format!("{}:{}: invalid restart policy", path.display(), line_no + 1)),
                }
            }
            "uid" => uid = value.parse().map_err(|_| format!("{}:{}: invalid uid", path.display(), line_no + 1))?,
            "gid" => gid = value.parse().map_err(|_| format!("{}:{}: invalid gid", path.display(), line_no + 1))?,
            _ => return Err(format!("{}:{}: unknown field {key}", path.display(), line_no + 1)),
        }
    }

    Ok(Service {
        name: name.ok_or_else(|| format!("{}: missing name", path.display()))?,
        exec: exec.ok_or_else(|| format!("{}: missing exec", path.display()))?,
        args,
        after,
        restart,
        uid,
        gid,
    })
}

fn load(dir: &Path) -> Result<BTreeMap<String, Service>, String> {
    if !dir.exists() {
        return Ok(BTreeMap::new());
    }
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "service"))
        .collect();
    paths.sort();
    let mut out = BTreeMap::new();
    for p in paths {
        let s = parse(&p)?;
        let name = s.name.clone();
        if out.insert(name.clone(), s).is_some() {
            return Err(format!("duplicate service name {name}"));
        }
    }
    Ok(out)
}

fn order(map: &BTreeMap<String, Service>) -> Result<Vec<String>, String> {
    let mut indegree: BTreeMap<String, usize> =
        map.keys().map(|k| (k.clone(), 0usize)).collect();
    let mut edges: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (name, s) in map {
        for dep in &s.after {
            if !map.contains_key(dep) {
                return Err(format!("service {name}: missing dependency {dep}"));
            }
            *indegree.get_mut(name).expect("known service") += 1;
            edges.entry(dep.clone()).or_default().push(name.clone());
        }
    }
    let mut ready: BTreeSet<String> = indegree
        .iter()
        .filter_map(|(n, &d)| (d == 0).then_some(n.clone()))
        .collect();
    let mut out = Vec::new();
    while let Some(name) = ready.pop_first() {
        out.push(name.clone());
        for next in edges.get(&name).into_iter().flatten() {
            let d = indegree.get_mut(next).expect("known dependency target");
            *d -= 1;
            if *d == 0 {
                ready.insert(next.clone());
            }
        }
    }
    if out.len() != map.len() {
        return Err("service dependency cycle".into());
    }
    Ok(out)
}

fn spawn(s: &Service) -> Result<Child, String> {
    let mut cmd = Command::new(&s.exec);
    cmd.args(&s.args)
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .uid(s.uid)
        .gid(s.gid)
        .current_dir("/");
    let child = cmd
        .spawn()
        .map_err(|e| format!("{}: {}: {e}", s.name, s.exec.display()))?;
    info!(
        LOG,
        "service {} started pid={} uid={} gid={} restart={:?}",
        s.name,
        child.id(),
        s.uid,
        s.gid,
        s.restart
    );
    Ok(child)
}

fn supervise(map: BTreeMap<String, Service>, timeout: Option<Duration>) -> Result<(), String> {
    let sequence = order(&map)?;
    let mut children: BTreeMap<String, Child> = BTreeMap::new();
    for name in &sequence {
        children.insert(name.clone(), spawn(&map[name])?);
    }
    info!(LOG, "services ready: {} service(s)", children.len());
    let deadline = timeout.map(|d| Instant::now() + d);

    loop {
        if deadline.is_some_and(|d| Instant::now() >= d) {
            info!(LOG, "service supervisor test timeout reached");
            return Ok(());
        }
        for name in &sequence {
            let Some(child) = children.get_mut(name) else {
                continue;
            };
            let Some(status) = child.try_wait().map_err(|e| format!("{name}: wait: {e}"))? else {
                continue;
            };
            let policy = map[name].restart;
            let again = policy == Restart::Always || (policy == Restart::OnFailure && !status.success());
            if again {
                warn!(LOG, "service {name} exited {status}; restarting");
                sleep(Duration::from_millis(250));
                children.insert(name.clone(), spawn(&map[name])?);
            } else {
                info!(LOG, "service {name} exited {status}; not restarting");
                children.remove(name);
            }
        }
        sleep(Duration::from_millis(100));
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "run".into());
    let dir = PathBuf::from(args.next().unwrap_or_else(|| "/etc/wana/services.d".into()));
    let map = load(&dir)?;
    let sequence = order(&map)?;
    match command.as_str() {
        "check" => {
            info!(LOG, "service configuration PASS: {} service(s)", map.len());
            for name in sequence {
                info!(LOG, "service order: {name}");
            }
            Ok(())
        }
        "run" => {
            let timeout = args
                .next()
                .map(|v| v.parse::<u64>().map(Duration::from_secs))
                .transpose()
                .map_err(|e| format!("timeout: {e}"))?;
            supervise(map, timeout)
        }
        _ => Err(format!("unknown command {command}")),
    }
}

fn main() -> ExitCode {
    wana_log::init_from_env();
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!(LOG, "services: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static N: AtomicUsize = AtomicUsize::new(0);
    fn dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "wana-services-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn dependencies_are_topologically_ordered() {
        let d = dir();
        fs::write(d.join("a.service"), "name=a\nexec=/bin/true\nrestart=never\n").unwrap();
        fs::write(d.join("b.service"), "name=b\nexec=/bin/true\nafter=a\nuid=10\ngid=20\n").unwrap();
        let map = load(&d).unwrap();
        assert_eq!(order(&map).unwrap(), ["a", "b"]);
        assert_eq!(map["b"].uid, 10);
        assert_eq!(map["b"].gid, 20);
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn cycles_missing_dependencies_and_relative_exec_are_rejected() {
        let d = dir();
        fs::write(d.join("bad.service"), "name=bad\nexec=relative\n").unwrap();
        assert!(load(&d).is_err());
        fs::remove_file(d.join("bad.service")).unwrap();
        fs::write(d.join("a.service"), "name=a\nexec=/bin/true\nafter=b\n").unwrap();
        fs::write(d.join("b.service"), "name=b\nexec=/bin/true\nafter=a\n").unwrap();
        assert!(order(&load(&d).unwrap()).is_err());
        let _ = fs::remove_dir_all(d);
    }
}
