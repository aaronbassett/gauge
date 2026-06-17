//! Coarse environment capture for the `install`/`heartbeat` events, plus the
//! `os.type`/`host.arch` remaps the Gauge profile requires. Everything is
//! best-effort: a field that can't be detected is omitted (`None`).

use serde::Serialize;

/// Coarse, low-cardinality environment attributes. Sent only on low-frequency
/// lifecycle events. Quantities are raw integers (bucketed at read time).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct EnvAttributes {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_cores: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ram_gb: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub libc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
}

/// `os.type` resource attribute, remapped to the Gauge profile vocabulary.
pub fn os_type() -> String {
    match std::env::consts::OS {
        "macos" => "darwin",
        other => other, // "linux", "windows"
    }
    .to_string()
}

/// `host.arch` resource attribute, remapped to the Gauge profile vocabulary.
pub fn host_arch() -> String {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    }
    .to_string()
}

/// libc is a compile-time property on Linux; `None` off Linux.
fn libc() -> Option<String> {
    if cfg!(target_os = "linux") {
        Some(
            if cfg!(target_env = "musl") {
                "musl"
            } else {
                "glibc"
            }
            .to_string(),
        )
    } else {
        None
    }
}

/// Language subtag only, e.g. `en_US.UTF-8` → `en`. Strips any `@modifier`
/// and treats the `C`/`POSIX`/`C.UTF-8` no-locale values as absent. Pure for
/// testability.
pub fn language_from(lang: Option<&str>) -> Option<String> {
    let raw = lang?.trim();
    if raw.is_empty() {
        return None;
    }
    let subtag = raw
        .split(['_', '.', '-', '@'])
        .next()
        .unwrap_or(raw)
        .to_ascii_lowercase();
    if subtag.is_empty() || subtag == "c" || subtag == "posix" {
        return None;
    }
    Some(subtag)
}

/// Map a `$SHELL` path to a closed enum string. Pure for testability.
pub fn shell_from(shell_path: Option<&str>) -> Option<String> {
    let base = std::path::Path::new(shell_path?.trim())
        .file_name()?
        .to_str()?
        .to_ascii_lowercase();
    Some(
        match base.as_str() {
            "bash" => "bash",
            "zsh" => "zsh",
            "fish" => "fish",
            "pwsh" | "powershell" | "pwsh.exe" | "powershell.exe" => "pwsh",
            "cmd" | "cmd.exe" => "cmd",
            _ => "other",
        }
        .to_string(),
    )
}

/// Detect everything. `accel` is supplied by the app (it knows its inference
/// backend better than any detector); pass `None` to omit it.
pub fn detect(accel: Option<String>) -> EnvAttributes {
    EnvAttributes {
        os_version: os_version(),
        cpu_cores: std::thread::available_parallelism()
            .ok()
            .map(|n| n.get() as u32),
        ram_gb: ram_gb(),
        accel,
        libc: libc(),
        language: language_from(std::env::var("LANG").ok().as_deref()),
        shell: shell_from(std::env::var("SHELL").ok().as_deref()),
    }
}

/// `<id>:<major>` e.g. `darwin:14`, `ubuntu:22`, `windows:11`. Best-effort.
fn os_version() -> Option<String> {
    // Lowercase defensively so the macos→darwin remap can't be missed by a
    // mixed-case id from a future sysinfo version.
    let id = sysinfo::System::distribution_id().to_lowercase();
    let id = if id == "macos" {
        "darwin".to_string()
    } else {
        id
    };
    let ver = sysinfo::System::os_version()?; // e.g. "14.5", "22.04", "11"
    let major = ver.split(['.', ' ']).next().filter(|s| !s.is_empty())?;
    Some(format!("{id}:{major}"))
}

/// Total physical RAM rounded to whole GB. Best-effort.
fn ram_gb() -> Option<u32> {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    let bytes = sys.total_memory(); // sysinfo >= 0.30 returns BYTES
    if bytes == 0 {
        return None;
    }
    Some((bytes as f64 / 1_073_741_824.0).round() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_and_arch_are_profile_vocab() {
        assert!(["darwin", "linux", "windows"].contains(&os_type().as_str()));
        assert!(["amd64", "arm64"].contains(&host_arch().as_str()) || !host_arch().is_empty());
    }

    #[test]
    fn language_subtag_only() {
        assert_eq!(language_from(Some("en_US.UTF-8")).as_deref(), Some("en"));
        assert_eq!(language_from(Some("de_DE")).as_deref(), Some("de"));
        assert_eq!(language_from(Some("C")), None);
        assert_eq!(language_from(None), None);
        // `@modifier` stripped; C/POSIX/C.UTF-8 treated as no-locale.
        assert_eq!(language_from(Some("C.UTF-8")), None);
        assert_eq!(language_from(Some("en@piglatin")).as_deref(), Some("en"));
        assert_eq!(language_from(Some("POSIX")), None);
        assert_eq!(language_from(Some("c")), None);
    }

    #[test]
    fn shell_maps_to_enum() {
        assert_eq!(shell_from(Some("/bin/zsh")).as_deref(), Some("zsh"));
        assert_eq!(shell_from(Some("/usr/bin/fish")).as_deref(), Some("fish"));
        assert_eq!(
            shell_from(Some("/opt/weird/tcsh")).as_deref(),
            Some("other")
        );
        assert_eq!(shell_from(None), None);
        // A `pwsh.exe` filename classifies as pwsh. Use forward slashes so the
        // filename is extracted identically on every host (`Path::file_name`
        // does not split on `\` off Windows, which is where this test runs).
        assert_eq!(
            shell_from(Some("C:/Program Files/PowerShell/7/pwsh.exe")).as_deref(),
            Some("pwsh")
        );
    }

    #[test]
    fn detect_does_not_panic_and_sees_cpus() {
        let env = detect(Some("cpu".into()));
        assert!(env.cpu_cores.unwrap_or(0) >= 1);
        assert_eq!(env.accel.as_deref(), Some("cpu"));
    }
}
