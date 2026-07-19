use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use super::model::ResourceSample;

#[derive(Clone, Debug)]
pub(super) struct InvocationContext {
    pub compiler: String,
    pub scenario: String,
    pub iteration: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
enum ResourceResponse {
    Success {
        sample: ResourceSample,
    },
    ChildFailure {
        status: String,
        sample: ResourceSample,
    },
    SpawnFailure {
        error: String,
    },
}

pub(super) fn measure(
    command: &[OsString],
    context: &InvocationContext,
) -> Result<ResourceSample, String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("cannot locate benchmark executable: {error}"))?;
    let output = Command::new(executable)
        .arg("--resource-child")
        .args(command)
        .output()
        .map_err(|error| format!("cannot start resource helper: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "resource helper failed for {} {} {}: status {}; stderr: {}",
            context.compiler,
            context.scenario,
            context.iteration,
            output.status,
            String::from_utf8_lossy(&output.stderr).trim(),
        ));
    }
    match parse_response(&output.stdout)? {
        ResourceResponse::Success { sample } => Ok(sample),
        ResourceResponse::ChildFailure { status, .. } => Err(failure_diagnostic(
            command,
            context,
            &format!("measured child exited {status}"),
        )),
        ResourceResponse::SpawnFailure { error } => Err(format!(
            "{} {} {} failed to spawn: {error}; command: {}",
            context.compiler,
            context.scenario,
            context.iteration,
            format_command(command),
        )),
    }
}

fn parse_response(bytes: &[u8]) -> Result<ResourceResponse, String> {
    serde_json::from_slice(bytes)
        .map_err(|error| format!("invalid resource-helper response: {error}"))
}

fn failure_diagnostic(
    command: &[OsString],
    context: &InvocationContext,
    measured_status: &str,
) -> String {
    let replay = Command::new(&command[0]).args(&command[1..]).output();
    match replay {
        Ok(output) => format!(
            "{} {} {} failed ({measured_status}); command: {}; diagnostic replay status: {}; stderr: {}",
            context.compiler,
            context.scenario,
            context.iteration,
            format_command(command),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim(),
        ),
        Err(error) => format!(
            "{} {} {} failed ({measured_status}); command: {}; diagnostic replay could not start: {error}",
            context.compiler,
            context.scenario,
            context.iteration,
            format_command(command),
        ),
    }
}

pub(super) fn format_command(command: &[OsString]) -> String {
    command
        .iter()
        .map(|argument| shell_quote(argument.as_os_str()))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(value: &OsStr) -> String {
    let value = value.to_string_lossy();
    if !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_@%+=:,./-".contains(&byte))
    {
        value.into_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

pub(super) fn child(args: Vec<OsString>) -> ! {
    if args.is_empty() {
        eprintln!("resource child requires a command");
        std::process::exit(2);
    }
    let before = child_usage().unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(1);
    });
    let start = Instant::now();
    let status = Command::new(&args[0])
        .args(&args[1..])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let wall_ns = duration_ns(start.elapsed());
    let after = child_usage().unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(1);
    });
    let response = match status {
        Ok(status) => {
            let sample = usage_delta(&before, &after, wall_ns).unwrap_or_else(|error| {
                eprintln!("{error}");
                std::process::exit(1);
            });
            if status.success() {
                ResourceResponse::Success { sample }
            } else {
                ResourceResponse::ChildFailure {
                    status: status_text(status),
                    sample,
                }
            }
        }
        Err(error) => ResourceResponse::SpawnFailure {
            error: error.to_string(),
        },
    };
    serde_json::to_writer(std::io::stdout(), &response).unwrap_or_else(|error| {
        eprintln!("cannot serialize resource response: {error}");
        std::process::exit(1);
    });
    println!();
    std::process::exit(0);
}

#[cfg(unix)]
fn status_text(status: std::process::ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt;
    status.code().map_or_else(
        || format!("from signal {}", status.signal().unwrap_or_default()),
        |code| format!("with code {code}"),
    )
}

#[cfg(not(unix))]
fn status_text(status: std::process::ExitStatus) -> String {
    status.to_string()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ChildUsage {
    user_us: u64,
    system_us: u64,
    max_rss_kib: u64,
    minor_faults: u64,
    major_faults: u64,
    voluntary_context_switches: u64,
    involuntary_context_switches: u64,
}

fn usage_delta(
    before: &ChildUsage,
    after: &ChildUsage,
    wall_ns: u64,
) -> Result<ResourceSample, String> {
    let difference = |name: &str, before: u64, after: u64| {
        after
            .checked_sub(before)
            .ok_or_else(|| format!("resource counter {name} moved backwards"))
    };
    Ok(ResourceSample {
        wall_ns,
        user_us: difference("user_us", before.user_us, after.user_us)?,
        system_us: difference("system_us", before.system_us, after.system_us)?,
        max_rss_kib: after.max_rss_kib,
        minor_faults: difference("minor_faults", before.minor_faults, after.minor_faults)?,
        major_faults: difference("major_faults", before.major_faults, after.major_faults)?,
        voluntary_context_switches: difference(
            "voluntary_context_switches",
            before.voluntary_context_switches,
            after.voluntary_context_switches,
        )?,
        involuntary_context_switches: difference(
            "involuntary_context_switches",
            before.involuntary_context_switches,
            after.involuntary_context_switches,
        )?,
    })
}

#[cfg(all(
    target_os = "linux",
    target_env = "gnu",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn child_usage() -> Result<ChildUsage, String> {
    use std::mem::{align_of, size_of};
    use std::os::raw::{c_int, c_long};

    #[repr(C)]
    #[derive(Default)]
    struct TimeVal {
        seconds: c_long,
        microseconds: c_long,
    }

    #[repr(C)]
    #[derive(Default)]
    struct ResourceUsage {
        user: TimeVal,
        system: TimeVal,
        max_rss: c_long,
        shared_memory: c_long,
        unshared_data: c_long,
        unshared_stack: c_long,
        minor_faults: c_long,
        major_faults: c_long,
        swaps: c_long,
        block_inputs: c_long,
        block_outputs: c_long,
        messages_sent: c_long,
        messages_received: c_long,
        signals: c_long,
        voluntary_context_switches: c_long,
        involuntary_context_switches: c_long,
    }

    const _: [(); 16] = [(); size_of::<TimeVal>()];
    const _: [(); 8] = [(); align_of::<TimeVal>()];
    const _: [(); 144] = [(); size_of::<ResourceUsage>()];
    const _: [(); 8] = [(); align_of::<ResourceUsage>()];

    unsafe extern "C" {
        fn getrusage(who: c_int, usage: *mut ResourceUsage) -> c_int;
    }

    const RUSAGE_CHILDREN: c_int = -1;
    let mut usage = ResourceUsage::default();
    let result = unsafe { getrusage(RUSAGE_CHILDREN, &mut usage) };
    if result != 0 {
        return Err(format!(
            "getrusage failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    let micros = |value: &TimeVal| {
        (value.seconds.max(0) as u64) * 1_000_000 + value.microseconds.max(0) as u64
    };
    Ok(ChildUsage {
        user_us: micros(&usage.user),
        system_us: micros(&usage.system),
        max_rss_kib: usage.max_rss.max(0) as u64,
        minor_faults: usage.minor_faults.max(0) as u64,
        major_faults: usage.major_faults.max(0) as u64,
        voluntary_context_switches: usage.voluntary_context_switches.max(0) as u64,
        involuntary_context_switches: usage.involuntary_context_switches.max(0) as u64,
    })
}

#[cfg(not(all(
    target_os = "linux",
    target_env = "gnu",
    any(target_arch = "x86_64", target_arch = "aarch64")
)))]
fn child_usage() -> Result<ChildUsage, String> {
    Err("resource accounting supports only GNU Linux x86_64 and aarch64".to_string())
}

fn duration_ns(duration: std::time::Duration) -> u64 {
    duration.as_nanos().min(u64::MAX as u128) as u64
}

pub(super) fn resolve_executable(value: &str) -> Result<String, String> {
    let path = Path::new(value);
    let candidate = if path.components().count() > 1 {
        path.to_path_buf()
    } else {
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|directory| directory.join(path))
            .find(|candidate| candidate.is_file())
            .ok_or_else(|| format!("cannot resolve executable {value:?} through PATH"))?
    };
    let canonical = candidate
        .canonicalize()
        .map_err(|error| format!("cannot resolve executable {}: {error}", candidate.display()))?;
    if !canonical.is_file() {
        return Err(format!(
            "executable path is not a file: {}",
            canonical.display()
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = canonical
            .metadata()
            .map_err(|error| format!("cannot inspect {}: {error}", canonical.display()))?
            .permissions()
            .mode();
        if mode & 0o111 == 0 {
            return Err(format!(
                "executable path has no execute bit: {}",
                canonical.display()
            ));
        }
    }
    Ok(canonical.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::{ChildUsage, ResourceResponse, parse_response, resolve_executable, usage_delta};
    use crate::model::ResourceSample;

    fn sample() -> ResourceSample {
        ResourceSample {
            wall_ns: 1,
            user_us: 2,
            system_us: 3,
            max_rss_kib: 4,
            minor_faults: 5,
            major_faults: 6,
            voluntary_context_switches: 7,
            involuntary_context_switches: 8,
        }
    }

    #[test]
    fn parses_success_child_failure_and_spawn_failure() {
        for response in [
            ResourceResponse::Success { sample: sample() },
            ResourceResponse::ChildFailure {
                status: "with code 7".into(),
                sample: sample(),
            },
            ResourceResponse::SpawnFailure {
                error: "missing".into(),
            },
        ] {
            let json = serde_json::to_vec(&response).unwrap();
            assert!(parse_response(&json).is_ok());
        }
        assert!(parse_response(br#"{"outcome":"success"}"#).is_err());
        assert!(parse_response(b"not json").is_err());
    }

    #[test]
    fn resource_deltas_reject_counter_regression() {
        let before = ChildUsage {
            user_us: 10,
            ..ChildUsage::default()
        };
        let after = ChildUsage {
            user_us: 9,
            ..ChildUsage::default()
        };
        assert!(usage_delta(&before, &after, 1).is_err());
    }

    #[test]
    fn executable_resolution_rejects_non_executable_files() {
        let path =
            std::env::temp_dir().join(format!("njavac-non-executable-{}", std::process::id()));
        std::fs::write(&path, b"not executable").unwrap();
        assert!(resolve_executable(path.to_str().unwrap()).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
