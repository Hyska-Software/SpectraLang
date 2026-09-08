// APPEND-ONLY (RemoteRegistry): remote HTTP(S) registry protocol.
//
// Protocol (static-file friendly, works on GitHub Pages / S3 / any file server):
//   <base>/<name>/<version>/package/<relative-path>   payload files
//   <base>/<name>/<version>/package.toml              RegistryMetadata (+ `files` list)
//   <base>/index/<name>.toml                          RemoteRegistryIndex version list
//
// Concurrency decision: index updates use GET-modify-PUT with ETag/If-Match when
// the server advertises an ETag; servers without ETag support degrade to
// documented last-write-wins semantics.

const REMOTE_HTTP_TIMEOUT_SECS: u64 = 30;
const REGISTRY_INDEX_SCHEMA: &str = "spectra-registry-index-v1";

pub fn is_remote_registry(value: &str) -> bool {
    let lowered = value.to_ascii_lowercase();
    lowered.starts_with("http://") || lowered.starts_with("https://")
}

fn remote_agent() -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(
            REMOTE_HTTP_TIMEOUT_SECS,
        )))
        .build();
    config.into()
}

fn normalize_remote_base(base_url: &str) -> Result<String, PackageError> {
    let trimmed = base_url.trim_end_matches('/');
    if !is_remote_registry(trimmed) {
        return Err(PackageError::Registry(format!(
            "remote registry '{}' must be an http:// or https:// URL",
            base_url
        )));
    }
    Ok(trimmed.to_string())
}

fn remote_package_url(base_url: &str, name: &str, version: &str) -> String {
    format!("{}/{}/{}", base_url.trim_end_matches('/'), name, version)
}

fn remote_index_url(base_url: &str, name: &str) -> String {
    format!("{}/index/{}.toml", base_url.trim_end_matches('/'), name)
}

fn remote_http_error(url: String, error: ureq::Error) -> PackageError {
    match error {
        ureq::Error::StatusCode(status) => PackageError::RemoteHttp { status, url },
        other => PackageError::RemoteTransport {
            url,
            message: other.to_string(),
        },
    }
}

struct RemoteGetResponse {
    body: Vec<u8>,
    etag: Option<String>,
}

fn remote_get(agent: &ureq::Agent, url: &str) -> Result<RemoteGetResponse, PackageError> {
    let mut response = agent
        .get(url)
        .call()
        .map_err(|error| remote_http_error(url.to_string(), error))?;
    let etag = response
        .headers()
        .get("etag")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let body = response.body_mut().read_to_vec().map_err(|error| {
        PackageError::RemoteTransport {
            url: url.to_string(),
            message: error.to_string(),
        }
    })?;
    Ok(RemoteGetResponse { body, etag })
}

fn remote_put_bytes(
    agent: &ureq::Agent,
    url: &str,
    payload: Vec<u8>,
    if_match: Option<&str>,
) -> Result<(), PackageError> {
    let mut request = agent
        .put(url)
        .header("Content-Type", "application/octet-stream");
    if let Some(tag) = if_match {
        request = request.header("If-Match", tag);
    }
    request
        .send(payload)
        .map_err(|error| remote_http_error(url.to_string(), error))?;
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
struct RemoteRegistryIndex {
    #[serde(default = "default_registry_index_schema")]
    schema: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    versions: Vec<String>,
}

fn default_registry_index_schema() -> String {
    REGISTRY_INDEX_SCHEMA.to_string()
}

fn update_remote_index(
    agent: &ureq::Agent,
    base_url: &str,
    name: &str,
    version: &str,
) -> Result<(), PackageError> {
    let url = remote_index_url(base_url, name);
    let (mut index, etag) = match remote_get(agent, &url) {
        Ok(response) => {
            let text = std::str::from_utf8(&response.body).map_err(|_| {
                PackageError::Registry(format!("remote index '{}' is not valid UTF-8", url))
            })?;
            let parsed: RemoteRegistryIndex =
                toml::from_str(text).map_err(|error| PackageError::Parse {
                    path: PathBuf::from(&url),
                    error,
                })?;
            (parsed, response.etag)
        }
        Err(PackageError::RemoteHttp { status: 404, .. }) => (
            RemoteRegistryIndex {
                schema: default_registry_index_schema(),
                name: name.to_string(),
                versions: Vec::new(),
            },
            None,
        ),
        Err(other) => return Err(other),
    };
    if !index.versions.iter().any(|existing| existing == version) {
        index.versions.push(version.to_string());
        index.versions.sort();
    }
    index.schema = default_registry_index_schema();
    index.name = name.to_string();
    let text = toml::to_string_pretty(&index).map_err(PackageError::Serialize)?;
    // If the server returned no ETag this PUT relies on last-write-wins semantics.
    remote_put_bytes(agent, &url, text.into_bytes(), etag.as_deref())
}

pub fn publish_remote(root: &Path, base_url: &str) -> Result<String, PackageError> {
    let base_url = normalize_remote_base(base_url)?;
    let workspace = resolve(root)?;
    let package = workspace
        .packages
        .first()
        .ok_or_else(|| PackageError::MissingPackage("root".to_string()))?;
    validate_package_identity(&package.name, &package.version)?;

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let staging_root = std::env::temp_dir().join(format!(
        "spectra-publish-{}-{}",
        std::process::id(),
        nonce
    ));
    fs::create_dir_all(&staging_root).map_err(|error| PackageError::Io {
        path: staging_root.clone(),
        error,
    })?;
    let result = publish_remote_staged(&base_url, package, &staging_root);
    let _ = fs::remove_dir_all(&staging_root);
    result
}

fn publish_remote_staged(
    base_url: &str,
    package: &ResolvedPackage,
    staging_root: &Path,
) -> Result<String, PackageError> {
    let payload_dir = staging_root.join("payload");
    fs::create_dir_all(&payload_dir).map_err(|error| PackageError::Io {
        path: payload_dir.clone(),
        error,
    })?;
    copy_package_payload(&package.root, &payload_dir)?;
    let checksum = directory_checksum(&payload_dir)?;
    let mut files = Vec::new();
    collect_files(&payload_dir, &payload_dir, &mut files)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let metadata = RegistryMetadata {
        name: package.name.clone(),
        version: package.version.clone(),
        channel: package.release.channel.as_str().to_string(),
        compatibility: package.release.compatibility.clone(),
        deprecated_since: package.release.deprecated_since.clone(),
        migration: package.release.migration.clone(),
        checksum,
        source_path: package.root.to_string_lossy().replace('\\', "/"),
        files: files
            .iter()
            .map(|(relative, _)| relative.to_string_lossy().replace('\\', "/"))
            .collect(),
    };
    let metadata_text = toml::to_string_pretty(&metadata).map_err(PackageError::Serialize)?;

    let agent = remote_agent();
    let package_base = remote_package_url(base_url, &package.name, &package.version);
    for (relative, full) in &files {
        let content = fs::read(full).map_err(|error| PackageError::Io {
            path: full.clone(),
            error,
        })?;
        let url = format!(
            "{}/package/{}",
            package_base,
            relative.to_string_lossy().replace('\\', "/")
        );
        remote_put_bytes(&agent, &url, content, None)?;
    }
    remote_put_bytes(
        &agent,
        &format!("{}/package.toml", package_base),
        metadata_text.into_bytes(),
        None,
    )?;
    update_remote_index(&agent, base_url, &package.name, &package.version)?;
    Ok(package_base)
}

fn ensure_remote_relative_path_is_safe(relative: &str) -> Result<(), PackageError> {
    if relative.is_empty()
        || relative.starts_with('/')
        || relative.contains('\\')
        || relative
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == ".." || component.contains(':'))
    {
        return Err(PackageError::UnsafePath {
            path: PathBuf::from(relative),
            reason: "remote package file list contains an unsafe path".to_string(),
        });
    }
    Ok(())
}

fn download_remote_payload(
    agent: &ureq::Agent,
    package_base: &str,
    files: &[String],
    staging: &Path,
) -> Result<(), PackageError> {
    fs::create_dir_all(staging).map_err(|error| PackageError::Io {
        path: staging.to_path_buf(),
        error,
    })?;
    for relative in files {
        ensure_remote_relative_path_is_safe(relative)?;
        let url = format!("{}/package/{}", package_base, relative);
        let response = remote_get(agent, &url)?;
        let destination = staging.join(relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|error| PackageError::Io {
                path: parent.to_path_buf(),
                error,
            })?;
        }
        fs::write(&destination, &response.body).map_err(|error| PackageError::Io {
            path: destination,
            error,
        })?;
    }
    Ok(())
}

pub(crate) fn install_from_remote_registry(
    workspace_root: &Path,
    base_url: &str,
    name: &str,
    version: &str,
) -> Result<InstalledRegistryPackage, PackageError> {
    let base_url = normalize_remote_base(base_url)?;
    validate_package_identity(name, version)?;
    let agent = remote_agent();
    let package_base = remote_package_url(&base_url, name, version);
    let metadata_response = remote_get(&agent, &format!("{}/package.toml", package_base))?;
    let metadata_text = String::from_utf8(metadata_response.body).map_err(|_| {
        PackageError::Registry(format!(
            "registry metadata for {} {} is not valid UTF-8",
            name, version
        ))
    })?;
    let metadata_path = PathBuf::from(format!("{}/package.toml", package_base));
    let metadata: RegistryMetadata =
        toml::from_str(&metadata_text).map_err(|error| PackageError::Parse {
            path: metadata_path,
            error,
        })?;
    if metadata.version != version {
        return Err(PackageError::Registry(format!(
            "remote registry metadata version '{}' does not match requested version '{}'",
            metadata.version, version
        )));
    }
    if metadata.files.is_empty() {
        return Err(PackageError::Registry(format!(
            "remote registry metadata for {} {} lists no payload files; republish the package to upgrade it",
            name, version
        )));
    }

    let vendor_dir = workspace_root
        .join(".spectra")
        .join("packages")
        .join(format!("{}-{}", name.replace('/', "_"), version));
    if let Some(parent) = vendor_dir.parent() {
        fs::create_dir_all(parent).map_err(|error| PackageError::Io {
            path: parent.to_path_buf(),
            error,
        })?;
    }
    let _lock = acquire_cache_lock(&vendor_dir)?;
    let staging = staging_path(&vendor_dir, "remote");
    let result = download_remote_payload(&agent, &package_base, &metadata.files, &staging)
        .and_then(|()| {
            let actual = directory_checksum(&staging)?;
            if actual != metadata.checksum {
                return Err(PackageError::Registry(format!(
                    "checksum mismatch for {} {} fetched from {}",
                    name, version, base_url
                )));
            }
            Ok(())
        })
        .and_then(|()| atomic_replace(&staging, &vendor_dir));
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result?;
    Ok(InstalledRegistryPackage {
        canonical_name: metadata.name,
        version: metadata.version,
        path: vendor_dir,
    })
}

fn install_registry_dependency(
    workspace_root: &Path,
    consumer_root: &Path,
    configured_remote: Option<&str>,
    registry_spec: &str,
    name: &str,
    version: &str,
    offline: bool,
) -> Result<InstalledRegistryPackage, PackageError> {
    if is_remote_registry(registry_spec) {
        return install_from_remote_registry(workspace_root, registry_spec, name, version);
    }
    let local_registry = consumer_root.join(registry_spec);
    let local_available = local_registry.join("package.toml").is_file();
    if let Some(remote) = configured_remote.filter(|_| !offline) {
        // Remote-first when [registry].remote is configured; fall back to the
        // local directory only when it actually holds the requested package.
        match install_from_remote_registry(workspace_root, remote, name, version) {
            Ok(installed) => return Ok(installed),
            Err(remote_error) => {
                if !local_available {
                    return Err(remote_error);
                }
            }
        }
    }
    install_from_registry(workspace_root, &local_registry, name, version)
}

#[cfg(test)]
mod remote_tests {
    use super::*;
    use std::io::{Read as _, Write as _};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};

    type SharedStore = Arc<Mutex<BTreeMap<String, Vec<u8>>>>;

    fn find_header_end(buffer: &[u8]) -> Option<usize> {
        buffer.windows(4).position(|window| window == b"\r\n\r\n")
    }

    // HTTP/1.1 keep-alive: ureq pools connections, so each accepted TCP stream
    // must serve repeated requests until the client closes it. Responding with
    // `Connection: close` races the pool and produces spurious resets.
    fn handle_connection(mut stream: TcpStream, store: &SharedStore) {
        let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(10)));
        let mut chunk = [0u8; 4096];
        let mut pending: Vec<u8> = Vec::new();
        loop {
            match read_one_request(&mut stream, &mut chunk, &mut pending) {
                Ok(Some(request)) => respond(&mut stream, store, request),
                Ok(None) => return,
                Err(()) => return,
            }
        }
    }

    struct TestRequest {
        method: String,
        path: String,
        if_match: Option<String>,
        body: Vec<u8>,
    }

    fn read_one_request(
        stream: &mut TcpStream,
        chunk: &mut [u8; 4096],
        pending: &mut Vec<u8>,
    ) -> Result<Option<TestRequest>, ()> {
        loop {
            if let Some(header_end) = find_header_end(pending) {
                let head = String::from_utf8_lossy(&pending[..header_end]).to_string();
                let mut lines = head.lines();
                let request_line = lines.next().unwrap_or_default();
                let mut parts = request_line.split_whitespace();
                let method = parts.next().unwrap_or_default().to_string();
                let target = parts.next().unwrap_or_default().to_string();
                let mut content_length = 0usize;
                let mut if_match: Option<String> = None;
                for line in lines {
                    if let Some((name, value)) = line.split_once(':') {
                        let normalized = name.trim().to_ascii_lowercase();
                        let value = value.trim().to_string();
                        if normalized == "content-length" {
                            content_length = value.parse().unwrap_or(0);
                        } else if normalized == "if-match" {
                            if_match = Some(value.trim_matches('"').to_string());
                        }
                    }
                }
                let header_bytes = header_end + 4;
                let mut body: Vec<u8> = pending[header_bytes..].to_vec();
                // Everything buffered so far has been copied into `body`; any
                // remainder arrives on the socket below.
                pending.clear();
                while body.len() < content_length {
                    match stream.read(chunk) {
                        Ok(0) => return Err(()),
                        Ok(read) => body.extend_from_slice(&chunk[..read]),
                        Err(_) => return Err(()),
                    }
                }
                body.truncate(content_length);
                let path = target.split('?').next().unwrap_or_default().to_string();
                if method.is_empty() {
                    return Err(());
                }
                return Ok(Some(TestRequest { method, path, if_match, body }));
            }
            match stream.read(chunk) {
                Ok(0) => return Ok(None),
                Ok(read) => pending.extend_from_slice(&chunk[..read]),
                Err(ref error) if error.kind() == io::ErrorKind::WouldBlock => continue,
                Err(ref error) if error.kind() == io::ErrorKind::TimedOut => return Ok(None),
                Err(_) => return Err(()),
            }
        }
    }

    fn respond(stream: &mut TcpStream, store: &SharedStore, request: TestRequest) {
        let (status, extra_headers, payload) = match request.method.as_str() {
            "GET" => match store.lock().expect("store lock").get(&request.path) {
                Some(bytes) => (
                    "200 OK",
                    format!("ETag: \"{}\"\r\n", stable_hash_hex(bytes)),
                    bytes.clone(),
                ),
                None => ("404 Not Found", String::new(), b"not found\n".to_vec()),
            },
            "PUT" => {
                let precondition_ok = match &request.if_match {
                    Some(expected) => store
                        .lock()
                        .expect("store lock")
                        .get(&request.path)
                        .map(|bytes| stable_hash_hex(bytes) == *expected)
                        .unwrap_or(false),
                    None => true,
                };
                if precondition_ok {
                    store
                        .lock()
                        .expect("store lock")
                        .insert(request.path, request.body);
                    ("201 Created", String::new(), Vec::new())
                } else {
                    ("412 Precondition Failed", String::new(), Vec::new())
                }
            }
            _ => ("405 Method Not Allowed", String::new(), Vec::new()),
        };

        let mut head = format!(
            "HTTP/1.1 {}\r\nContent-Length: {}\r\n",
            status,
            payload.len()
        );
        head.push_str(&extra_headers);
        head.push_str("\r\n");
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(&payload);
        let _ = stream.flush();
    }

    struct TestFileServer {
        base_url: String,
        store: SharedStore,
    }

    impl TestFileServer {
        fn start() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback listener");
            let port = listener.local_addr().expect("listener address").port();
            let store: SharedStore = Arc::new(Mutex::new(BTreeMap::new()));
            let thread_store = Arc::clone(&store);
            // Blocking accept loop on its own thread; each connection is served
            // by its own keep-alive thread so a slow or aborted connection can
            // never stall the others. The acceptor thread is intentionally left
            // running when the test ends — it dies with the test process.
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    match stream {
                        Ok(stream) => {
                            let store = Arc::clone(&thread_store);
                            std::thread::spawn(move || handle_connection(stream, &store));
                        }
                        Err(_) => continue,
                    }
                }
            });
            TestFileServer {
                base_url: format!("http://127.0.0.1:{}", port),
                store,
            }
        }

        fn key(&self, key: &str) -> Option<Vec<u8>> {
            self.store.lock().expect("store lock").get(key).cloned()
        }

        fn keys(&self) -> Vec<String> {
            self.store
                .lock()
                .expect("store lock")
                .keys()
                .cloned()
                .collect()
        }

        fn corrupt_first_byte(&self, key: &str) {
            if let Some(bytes) = self.store.lock().expect("store lock").get_mut(key) {
                if let Some(first) = bytes.first_mut() {
                    *first = first.wrapping_add(1);
                }
            }
        }
    }


    fn unique_temp_dir(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "spectralang-r917-{}-{}-{}",
            label,
            std::process::id(),
            nonce
        ));
        fs::create_dir_all(&dir).expect("temp project dir");
        dir
    }

    fn scaffold_project(root: &Path, name: &str, version: &str) {
        fs::create_dir_all(root.join("src")).expect("src dir");
        fs::write(
            root.join("spectra.toml"),
            format!("[project]\nname = \"{}\"\nversion = \"{}\"\n", name, version),
        )
        .expect("manifest");
        fs::write(
            root.join("src").join("hello.spectra"),
            "module hello;\n\nfun main(): unit {}\n",
        )
        .expect("source file");
    }

    #[test]
    fn publish_then_install_roundtrip_over_real_http_loopback() {
        let server = TestFileServer::start();
        let base = format!("{}/registry", server.base_url);

        let publisher_v1 = unique_temp_dir("publish-v1");
        scaffold_project(&publisher_v1, "demo-pkg", "1.2.3");
        let published =
            publish_remote(&publisher_v1, &base).expect("publish first version over HTTP");
        assert_eq!(published, format!("{}/demo-pkg/1.2.3", base));
        assert!(server
            .key("/registry/demo-pkg/1.2.3/package.toml")
            .is_some());
        assert!(server
            .key("/registry/demo-pkg/1.2.3/package/src/hello.spectra")
            .is_some());
        assert!(
            server.key("/registry/index/demo-pkg.toml").is_some(),
            "index missing; stored keys: {:?}",
            server.keys()
        );

        let publisher_v2 = unique_temp_dir("publish-v2");
        scaffold_project(&publisher_v2, "demo-pkg", "1.3.0");
        publish_remote(&publisher_v2, &base).expect("publish second version over HTTP");
        let index_text = String::from_utf8(
            server
                .key("/registry/index/demo-pkg.toml")
                .expect("index stored"),
        )
        .expect("index utf-8");
        assert!(index_text.contains("\"1.2.3\""), "index keeps v1: {}", index_text);
        assert!(index_text.contains("\"1.3.0\""), "index gains v2: {}", index_text);

        let consumer = unique_temp_dir("consumer");
        scaffold_project(&consumer, "consumer", "0.1.0");
        let installed = install_from_remote_registry(&consumer, &base, "demo-pkg", "1.2.3")
            .expect("install over HTTP");
        let vendored_hello = installed.path.join("src").join("hello.spectra");
        assert_eq!(
            fs::read_to_string(&vendored_hello).expect("vendored source"),
            fs::read_to_string(publisher_v1.join("src").join("hello.spectra"))
                .expect("published source")
        );

        fs::write(
            consumer.join("spectra.toml"),
            format!(
                "[project]\nname = \"consumer\"\nversion = \"0.1.0\"\n\n[registry]\nremote = \"{}\"\n\n[dependencies]\ndemo-pkg = {{ version = \"1.3.0\", registry = \"./missing-local-registry\" }}\n",
                base
            ),
        )
        .expect("consumer manifest");
        let workspace = resolve(&consumer).expect("resolve resolves dependency from remote");
        assert!(workspace
            .packages
            .iter()
            .any(|package| package.name == "demo-pkg" && package.version == "1.3.0"));
    }

    #[test]
    fn missing_remote_package_reports_typed_404() {
        let server = TestFileServer::start();
        let base = format!("{}/registry", server.base_url);
        let consumer = unique_temp_dir("missing");
        scaffold_project(&consumer, "consumer", "0.1.0");
        let error =
            install_from_remote_registry(&consumer, &base, "ghost", "1.0.0").expect_err("404");
        match error {
            PackageError::RemoteHttp { status, url } => {
                assert_eq!(status, 404);
                assert!(url.ends_with("/ghost/1.0.0/package.toml"), "url: {}", url);
            }
            other => panic!("expected typed 404, got: {:?}", other),
        }
    }

    #[test]
    fn tampered_remote_payload_fails_checksum_verification() {
        let server = TestFileServer::start();
        let base = format!("{}/registry", server.base_url);
        let publisher = unique_temp_dir("tamper-publish");
        scaffold_project(&publisher, "tamper-pkg", "1.0.0");
        publish_remote(&publisher, &base).expect("publish");

        server.corrupt_first_byte("/registry/tamper-pkg/1.0.0/package/src/hello.spectra");

        let consumer = unique_temp_dir("tamper-consumer");
        scaffold_project(&consumer, "consumer", "0.1.0");
        let error = install_from_remote_registry(&consumer, &base, "tamper-pkg", "1.0.0")
            .expect_err("checksum mismatch must fail the install");
        match error {
            PackageError::Registry(message) => {
                assert!(message.contains("checksum mismatch"), "message: {}", message);
            }
            other => panic!("expected checksum mismatch, got: {:?}", other),
        }
    }

    #[test]
    fn non_http_registry_target_is_rejected_without_network() {
        let publisher = unique_temp_dir("bad-url");
        scaffold_project(&publisher, "bad-pkg", "1.0.0");
        let error = publish_remote(&publisher, "./local-dir").expect_err("non-http rejected");
        assert!(matches!(error, PackageError::Registry(_)));
    }

}
