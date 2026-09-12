// Linker detection and invocation for `--emit-exe`.
//
// On Windows: searches for MSVC `link.exe` via:
//   1. CC environment variable override
//   2. VCToolsInstallDir / VSINSTALLDIR env vars (set by vcvars)
//   3. vswhere.exe (standard VS Installer tool)
//   4. Known VS installation paths glob
//   5. System PATH (fallback, works in VS Developer Command Prompt)
//   6. MinGW gcc / clang fallback
//
// On Unix / macOS: tries `cc`, then `clang`, then `gcc`.
//
// The `CC` environment variable always takes priority and overrides detection.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fmt};

#[derive(Debug)]
pub enum LinkerKind {
    /// A Unix-style C compiler driver (cc / gcc / clang / …).
    Cc(PathBuf),
    /// Microsoft Visual C++ `link.exe` — stores the full path.
    Msvc(PathBuf),
}

impl fmt::Display for LinkerKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LinkerKind::Cc(p) => write!(f, "{}", p.display()),
            LinkerKind::Msvc(p) => write!(f, "link.exe (MSVC) at {}", p.display()),
        }
    }
}

/// Searches the system `PATH` for an executable with the given name.
fn find_in_path(name: &str) -> Option<PathBuf> {
    let paths = env::var_os("PATH")?;
    for dir in env::split_paths(&paths) {
        #[cfg(windows)]
        {
            let with_ext = dir.join(format!("{name}.exe"));
            if with_ext.is_file() {
                return Some(with_ext);
            }
        }
        let bare = dir.join(name);
        if bare.is_file() {
            return Some(bare);
        }
    }
    None
}

/// Returns true if the given path is actually MSVC's link.exe (not GNU ld).
fn is_msvc_link(p: &Path) -> bool {
    Command::new(p)
        .arg("/?")
        .output()
        .map(|out| {
            let out_str = String::from_utf8_lossy(&out.stdout);
            let err_str = String::from_utf8_lossy(&out.stderr);
            out_str.contains("Microsoft") || err_str.contains("Microsoft")
        })
        .unwrap_or(false)
}

/// Searches a `VC\Tools\MSVC` directory (which contains version-numbered
/// subdirs) for the first `HostX64\x64\link.exe` found, picking the newest
/// version first.
#[cfg(windows)]
fn find_link_in_msvc_dir(msvc_dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(msvc_dir).ok()?;
    let mut versions: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    // Sort descending so the newest MSVC toolset is tried first.
    versions.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    for ver in versions {
        let link = ver.join("bin").join("HostX64").join("x64").join("link.exe");
        if link.is_file() {
            return Some(link);
        }
    }
    None
}

/// Tries to find MSVC `link.exe` through VS-specific environment variables
/// and known installation paths (does NOT require `link.exe` to be on PATH).
#[cfg(windows)]
fn find_msvc_link() -> Option<PathBuf> {
    // 1. VCToolsInstallDir — set by vcvarsall / Developer Command Prompt.
    if let Ok(vc_tools) = env::var("VCToolsInstallDir") {
        let link = PathBuf::from(&vc_tools)
            .join("bin")
            .join("HostX64")
            .join("x64")
            .join("link.exe");
        if link.is_file() {
            return Some(link);
        }
    }

    // 2. VSINSTALLDIR — also set by vcvarsall.
    if let Ok(vs_dir) = env::var("VSINSTALLDIR") {
        let msvc = PathBuf::from(&vs_dir).join("VC").join("Tools").join("MSVC");
        if let Some(p) = find_link_in_msvc_dir(&msvc) {
            return Some(p);
        }
    }

    // 3. vswhere.exe — installed by every VS 2017+ installer.
    let vswhere =
        PathBuf::from(r"C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe");
    if vswhere.is_file() {
        if let Ok(out) = Command::new(&vswhere)
            .args(["-latest", "-property", "installationPath"])
            .output()
        {
            let vs_path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !vs_path.is_empty() {
                let msvc = PathBuf::from(&vs_path)
                    .join("VC")
                    .join("Tools")
                    .join("MSVC");
                if let Some(p) = find_link_in_msvc_dir(&msvc) {
                    return Some(p);
                }
            }
        }
    }

    // 4. Hardcoded search over the most common VS installation roots.
    for year in ["2022", "2019", "2017"] {
        for edition in ["Community", "Professional", "Enterprise", "BuildTools"] {
            // VS 2019+ default install under "Program Files"
            for pf in [r"C:\Program Files", r"C:\Program Files (x86)"] {
                let msvc = PathBuf::from(pf)
                    .join("Microsoft Visual Studio")
                    .join(year)
                    .join(edition)
                    .join("VC")
                    .join("Tools")
                    .join("MSVC");
                if let Some(p) = find_link_in_msvc_dir(&msvc) {
                    return Some(p);
                }
            }
        }
    }

    // 5. Fall back to whatever `link.exe` is on PATH and confirm it's MSVC.
    if let Some(p) = find_in_path("link.exe").or_else(|| find_in_path("link")) {
        if is_msvc_link(&p) {
            return Some(p);
        }
    }

    None
}

/// Detects the best available linker on the current host.
pub fn detect_linker() -> Option<LinkerKind> {
    // CC environment variable overrides everything.
    if let Ok(cc) = env::var("CC") {
        let p = PathBuf::from(&cc);
        if p.is_file() {
            return Some(LinkerKind::Cc(p));
        }
        if let Some(p) = find_in_path(&cc) {
            return Some(LinkerKind::Cc(p));
        }
    }

    #[cfg(windows)]
    {
        // Prefer MSVC link.exe — searches VS installations automatically.
        if let Some(p) = find_msvc_link() {
            return Some(LinkerKind::Msvc(p));
        }
        // Fall back to MinGW / LLVM toolchain.
        for name in &["gcc", "clang", "cc"] {
            if let Some(p) = find_in_path(name) {
                return Some(LinkerKind::Cc(p));
            }
        }
    }

    #[cfg(not(windows))]
    {
        for name in &["cc", "clang", "gcc"] {
            if let Some(p) = find_in_path(name) {
                return Some(LinkerKind::Cc(p));
            }
        }
    }

    None
}

/// Links `obj_path` with `runtime_lib_path` to produce a native executable at
/// `output_path`. Returns `Ok(())` on success or an error message on failure.
pub fn link_executable(
    obj_path: &Path,
    runtime_lib_path: &Path,
    api_lib_path: Option<&Path>,
    output_path: &Path,
    native_debug: bool,
) -> Result<(), String> {
    link_executable_many(
        std::slice::from_ref(&obj_path.to_path_buf()),
        runtime_lib_path,
        api_lib_path,
        output_path,
        native_debug,
    )
}

/// Links multiple relocatable Spectra objects with the runtime library.
///
/// Project AOT keeps one object per source module so cross-module symbols are
/// resolved by the native linker instead of being silently dropped by a
/// single-file compilation path.
pub fn link_executable_many(
    obj_paths: &[PathBuf],
    runtime_lib_path: &Path,
    api_lib_path: Option<&Path>,
    output_path: &Path,
    native_debug: bool,
) -> Result<(), String> {
    if obj_paths.is_empty() {
        return Err("No object files were provided to the linker.".to_string());
    }
    let linker = detect_linker().ok_or_else(|| {
        "No linker found. Install a C compiler (gcc, clang, or MSVC) or set the CC \
         environment variable to the path of your linker."
            .to_string()
    })?;

    match &linker {
        LinkerKind::Msvc(link_exe) => link_with_msvc(
            link_exe,
            obj_paths,
            runtime_lib_path,
            api_lib_path,
            output_path,
            native_debug,
        ),
        LinkerKind::Cc(cc) => link_with_cc(
            cc,
            obj_paths,
            runtime_lib_path,
            api_lib_path,
            output_path,
            native_debug,
        ),
    }
}

fn link_with_cc(
    cc: &Path,
    obj_paths: &[PathBuf],
    runtime_lib_path: &Path,
    api_lib_path: Option<&Path>,
    output_path: &Path,
    native_debug: bool,
) -> Result<(), String> {
    let runtime_lib_dir = runtime_lib_path.parent().ok_or_else(|| {
        format!(
            "Cannot determine parent directory of '{}'",
            runtime_lib_path.display()
        )
    })?;

    // Derive the bare library name (strip `lib` prefix and `.a` / `.so` suffix).
    let lib_stem = runtime_lib_path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.trim_start_matches("lib"))
        .unwrap_or("spectra_runtime");

    let mut cmd = Command::new(cc);
    cmd.args(obj_paths);
    // The API static archive is built with spectra-runtime as a dependency and
    // therefore already contains the runtime objects needed by the AOT shim.
    // Passing both archives makes static linkers see duplicate Rust symbols.
    if api_lib_path.is_none() {
        cmd.arg(format!("-L{}", runtime_lib_dir.display()))
            .arg(format!("-l{}", lib_stem));
    }
    cmd.arg("-o").arg(output_path);
    if let Some(api_lib_path) = api_lib_path {
        let api_lib_dir = api_lib_path.parent().ok_or_else(|| {
            format!(
                "Cannot determine parent directory of '{}'",
                api_lib_path.display()
            )
        })?;
        let api_lib_stem = api_lib_path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.trim_start_matches("lib"))
            .unwrap_or("spectra_api");
        cmd.arg(format!("-L{}", api_lib_dir.display()))
            .arg(format!("-l{}", api_lib_stem));
    }
    if native_debug {
        cmd.arg("-g");
    }

    // On macOS / Linux the runtime may use pthreads and system libraries.
    #[cfg(target_os = "linux")]
    cmd.arg("-lpthread").arg("-ldl").arg("-lm");
    #[cfg(target_os = "macos")]
    cmd.args(["-framework", "CoreFoundation"]);

    run_linker_command(cmd, "cc")
}

fn link_with_msvc(
    link_exe: &Path,
    obj_paths: &[PathBuf],
    runtime_lib_path: &Path,
    api_lib_path: Option<&Path>,
    output_path: &Path,
    native_debug: bool,
) -> Result<(), String> {
    let link_name = link_exe.display().to_string();
    let mut cmd = Command::new(link_exe);
    cmd.args(obj_paths);
    // `spectra-api` is a staticlib that embeds its spectra-runtime dependency.
    // When API registration is requested it is the sole runtime archive; adding
    // the standalone runtime archive as well produces LNK2005 duplicate symbols.
    if api_lib_path.is_none() {
        cmd.arg(runtime_lib_path);
    }
    if let Some(api_lib_path) = api_lib_path {
        cmd.arg(api_lib_path);
    }
    cmd
        // Standard Windows system libraries (same set Rust uses).
        .args([
            "ws2_32.lib",
            "bcrypt.lib",
            "userenv.lib",
            "ntdll.lib",
            "kernel32.lib",
            "advapi32.lib",
            "dbghelp.lib",
            "legacy_stdio_definitions.lib",
            "ole32.lib",
            "oleaut32.lib",
            "user32.lib",
            "gdi32.lib",
            "opengl32.lib",
            "d3dcompiler.lib",
        ])
        // Link the dynamic MSVC C runtime — provides mainCRTStartup, memcpy,
        // __CxxFrameHandler3 and other CRT symbols required by Rust's staticlib.
        .arg("/defaultlib:msvcrt.lib")
        .arg(format!("/OUT:{}", output_path.display()))
        .arg("/SUBSYSTEM:CONSOLE")
        .arg("/NOLOGO");
    if native_debug {
        let pdb_path = output_path.with_extension("pdb");
        cmd.arg("/DEBUG")
            .arg(format!("/PDB:{}", pdb_path.display()))
            .arg("/PDBALTPATH:%_PDB%");
    }

    // Collect MSVC and Windows SDK library search paths so link.exe can find
    // system .lib files even when running outside a Developer Command Prompt.
    let lib_paths = collect_msvc_lib_paths(link_exe);
    if !lib_paths.is_empty() {
        // Build a LIB env var by extending the existing one.
        let existing_lib = env::var("LIB").unwrap_or_default();
        let sep = if existing_lib.is_empty() { "" } else { ";" };
        let extra: String = lib_paths
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(";");
        cmd.env("LIB", format!("{existing_lib}{sep}{extra}"));
    }

    run_linker_command(cmd, &link_name)
}

/// Given the path to MSVC `link.exe`, returns a list of library directories
/// that should be added to `LIB` so that standard `.lib` files can be found.
#[cfg(windows)]
fn collect_msvc_lib_paths(link_exe: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();

    // Derive MSVC lib path from link.exe location.
    // link.exe is at: <vs_root>\VC\Tools\MSVC\<ver>\bin\HostX64\x64\link.exe
    // MSVC libs are:  <vs_root>\VC\Tools\MSVC\<ver>\lib\x64\
    if let Some(msvc_ver_dir) = link_exe
        .parent() // x64
        .and_then(Path::parent) // HostX64
        .and_then(Path::parent) // bin
        .and_then(Path::parent)
    // <ver>
    {
        let msvc_lib = msvc_ver_dir.join("lib").join("x64");
        if msvc_lib.is_dir() {
            paths.push(msvc_lib);
        }
    }

    // Find the Windows 10/11 SDK library paths.
    // Try the registry via `reg.exe query` first, then fall back to filesystem.
    let wk_root = find_windows_kits_root();
    let wk = wk_root.unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)\Windows Kits\10"));
    let lib_root = wk.join("Lib");
    if lib_root.is_dir() {
        // Enumerate SDK versions, pick the newest one.
        if let Ok(entries) = std::fs::read_dir(&lib_root) {
            let mut versions: Vec<PathBuf> = entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect();
            versions.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
            if let Some(sdk_ver) = versions.first() {
                let um = sdk_ver.join("um").join("x64");
                let ucrt = sdk_ver.join("ucrt").join("x64");
                if um.is_dir() {
                    paths.push(um);
                }
                if ucrt.is_dir() {
                    paths.push(ucrt);
                }
            }
        }
    }

    paths
}

#[cfg(not(windows))]
fn collect_msvc_lib_paths(_link_exe: &Path) -> Vec<PathBuf> {
    Vec::new()
}

/// Queries the Windows registry for the Windows Kits installation root.
#[cfg(windows)]
fn find_windows_kits_root() -> Option<PathBuf> {
    // Run `reg.exe query` — available on every modern Windows installation.
    let output = Command::new("reg")
        .args([
            "query",
            r"HKLM\SOFTWARE\Microsoft\Windows Kits\Installed Roots",
            "/v",
            "KitsRoot10",
        ])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    // Output looks like: "    KitsRoot10    REG_SZ    C:\Program Files (x86)\..."
    for line in text.lines() {
        if line.trim_start().starts_with("KitsRoot10") {
            let parts: Vec<&str> = line.split("REG_SZ").collect();
            if let Some(val) = parts.get(1) {
                let root = val.trim();
                if !root.is_empty() {
                    return Some(PathBuf::from(root));
                }
            }
        }
    }
    None
}

fn run_linker_command(mut cmd: Command, name: &str) -> Result<(), String> {
    let output = cmd.output().map_err(|e| {
        format!(
            "Failed to spawn linker '{}': {}. Make sure it is installed and on PATH.",
            name, e
        )
    })?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let detail = if !stderr.is_empty() {
        stderr.trim().to_string()
    } else {
        stdout.trim().to_string()
    };

    Err(format!(
        "Linker '{}' exited with status {}.\n{}",
        name, output.status, detail
    ))
}
