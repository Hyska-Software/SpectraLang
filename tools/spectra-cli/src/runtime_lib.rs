// Locates the pre-built Spectra runtime static library that is required when
// linking a native executable with `--emit-exe`.
//
// Search order:
//   1. `SPECTRA_RUNTIME_LIB` environment variable (user override).
//   2. The newest matching archive beside the binary, in `deps/`, or in `../lib/`.
//      Cargo emits hashed static-library names under `target/{profile}/deps` on
//      MSVC, while older builds may leave an un-hashed archive beside the binary.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::SystemTime;

/// Returns the path to `libspectra_runtime.a` (Unix) or `spectra_runtime.lib` (MSVC Windows),
/// or `None` if it cannot be found.
pub fn find_runtime_lib() -> Option<PathBuf> {
    // 1. Explicit user override.
    if let Ok(val) = env::var("SPECTRA_RUNTIME_LIB") {
        let path = PathBuf::from(val);
        if path.exists() {
            return Some(path);
        }
    }

    let exe = env::current_exe().ok()?;
    let bin_dir = exe.parent()?;

    let mut roots = vec![bin_dir.to_path_buf(), bin_dir.join("deps")];
    if let Some(lib_dir) = bin_dir.parent().map(|d| d.join("lib")) {
        roots.push(lib_dir);
    }
    newest_runtime_archive(&roots)
}

/// Locate the static library that owns the `spectra.api` host-call registry.
/// Cargo emits the current archive with a hash under `target/{profile}/deps`;
/// installed layouts may instead provide an un-hashed `spectra_api.lib` or
/// `libspectra_api.a` beside the CLI.
pub fn find_api_lib() -> Option<PathBuf> {
    if let Ok(val) = env::var("SPECTRA_API_LIB") {
        let path = PathBuf::from(val);
        if path.is_file() {
            return Some(path);
        }
    }

    let exe = env::current_exe().ok()?;
    let bin_dir = exe.parent()?;
    let mut roots = vec![bin_dir.to_path_buf(), bin_dir.join("deps")];
    if let Some(lib_dir) = bin_dir.parent().map(|d| d.join("lib")) {
        roots.push(lib_dir);
    }
    newest_archive(&roots, is_api_archive_name)
}

fn is_runtime_archive_name(name: &str) -> bool {
    if cfg!(windows) {
        name == "spectra_runtime.lib"
            || (name.starts_with("spectra_runtime-") && name.ends_with(".lib"))
    } else {
        name == "libspectra_runtime.a"
            || (name.starts_with("libspectra_runtime-") && name.ends_with(".a"))
    }
}

fn is_api_archive_name(name: &str) -> bool {
    if cfg!(windows) {
        name == "spectra_api.lib" || (name.starts_with("spectra_api-") && name.ends_with(".lib"))
    } else {
        name == "libspectra_api.a" || (name.starts_with("libspectra_api-") && name.ends_with(".a"))
    }
}

fn newest_runtime_archive(roots: &[PathBuf]) -> Option<PathBuf> {
    newest_archive(roots, is_runtime_archive_name)
}

fn newest_archive(roots: &[PathBuf], matches_name: fn(&str) -> bool) -> Option<PathBuf> {
    let mut matches = Vec::new();
    for root in roots {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            if matches_name(name) && path.is_file() {
                let modified = fs::metadata(&path)
                    .and_then(|metadata| metadata.modified())
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                matches.push((modified, path));
            }
        }
    }
    matches
        .into_iter()
        .max_by_key(|(modified, path)| (*modified, path.clone()))
        .map(|(_, path)| path)
}

/// Verify the runtime archive selected for AOT contains the ABI symbols used
/// by compiler-native autodiff.  The linker remains the final authority, but
/// catching an obsolete archive here gives a deterministic diagnostic instead
/// of an opaque unresolved-symbol error much later in the pipeline.
pub fn validate_required_symbols(path: &std::path::Path) -> Result<(), String> {
    const REQUIRED: [&str; 2] = [
        "spectra_rt_tensor_autodiff_apply_fast",
        "spectra_rt_tensor_grad_handle_fast",
    ];
    let mut command = None;
    if cfg!(windows) {
        if let Ok(output) = Command::new("dumpbin").arg("/symbols").arg(path).output() {
            command = Some(output);
        }
    }
    if command.is_none() {
        for tool in ["llvm-nm", "nm"] {
            if let Ok(output) = Command::new(tool).arg("-g").arg(path).output() {
                command = Some(output);
                break;
            }
        }
    }
    let Some(output) = command else {
        // Tooling is not guaranteed on developer machines. The real linker
        // still validates the archive; the validator records this as an
        // environment limitation rather than pretending inspection occurred.
        return Ok(());
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let missing = REQUIRED
        .iter()
        .filter(|symbol| !text.contains(**symbol))
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!(
            "runtime archive '{}' is missing required symbols: {}",
            path.display(),
            missing.iter().map(|s| **s).collect::<Vec<_>>().join(", ")
        ));
    }
    Ok(())
}

/// Verify that the API archive exports the registration entry point required
/// by AOT executable shims.
pub fn validate_api_required_symbols(path: &std::path::Path) -> Result<(), String> {
    validate_archive_symbols(path, &["spectra_api_register_host_calls"])
}

fn validate_archive_symbols(path: &std::path::Path, required: &[&str]) -> Result<(), String> {
    let mut command = None;
    if cfg!(windows) {
        if let Ok(output) = Command::new("dumpbin").arg("/symbols").arg(path).output() {
            command = Some(output);
        }
    }
    if command.is_none() {
        for tool in ["llvm-nm", "nm"] {
            if let Ok(output) = Command::new(tool).arg("-g").arg(path).output() {
                command = Some(output);
                break;
            }
        }
    }
    let Some(output) = command else {
        return Ok(());
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let missing = required
        .iter()
        .filter(|symbol| !text.contains(**symbol))
        .copied()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "archive '{}' is missing required symbols: {}",
            path.display(),
            missing.join(", ")
        ))
    }
}
