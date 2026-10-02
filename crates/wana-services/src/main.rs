use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode};
use std::thread::sleep;
use std::time::{Duration, Instant};
use wana_log::{error, info, warn, Subsystem};

const LOG: Subsystem = Subsystem::Init;
const READY_MARKER: &str = "/run/wana/services.ready";

extern "C" {
    fn setgroups(size: usize, list: *const u32) -> i32;
    fn setgid(gid: u32) -> i32;
    fn setuid(uid: u32) -> i32;
}

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
    home: Option<PathBuf>,
    env: Vec<(String, String)>,
    ready_path: Option<PathBuf>,
}

fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

fn valid_env_key(s: &str) -> bool {
    let mut bytes = s.bytes();
    matches!(bytes.next(), Some(b'A'..=b'Z' | b'_'))
        && bytes.all(|b| matches!(b, b'A'..=b'Z' | b'0'..=b'9' | b'_'))
}

fn effective_uid() -> Result<u32, String> {
    let status = fs::read_to_string("/proc/self/status")
        .map_err(|e| format!("read /proc/self/status: {e}"))?;
    let line = status
        .lines()
        .find(|line| line.starts_with("Uid:"))
        .ok_or("/proc/self/status: Uid field missing")?;
    line.split_whitespace()
        .nth(2)
        .ok_or("/proc/self/status: effective uid missing")?
        .parse()
        .map_err(|e| format!("/proc/self/status: invalid effective uid: {e}"))
}

fn secure_directory(path: &Path) -> Result<(), String> {
    let meta = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(format!(
            "{}: service directory must be a real directory",
            path.display()
        ));
    }
    let euid = effective_uid()?;
    if meta.uid() != euid {
        return Err(format!(
            "{}: service directory owner uid {} does not match effective uid {euid}",
            path.display(),
            meta.uid()
        ));
    }
    if meta.mode() & 0o022 != 0 {
        return Err(format!(
            "{}: service directory is group/other writable (mode {:o})",
            path.display(),
            meta.mode() & 0o777
        ));
    }
    Ok(())
}

fn read_secure_service(path: &Path) -> Result<String, String> {
    let before = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if before.file_type().is_symlink() || !before.is_file() {
        return Err(format!(
            "{}: service config must be a regular non-symlink file",
            path.display()
        ));
    }
    let euid = effective_uid()?;
    if before.uid() != euid {
        return Err(format!(
            "{}: service config owner uid {} does not match effective uid {euid}",
            path.display(),
            before.uid()
        ));
    }
    if before.mode() & 0o022 != 0 {
        return Err(format!(
            "{}: service config is group/other writable (mode {:o})",
            path.display(),
            before.mode() & 0o777
        ));
    }

    let mut file = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let opened = file
        .metadata()
        .map_err(|e| format!("metadata {}: {e}", path.display()))?;
    if before.dev() != opened.dev() || before.ino() != opened.ino() {
        return Err(format!(
            "{}: service config changed while opening",
            path.display()
        ));
    }

    let mut text = String::new();
    file.read_to_string(&mut text)
        .map_err(|e| format!("read {}: {e}", path.display()))?;
    Ok(text)
}

fn parse(path: &Path) -> Result<Service, String> {
    let text = read_secure_service(path)?;
    let mut name = None;
    let mut exec = None;
    let mut args = Vec::new();
    let mut after = Vec::new();
    let mut restart = Restart::Never;
    let mut uid = 0u32;
    let mut gid = 0u32;
    let mut home = None;
    let mut env = Vec::new();
    let mut ready_path = None;
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
        if key != "arg" && key != "env" && !seen.insert(key.to_string()) {
            return Err(format!(
                "{}:{}: duplicate {key}",
                path.display(),
                line_no + 1
            ));
        }
        match key {
            "name" if valid_name(value) => name = Some(value.to_string()),
            "name" => {
                return Err(format!(
                    "{}:{}: invalid service name",
                    path.display(),
                    line_no + 1
                ))
            }
            "exec" => {
                let p = PathBuf::from(value);
                if !p.is_absolute() || value.split('/').any(|p| p == "..") {
                    return Err(format!(
                        "{}:{}: exec must be an absolute safe path",
                        path.display(),
                        line_no + 1
                    ));
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
                            Err(format!(
                                "{}:{}: invalid dependency {v:?}",
                                path.display(),
                                line_no + 1
                            ))
                        }
                    })
                    .collect::<Result<_, _>>()?;
            }
            "restart" => {
                restart = match value {
                    "never" => Restart::Never,
                    "on-failure" => Restart::OnFailure,
                    "always" => Restart::Always,
                    _ => {
                        return Err(format!(
                            "{}:{}: invalid restart policy",
                            path.display(),
                            line_no + 1
                        ))
                    }
                }
            }
            "uid" => {
                uid = value
                    .parse()
                    .map_err(|_| format!("{}:{}: invalid uid", path.display(), line_no + 1))?
            }
            "gid" => {
                gid = value
                    .parse()
                    .map_err(|_| format!("{}:{}: invalid gid", path.display(), line_no + 1))?
            }
            "home" => {
                let p = PathBuf::from(value);
                if !p.is_absolute() || value.split('/').any(|part| part == "..") {
                    return Err(format!(
                        "{}:{}: home must be an absolute safe path",
                        path.display(),
                        line_no + 1
                    ));
                }
                home = Some(p);
            }
            "ready_path" => {
                let p = PathBuf::from(value);
                if !p.is_absolute()
                    || value.split('/').any(|part| part == "..")
                    || !p.starts_with("/run")
                    || p == Path::new("/run")
                {
                    return Err(format!(
                        "{}:{}: ready_path must be a safe transient path under /run",
                        path.display(),
                        line_no + 1
                    ));
                }
                ready_path = Some(p);
            }
            "env" => {
                let (name, val) = value.split_once('=').ok_or_else(|| {
                    format!("{}:{}: env must be NAME=VALUE", path.display(), line_no + 1)
                })?;
                if !valid_env_key(name) || val.contains('\0') {
                    return Err(format!(
                        "{}:{}: invalid environment entry",
                        path.display(),
                        line_no + 1
                    ));
                }
                if env.iter().any(|(existing, _)| existing == name) {
                    return Err(format!(
                        "{}:{}: duplicate environment key {name}",
                        path.display(),
                        line_no + 1
                    ));
                }
                env.push((name.to_string(), val.to_string()));
            }
            _ => {
                return Err(format!(
                    "{}:{}: unknown field {key}",
                    path.display(),
                    line_no + 1
                ))
            }
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
        home,
        env,
        ready_path,
    })
}

fn load(dir: &Path) -> Result<BTreeMap<String, Service>, String> {
    match fs::symlink_metadata(dir) {
        Ok(_) => secure_directory(dir)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(e) => return Err(format!("{}: {e}", dir.display())),
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
    let mut indegree: BTreeMap<String, usize> = map.keys().map(|k| (k.clone(), 0usize)).collect();
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
        .current_dir("/");
    if let Some(home) = &s.home {
        cmd.env("HOME", home);
    }
    for (name, value) in &s.env {
        cmd.env(name, value);
    }
    let (uid, gid) = (s.uid, s.gid);
    // SAFETY: this runs in the child after fork and before exec. Drop all
    // inherited supplementary groups before changing gid/uid so a non-root
    // service cannot retain root group access accidentally.
    unsafe {
        cmd.pre_exec(move || {
            if setgroups(0, std::ptr::null()) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if setgid(gid) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if setuid(uid) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
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

fn clear_stale_ready_path(service: &Service) -> Result<(), String> {
    let Some(path) = &service.ready_path else {
        return Ok(());
    };
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            if meta.file_type().is_symlink() || meta.is_dir() {
                return Err(format!(
                    "service {} readiness path {} is unsafe to replace",
                    service.name,
                    path.display()
                ));
            }
            fs::remove_file(path).map_err(|e| {
                format!(
                    "service {} remove stale readiness path {}: {e}",
                    service.name,
                    path.display()
                )
            })?;
            info!(
                LOG,
                "service {} removed stale readiness path {}",
                service.name,
                path.display()
            );
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!(
            "service {} inspect readiness path {}: {e}",
            service.name,
            path.display()
        )),
    }
}

fn wait_ready(service: &Service, child: &mut Child) -> Result<(), String> {
    let Some(path) = &service.ready_path else {
        return Ok(());
    };
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if path.exists() {
            info!(
                LOG,
                "service {} ready: {}",
                service.name,
                path.display()
            );
            return Ok(());
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|e| format!("{}: readiness wait: {e}", service.name))?
        {
            return Err(format!(
                "service {} exited {status} before readiness path {} appeared",
                service.name,
                path.display()
            ));
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "service {} readiness timeout waiting for {}",
                service.name,
                path.display()
            ));
        }
        sleep(Duration::from_millis(50));
    }
}

fn supervise(map: BTreeMap<String, Service>, timeout: Option<Duration>) -> Result<(), String> {
    let sequence = order(&map)?;
    let mut children: BTreeMap<String, Child> = BTreeMap::new();
    for name in &sequence {
        clear_stale_ready_path(&map[name])?;
        let mut child = spawn(&map[name])?;
        wait_ready(&map[name], &mut child)?;
        children.insert(name.clone(), child);
    }
    fs::create_dir_all("/run/wana").map_err(|e| format!("create /run/wana: {e}"))?;
    fs::write(READY_MARKER, format!("services={}\n", children.len()))
        .map_err(|e| format!("write {READY_MARKER}: {e}"))?;
    info!(LOG, "services ready: {} service(s); marker={READY_MARKER}", children.len());
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
            let again =
                policy == Restart::Always || (policy == Restart::OnFailure && !status.success());
            if again {
                warn!(LOG, "service {name} exited {status}; restarting");
                sleep(Duration::from_millis(250));
                clear_stale_ready_path(&map[name])?;
                let mut replacement = spawn(&map[name])?;
                wait_ready(&map[name], &mut replacement)?;
                children.insert(name.clone(), replacement);
            } else {
                info!(LOG, "service {name} exited {status}; not restarting");
                children.remove(name);
            }
        }
        sleep(Duration::from_millis(100));
    }
}

fn run() -> Result<(), String> {
    match fs::remove_file(READY_MARKER) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("remove stale {READY_MARKER}: {e}")),
    }
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
        fs::write(
            d.join("a.service"),
            "name=a\nexec=/bin/true\nrestart=never\n",
        )
        .unwrap();
        fs::write(
            d.join("b.service"),
            "name=b\nexec=/bin/true\nafter=a\nuid=10\ngid=20\nhome=/tmp\nready_path=/run/user/10/bus\nenv=XDG_RUNTIME_DIR=/run/user/10\nenv=DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/10/bus\n",
        )
        .unwrap();
        let map = load(&d).unwrap();
        assert_eq!(order(&map).unwrap(), ["a", "b"]);
        assert_eq!(map["b"].uid, 10);
        assert_eq!(map["b"].gid, 20);
        assert_eq!(map["b"].home, Some(PathBuf::from("/tmp")));
        assert_eq!(map["b"].ready_path, Some(PathBuf::from("/run/user/10/bus")));
        assert_eq!(
            map["b"].env,
            [
                ("XDG_RUNTIME_DIR".into(), "/run/user/10".into()),
                (
                    "DBUS_SESSION_BUS_ADDRESS".into(),
                    "unix:path=/run/user/10/bus".into()
                ),
            ]
        );
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn insecure_service_files_are_rejected() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let d = dir();
        let service = d.join("bad.service");
        fs::write(&service, "name=bad\nexec=/bin/true\n").unwrap();
        fs::set_permissions(&service, fs::Permissions::from_mode(0o666)).unwrap();
        assert!(load(&d).is_err());

        fs::remove_file(&service).unwrap();
        symlink("/etc/passwd", &service).unwrap();
        assert!(load(&d).is_err());

        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn readiness_paths_must_be_transient_runtime_paths() {
        let d = dir();
        fs::write(
            d.join("bad-ready.service"),
            "name=bad-ready\nexec=/bin/true\nready_path=/tmp/service.ready\n",
        )
        .unwrap();
        assert!(load(&d).is_err());
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn cycles_missing_dependencies_and_relative_exec_are_rejected() {
        let d = dir();
        fs::write(d.join("bad.service"), "name=bad\nexec=relative\n").unwrap();
        assert!(load(&d).is_err());
        fs::remove_file(d.join("bad.service")).unwrap();
        fs::write(
            d.join("bad.service"),
            "name=bad\nexec=/bin/true\nenv=bad-name=value\n",
        )
        .unwrap();
        assert!(load(&d).is_err());
        fs::remove_file(d.join("bad.service")).unwrap();
        fs::write(d.join("a.service"), "name=a\nexec=/bin/true\nafter=b\n").unwrap();
        fs::write(d.join("b.service"), "name=b\nexec=/bin/true\nafter=a\n").unwrap();
        assert!(order(&load(&d).unwrap()).is_err());
        let _ = fs::remove_dir_all(d);
    }
}
