//! Ordinary-user desktop actions. Privileged removal uses only the bundled script.
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub fn can_uninstall() -> bool {
    cfg!(target_os = "macos")
        && Path::new("/Library/PrivilegedHelperTools/io.github.batterybridge.service").exists()
}

pub fn uninstall() -> Result<(), String> {
    if !can_uninstall() {
        return Err("The packaged macOS installation was not found.".into());
    }
    #[cfg(target_os = "macos")]
    {
        super::startup::disable_for_uninstall()?;
        let script = include_str!("../../../packaging/macos/uninstall.sh");
        let quoted = script
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n");
        let output = std::process::Command::new("/usr/bin/osascript")
            .args([
                "-e",
                &format!("do shell script \"{quoted}\" with administrator privileges"),
            ])
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().into());
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    Err("Graphical uninstall is available only for the macOS package.".into())
}

pub fn report_path() -> PathBuf {
    // The user can edit the complete destination before saving. No privileged path
    // is passed to the daemon and existing files are never silently overwritten.
    let home =
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
    let root = home
        .map(|home| {
            let downloads = home.join("Downloads");
            if downloads.is_dir() { downloads } else { home }
        })
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    root.join("batterybridge-probe.json")
}

pub fn save_report(path: &Path, report: &str) -> Result<(), String> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(report.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn saving_a_report_does_not_overwrite_an_existing_file() {
        let path = std::env::temp_dir().join(format!(
            "batterybridge-report-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        super::save_report(&path, "first").unwrap();
        assert!(super::save_report(&path, "second").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "first");
        std::fs::remove_file(path).unwrap();
    }
}
