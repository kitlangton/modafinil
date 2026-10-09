use crate::Result;
use fs2::FileExt;
use modafinil_cli::{
    Mode, Power, Settings, SleepControl, parse_disabled, parse_power, plist, policy,
    reconcile_sleep,
};
use serde::Serialize;
use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const LABEL: &str = "dev.kitlangton.modafinil";
const HELPER: &str = "dev.kitlangton.modafinil.lid";
const ROOT_DIR: &str = "/Library/Application Support/Modafinil";
const ROOT_PLIST: &str = "/Library/LaunchDaemons/dev.kitlangton.modafinil.lid.plist";
const PMSET: &str = "/usr/bin/pmset";
const LAUNCHCTL: &str = "/bin/launchctl";

/// Fixed system commands only. Never invoke a shell or resolve executables via PATH.
fn command(program: &str, args: &[&str]) -> Result<String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let read = |pipe: Box<dyn Read + Send>| {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            pipe.take(1024 * 1024)
                .read_to_end(&mut bytes)
                .map(|_| bytes)
        })
    };
    let out = read(Box::new(stdout));
    let err = read(Box::new(stderr));
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("{program} timed out after 5 seconds").into());
        }
        thread::sleep(Duration::from_millis(25));
    };
    let out = out.join().map_err(|_| "stdout reader failed")??;
    let err = err.join().map_err(|_| "stderr reader failed")??;
    if !status.success() {
        return Err(format!(
            "{program} failed ({status}): {}",
            String::from_utf8_lossy(&err).trim()
        )
        .into());
    }
    Ok(String::from_utf8(out)?)
}

fn home() -> Result<PathBuf> {
    Ok(PathBuf::from(
        std::env::var_os("HOME").ok_or("HOME is not set")?,
    ))
}
fn config_path() -> Result<PathBuf> {
    Ok(home()?.join(".config/modafinil/settings.json"))
}
fn user_binary() -> Result<PathBuf> {
    Ok(home()?.join(".local/share/modafinil/modafinil"))
}
fn user_plist() -> Result<PathBuf> {
    Ok(home()?.join(format!("Library/LaunchAgents/{LABEL}.plist")))
}
fn user_service() -> String {
    format!("gui/{}/{LABEL}", unsafe { libc::getuid() })
}
fn root_service() -> String {
    format!("system/{HELPER}")
}
fn path_str(path: &Path) -> Result<&str> {
    path.to_str().ok_or_else(|| "path is not UTF-8".into())
}

fn require_user() -> Result {
    if unsafe { libc::geteuid() } == 0 {
        return Err("run this command as your normal login user, without sudo".into());
    }
    Ok(())
}
fn require_root() -> Result {
    if unsafe { libc::geteuid() } != 0 {
        return Err("this internal helper requires administrator access".into());
    }
    Ok(())
}

fn load(path: &Path) -> Result<Settings> {
    match fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(error) => Err(error.into()),
    }
}

fn save(path: &Path, bytes: &[u8], mode: u32) -> Result {
    let parent = path.parent().ok_or("path has no parent")?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    let result = (|| -> Result {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(&temporary)?;
        use std::io::Write;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn copy_binary(path: &Path) -> Result {
    let bytes = fs::read(std::env::current_exe()?)?;
    save(path, &bytes, 0o755)
}

fn loaded(service: &str) -> bool {
    command(LAUNCHCTL, &["print", service]).is_ok()
}
fn bootout(service: &str) -> Result {
    if loaded(service) {
        command(LAUNCHCTL, &["bootout", service])?;
    }
    Ok(())
}
fn bootstrap(domain: &str, path: &Path, service: &str) -> Result {
    command(LAUNCHCTL, &["bootstrap", domain, path_str(path)?])?;
    verify_running(service)?;
    Ok(())
}

fn verify_running(service: &str) -> Result {
    for _ in 0..10 {
        let state = command(LAUNCHCTL, &["print", service])?;
        if state.lines().any(|line| line.trim() == "state = running") {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(200));
    }
    Err(format!("{service} loaded but did not start; inspect its log").into())
}

fn lock(path: &Path) -> Result<File> {
    fs::create_dir_all(path.parent().ok_or("lock has no parent")?)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    file.try_lock_exclusive()
        .map_err(|_| "another Modafinil process already owns this service; stop it first")?;
    Ok(file)
}

fn shutdown_flag() -> Result<Arc<AtomicBool>> {
    let stop = Arc::new(AtomicBool::new(false));
    for signal in [
        signal_hook::consts::SIGINT,
        signal_hook::consts::SIGTERM,
        signal_hook::consts::SIGHUP,
    ] {
        signal_hook::flag::register(signal, Arc::clone(&stop))?;
    }
    Ok(stop)
}
fn wait(stop: &AtomicBool) {
    for _ in 0..20 {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
}
fn power() -> Result<Power> {
    Ok(parse_power(&command(PMSET, &["-g", "batt"])?))
}
fn disabled() -> Result<bool> {
    parse_disabled(&command(PMSET, &["-g"])?)
        .ok_or_else(|| "pmset did not report SleepDisabled".into())
}
fn set_disabled(value: bool) -> Result {
    command(
        PMSET,
        &["-a", "disablesleep", if value { "1" } else { "0" }],
    )?;
    if disabled()? != value {
        return Err("sleep setting did not match the requested value".into());
    }
    Ok(())
}

struct Assertion(Option<Child>);
impl Assertion {
    fn stop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
    fn update(&mut self, mode: Mode, awake: bool) -> Result {
        if self
            .0
            .as_mut()
            .is_some_and(|c| c.try_wait().ok().flatten().is_some())
        {
            self.0 = None;
        }
        if !awake {
            self.stop();
        } else if self.0.is_none() {
            // -w releases the assertion if our process is killed without cleanup.
            let pid = std::process::id().to_string();
            let mut args = vec!["-i", "-w", &pid];
            if mode == Mode::Ac {
                args.push("-s");
            }
            self.0 = Some(
                Command::new("/usr/bin/caffeinate")
                    .args(args)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()?,
            );
        }
        Ok(())
    }
}
impl Drop for Assertion {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn run(mode: Option<Mode>, seconds: Option<u64>, config: Option<PathBuf>) -> Result {
    require_user()?;
    let path = config.unwrap_or(config_path()?);
    let _lock = lock(&config_path()?.with_file_name("run.lock"))?;
    let stop = shutdown_flag()?;
    let until = seconds.map(|s| Instant::now() + Duration::from_secs(s));
    let mut assertion = Assertion(None);
    let mut previous = None;
    println!(
        "Running; Ctrl-C stops. Closed-lid support is managed separately by the installed helper."
    );
    while !stop.load(Ordering::Relaxed) && !until.is_some_and(|t| Instant::now() >= t) {
        let result = (|| -> Result<(Mode, bool)> {
            let mut settings = load(&path)?;
            if let Some(mode) = mode {
                settings.mode = mode;
            }
            let awake = policy(settings, power()?).awake;
            Ok((settings.mode, awake))
        })();
        match result {
            Ok((mode, awake)) => {
                // Restart on mode changes so AC-specific assertions cannot linger.
                if previous.is_some_and(|(old, _)| old != mode) {
                    assertion.stop();
                }
                assertion.update(mode, awake)?;
                if previous != Some((mode, awake)) {
                    println!("mode={} awake={awake}", mode.as_str());
                    previous = Some((mode, awake));
                }
            }
            Err(error) => {
                assertion.stop();
                previous = None;
                eprintln!("Check failed; assertion released: {error}");
            }
        }
        wait(&stop);
    }
    println!("Stopped; keep-awake assertion released.");
    Ok(())
}

#[derive(Serialize)]
struct Status {
    settings: Settings,
    power: String,
    awake_requested: bool,
    closed_lid_requested: bool,
    start_at_login: bool,
    login_service_loaded: bool,
    lid_service_loaded: bool,
    sleep_disabled: Option<bool>,
    legacy_helper_loaded: bool,
}
pub fn status(json: bool) -> Result {
    require_user()?;
    let settings = load(&config_path()?)?;
    let power = power()?;
    let policy = policy(settings, power);
    let status = Status {
        settings,
        power: format!("{power:?}").to_lowercase(),
        awake_requested: policy.awake,
        closed_lid_requested: policy.closed_lid,
        start_at_login: user_plist()?.exists(),
        login_service_loaded: loaded(&user_service()),
        lid_service_loaded: loaded(&root_service()),
        sleep_disabled: disabled().ok(),
        legacy_helper_loaded: loaded(&format!("gui/{}/local.kit.pro-awake", unsafe {
            libc::getuid()
        })),
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&status)?);
    } else {
        println!(
            "Mode: {}\nClosed lid: {} (AC only)\nPower: {}\nStart at login: {}\nLogin service loaded: {}\nLid service loaded: {}\nSystem sleep disabled: {}",
            status.settings.mode.as_str(),
            if settings.closed_lid { "on" } else { "off" },
            status.power,
            status.start_at_login,
            status.login_service_loaded,
            status.lid_service_loaded,
            status
                .sleep_disabled
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unknown".into())
        );
        if status.legacy_helper_loaded {
            println!(
                "Warning: legacy pro-awake helper is loaded. Do not enable this app's lid helper until it is removed."
            );
        }
    }
    Ok(())
}

fn admin(args: &[&str]) -> Result {
    // Interactive sudo, with no passwordless sudoers rule. The caller chooses this command.
    let status = Command::new("/usr/bin/sudo")
        .arg(std::env::current_exe()?)
        .args(args)
        .status()?;
    if !status.success() {
        return Err("administrator action failed; saved settings were not changed".into());
    }
    Ok(())
}

pub fn settings(mode: Option<Mode>, closed_lid: Option<bool>) -> Result {
    require_user()?;
    if mode.is_none() && closed_lid.is_none() {
        return Err("specify --mode and/or --closed-lid".into());
    }
    let mut settings = load(&config_path()?)?;
    if let Some(mode) = mode {
        settings.mode = mode;
    }
    if let Some(closed_lid) = closed_lid {
        settings.closed_lid = closed_lid;
    }
    let helper_exists = Path::new(ROOT_PLIST).exists();
    if settings.closed_lid && settings.mode != Mode::Off {
        if !helper_exists
            && (loaded(&format!("gui/{}/local.kit.pro-awake", unsafe {
                libc::getuid()
            })) || disabled()?)
        {
            return Err("another tool already disables sleep. Stop and remove that tool, restore `sudo pmset -a disablesleep 0`, then enable closed-lid support. Nothing changed.".into());
        }
        println!("Installing AC-only closed-lid helper (administrator approval required).");
        admin(&["helper-install", "--mode", settings.mode.as_str()])?;
    } else if helper_exists {
        println!(
            "Removing closed-lid helper and restoring system sleep (administrator approval required)."
        );
        admin(&["helper-remove"])?;
    }
    save(
        &config_path()?,
        &serde_json::to_vec_pretty(&settings)?,
        0o600,
    )?;
    println!(
        "Saved: mode={} closed-lid={}",
        settings.mode.as_str(),
        settings.closed_lid
    );
    Ok(())
}

pub fn install() -> Result {
    require_user()?;
    let settings = load(&config_path()?)?;
    if settings.closed_lid && settings.mode != Mode::Off && !Path::new(ROOT_PLIST).exists() {
        return Err("closed-lid support is saved but its helper is missing; run `modafinil set --closed-lid on` or `modafinil set --closed-lid off` first".into());
    }
    let binary = user_binary()?;
    let path = user_plist()?;
    let log = home()?.join("Library/Logs/modafinil.log");
    fs::create_dir_all(log.parent().unwrap())?;
    bootout(&user_service())?;
    copy_binary(&binary)?;
    let data = plist(
        LABEL,
        &[
            path_str(&binary)?.into(),
            "run".into(),
            "--config".into(),
            path_str(&config_path()?)?.into(),
        ],
        path_str(&log)?,
    );
    save(&path, data.as_bytes(), 0o644)?;
    let domain = format!("gui/{}", unsafe { libc::getuid() });
    bootstrap(&domain, &path, &user_service())?;
    println!(
        "Installed and started login service. Mode: {}. Log: {}",
        settings.mode.as_str(),
        log.display()
    );
    Ok(())
}
pub fn stop() -> Result {
    require_user()?;
    if Path::new(ROOT_PLIST).exists() {
        return Err("closed-lid helper is installed; use `modafinil set --mode off` to stop both modes safely".into());
    }
    bootout(&user_service())?;
    if loaded(&user_service()) {
        return Err("login service is still loaded".into());
    }
    println!("Login service stopped. Run `modafinil start` to resume.");
    Ok(())
}
pub fn start() -> Result {
    require_user()?;
    if !user_plist()?.exists() {
        return Err("run `modafinil install` first".into());
    }
    if !loaded(&user_service()) {
        bootstrap(
            &format!("gui/{}", unsafe { libc::getuid() }),
            &user_plist()?,
            &user_service(),
        )?;
    }
    verify_running(&user_service())?;
    println!("Login service loaded.");
    Ok(())
}
fn remove_if_exists(path: &Path) -> Result {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}
pub fn uninstall() -> Result {
    require_user()?;
    if Path::new(ROOT_PLIST).exists() {
        admin(&["helper-remove"])?;
    }
    stop()?;
    remove_if_exists(&user_plist()?)?;
    remove_if_exists(&user_binary()?)?;
    let mut settings = load(&config_path()?)?;
    settings.closed_lid = false;
    save(
        &config_path()?,
        &serde_json::to_vec_pretty(&settings)?,
        0o600,
    )?;
    println!(
        "Uninstalled services. Settings and logs retained. Remove the cargo binary separately with `cargo uninstall modafinil-cli`."
    );
    Ok(())
}

/// This daemon is installed with fixed arguments and a root-owned binary.
/// launchd restarts it after crashes; startup resets the persistent flag first.
pub fn lid_daemon(mode: Mode) -> Result {
    require_root()?;
    if std::env::current_exe()? != Path::new(ROOT_DIR).join("modafinil") {
        return Err("lid-daemon must run from the administrator-owned installed copy".into());
    }
    let _lock = lock(Path::new("/var/run/modafinil-lid.lock"))?;
    let stop = shutdown_flag()?;
    set_disabled(false)?;
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            if let Err(error) = set_disabled(false) {
                eprintln!("Final sleep restoration failed: {error}");
            }
        }
    }
    let _restore = Restore;
    struct SystemSleep;
    impl SleepControl for SystemSleep {
        fn disabled(&mut self) -> std::result::Result<bool, String> {
            disabled().map_err(|error| error.to_string())
        }
        fn set_disabled(&mut self, value: bool) -> std::result::Result<(), String> {
            set_disabled(value).map_err(|error| error.to_string())
        }
    }
    let settings = Settings {
        mode,
        closed_lid: true,
    };
    let mut previous = None;
    while !stop.load(Ordering::Relaxed) {
        let desired = match power() {
            Ok(power) => policy(settings, power).closed_lid,
            Err(error) => {
                eprintln!("Power check failed; restoring sleep: {error}");
                false
            }
        };
        let result = reconcile_sleep(&mut SystemSleep, desired);
        if let Err(error) = result {
            eprintln!("Sleep check failed: {error}");
            previous = None;
        } else if previous != Some(desired) {
            println!("closed-lid={desired} (AC only)");
            previous = Some(desired);
        }
        wait(&stop);
    }
    set_disabled(false)?;
    println!("Stopped; system sleep restored.");
    Ok(())
}

pub fn helper_install(mode: Mode) -> Result {
    require_root()?;
    if mode == Mode::Off {
        return Err("cannot install helper in off mode".into());
    }
    if !Path::new(ROOT_PLIST).exists() && disabled()? {
        return Err("another tool has disabled sleep; refusing to take over its setting".into());
    }
    // No user-controlled launchd paths, logs, commands, or configuration are read by root.
    let dir = Path::new(ROOT_DIR);
    if let Ok(metadata) = fs::symlink_metadata(dir) {
        use std::os::unix::fs::MetadataExt;
        if !metadata.is_dir() || metadata.uid() != 0 || metadata.permissions().mode() & 0o022 != 0 {
            return Err(
                "helper directory must be a real root-owned directory, not group/world writable"
                    .into(),
            );
        }
    }
    fs::create_dir_all(dir)?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o755))?;
    bootout(&root_service())?;
    set_disabled(false)?;
    copy_binary(&dir.join("modafinil"))?;
    let data = plist(
        HELPER,
        &[
            dir.join("modafinil").display().to_string(),
            "lid-daemon".into(),
            "--mode".into(),
            mode.as_str().into(),
        ],
        "/var/log/modafinil-lid.log",
    );
    save(Path::new(ROOT_PLIST), data.as_bytes(), 0o644)?;
    let result = bootstrap("system", Path::new(ROOT_PLIST), &root_service());
    if result.is_err() {
        let _ = bootout(&root_service());
        let _ = set_disabled(false);
    }
    result
}
pub fn helper_remove() -> Result {
    require_root()?;
    if !Path::new(ROOT_PLIST).exists() && !loaded(&root_service()) {
        return Err(
            "no Modafinil lid helper is installed; leaving system settings untouched".into(),
        );
    }
    bootout(&root_service())?;
    set_disabled(false)?;
    if loaded(&root_service()) {
        return Err("lid helper is still loaded; refusing removal".into());
    }
    remove_if_exists(Path::new(ROOT_PLIST))?;
    remove_if_exists(&Path::new(ROOT_DIR).join("modafinil"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    fn scratch() -> PathBuf {
        let path = std::env::current_dir()
            .unwrap()
            .join("target/test-scratch")
            .join(format!(
                "{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn bounded_command_checks_exit_status() {
        assert_eq!(command("/bin/echo", &["hello"]).unwrap().trim(), "hello");
        assert!(command("/usr/bin/false", &[]).is_err());
        assert!(command("/nonexistent-modafinil-command", &[]).is_err());
    }

    #[test]
    fn child_command_timeout_is_bounded() {
        let start = Instant::now();
        assert!(
            command("/bin/sleep", &["30"])
                .unwrap_err()
                .to_string()
                .contains("timed out")
        );
        assert!(start.elapsed() < Duration::from_secs(15));
    }

    #[test]
    fn lock_excludes_another_runner_and_releases_on_drop() {
        let dir = scratch();
        let path = dir.join("run.lock");
        let first = lock(&path).unwrap();
        assert!(lock(&path).is_err());
        drop(first);
        drop(lock(&path).unwrap());
        fs::remove_file(path).unwrap();
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn missing_settings_default_but_malformed_settings_error() {
        let dir = scratch();
        let path = dir.join("settings.json");
        assert_eq!(load(&path).unwrap(), Settings::default());
        save(&path, b"not json", 0o600).unwrap();
        assert!(load(&path).is_err());
        save(&path, br#"{"mode":"always","closed_lid":false}"#, 0o600).unwrap();
        assert_eq!(load(&path).unwrap().mode, Mode::Always);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::remove_file(path).unwrap();
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn locking_refuses_a_symlink() {
        let dir = scratch();
        let target = dir.join("target");
        fs::write(&target, b"unchanged").unwrap();
        let path = dir.join("run.lock");
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(lock(&path).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"unchanged");
        fs::remove_file(path).unwrap();
        fs::remove_file(target).unwrap();
        fs::remove_dir(dir).unwrap();
    }
}
