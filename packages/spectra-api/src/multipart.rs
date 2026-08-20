use crate::handles::ApiHandleTable;
use crate::{alloc_spectra_string, read_args, read_spectra_string, write_result};
use spectra_runtime::ffi::{
    SpectraHostCallContext, SpectraHostValue, HOST_STATUS_INVALID_ARGUMENT,
};
use spectra_runtime::handles::HandleKind;
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Multipart {
    parts: Vec<MultipartPart>,
}

impl Multipart {
    pub fn parse(
        body: &[u8],
        boundary: &str,
        limits: MultipartLimits,
    ) -> Result<Self, MultipartError> {
        parse_multipart(body, boundary, limits)
    }

    pub fn part_count(&self) -> usize {
        self.parts.len()
    }

    pub fn field_count(&self) -> usize {
        self.parts.iter().filter(|part| !part.is_file()).count()
    }

    pub fn file_count(&self) -> usize {
        self.parts.iter().filter(|part| part.is_file()).count()
    }

    pub fn part(&self, index: usize) -> Option<&MultipartPart> {
        self.parts.get(index)
    }

    pub fn text(&self, name: &str, index: usize) -> Option<&str> {
        self.parts
            .iter()
            .filter(|part| part.name == name && !part.is_file())
            .nth(index)
            .and_then(|part| part.text.as_deref())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MultipartPart {
    pub name: String,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub size: usize,
    pub text: Option<String>,
    pub spool_path: Option<PathBuf>,
}

impl MultipartPart {
    pub fn is_file(&self) -> bool {
        self.filename.is_some()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MultipartLimits {
    pub max_total_bytes: usize,
    pub max_parts: usize,
    pub max_part_bytes: usize,
}

impl Default for MultipartLimits {
    fn default() -> Self {
        Self {
            max_total_bytes: 16 * 1024 * 1024,
            max_parts: 128,
            max_part_bytes: 8 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MultipartErrorKind {
    InvalidBoundary,
    MissingOpeningBoundary,
    MissingHeaderTerminator,
    InvalidHeader,
    MissingName,
    TooManyParts,
    TotalTooLarge,
    PartTooLarge,
    InvalidUtf8,
    Io,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MultipartError {
    pub kind: MultipartErrorKind,
    pub part_index: usize,
    pub message: String,
}

impl MultipartError {
    fn new(kind: MultipartErrorKind, part_index: usize, message: impl Into<String>) -> Self {
        Self {
            kind,
            part_index,
            message: message.into(),
        }
    }

    fn code(&self) -> SpectraHostValue {
        match self.kind {
            MultipartErrorKind::InvalidBoundary => 1,
            MultipartErrorKind::MissingOpeningBoundary => 2,
            MultipartErrorKind::MissingHeaderTerminator => 3,
            MultipartErrorKind::InvalidHeader => 4,
            MultipartErrorKind::MissingName => 5,
            MultipartErrorKind::TooManyParts => 6,
            MultipartErrorKind::TotalTooLarge => 7,
            MultipartErrorKind::PartTooLarge => 8,
            MultipartErrorKind::InvalidUtf8 => 9,
            MultipartErrorKind::Io => 10,
        }
    }
}

impl fmt::Display for MultipartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at multipart part {}", self.message, self.part_index)
    }
}

impl std::error::Error for MultipartError {}

struct MultipartStore {
    multiparts: ApiHandleTable<Multipart>,
    parts: ApiHandleTable<MultipartPart>,
    last_error_code: SpectraHostValue,
    last_error_message: String,
}

impl MultipartStore {
    fn new() -> Self {
        Self {
            multiparts: ApiHandleTable::new(HandleKind::ApiMultipart),
            parts: ApiHandleTable::new(HandleKind::ApiMultipartPart),
            last_error_code: 0,
            last_error_message: String::new(),
        }
    }

    fn multipart_handle(&mut self, multipart: Multipart) -> SpectraHostValue {
        self.multiparts.insert(multipart)
    }

    fn part_handle(&mut self, part: MultipartPart) -> SpectraHostValue {
        self.parts.insert(part)
    }

    fn clear_error(&mut self) {
        self.last_error_code = 0;
        self.last_error_message.clear();
    }

    fn set_error(&mut self, error: MultipartError) {
        self.last_error_code = error.code();
        self.last_error_message = error.to_string();
    }
}

fn store() -> &'static Mutex<MultipartStore> {
    static STORE: OnceLock<Mutex<MultipartStore>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(MultipartStore::new()))
}

fn parse_multipart(
    body: &[u8],
    boundary: &str,
    limits: MultipartLimits,
) -> Result<Multipart, MultipartError> {
    if boundary.is_empty()
        || boundary.len() > 200
        || boundary
            .bytes()
            .any(|byte| byte <= b' ' || byte == b'"' || byte == b';')
    {
        return Err(MultipartError::new(
            MultipartErrorKind::InvalidBoundary,
            0,
            "multipart boundary is invalid",
        ));
    }
    if body.len() > limits.max_total_bytes {
        return Err(MultipartError::new(
            MultipartErrorKind::TotalTooLarge,
            0,
            "multipart body exceeds configured total size limit",
        ));
    }

    let marker = format!("--{boundary}");
    let closing = format!("--{boundary}--");
    let text = std::str::from_utf8(body).map_err(|_| {
        MultipartError::new(
            MultipartErrorKind::InvalidUtf8,
            0,
            "multipart body must be valid UTF-8 for header scanning",
        )
    })?;
    if !text.starts_with(&marker) {
        return Err(MultipartError::new(
            MultipartErrorKind::MissingOpeningBoundary,
            0,
            "multipart body is missing the opening boundary",
        ));
    }

    let mut parts = Vec::new();
    let mut cursor = marker.len();
    if text[cursor..].starts_with("--") {
        return Ok(Multipart { parts });
    }
    cursor = consume_boundary_newline(text, cursor).ok_or_else(|| {
        MultipartError::new(
            MultipartErrorKind::InvalidBoundary,
            0,
            "opening boundary must be followed by CRLF",
        )
    })?;

    loop {
        if parts.len() >= limits.max_parts {
            return Err(MultipartError::new(
                MultipartErrorKind::TooManyParts,
                parts.len(),
                "multipart body exceeds configured part count limit",
            ));
        }
        let header_end_rel = text[cursor..].find("\r\n\r\n").ok_or_else(|| {
            MultipartError::new(
                MultipartErrorKind::MissingHeaderTerminator,
                parts.len(),
                "multipart part is missing the header terminator",
            )
        })?;
        let header_start = cursor;
        let header_end = cursor + header_end_rel;
        let headers = parse_headers(&text[header_start..header_end], parts.len())?;
        let content_start = header_end + 4;
        let next_boundary_marker = format!("\r\n--{boundary}");
        let next_rel = text[content_start..]
            .find(&next_boundary_marker)
            .ok_or_else(|| {
                MultipartError::new(
                    MultipartErrorKind::InvalidBoundary,
                    parts.len(),
                    "multipart part is missing the following boundary",
                )
            })?;
        let content_end = content_start + next_rel;
        let content = &body[content_start..content_end];
        if content.len() > limits.max_part_bytes {
            return Err(MultipartError::new(
                MultipartErrorKind::PartTooLarge,
                parts.len(),
                "multipart part exceeds configured per-part size limit",
            ));
        }
        parts.push(build_part(headers, content, parts.len())?);

        cursor = content_end + 2;
        if text[cursor..].starts_with(&closing) {
            break;
        }
        if text[cursor..].starts_with(&marker) {
            cursor += marker.len();
            cursor = consume_boundary_newline(text, cursor).ok_or_else(|| {
                MultipartError::new(
                    MultipartErrorKind::InvalidBoundary,
                    parts.len(),
                    "part boundary must be followed by CRLF",
                )
            })?;
            continue;
        }
        return Err(MultipartError::new(
            MultipartErrorKind::InvalidBoundary,
            parts.len(),
            "multipart parser could not advance to the next boundary",
        ));
    }

    Ok(Multipart { parts })
}

fn consume_boundary_newline(text: &str, cursor: usize) -> Option<usize> {
    if text[cursor..].starts_with("\r\n") {
        Some(cursor + 2)
    } else if text[cursor..].starts_with('\n') {
        Some(cursor + 1)
    } else {
        None
    }
}

fn parse_headers(
    header_text: &str,
    part_index: usize,
) -> Result<HashMap<String, String>, MultipartError> {
    let mut headers = HashMap::new();
    for line in header_text.split("\r\n") {
        if line.is_empty() {
            continue;
        }
        let Some(colon) = line.find(':') else {
            return Err(MultipartError::new(
                MultipartErrorKind::InvalidHeader,
                part_index,
                "multipart part contains a malformed header",
            ));
        };
        let name = line[..colon].trim().to_ascii_lowercase();
        let value = line[colon + 1..].trim().to_string();
        if name.is_empty() || value.contains('\r') || value.contains('\n') {
            return Err(MultipartError::new(
                MultipartErrorKind::InvalidHeader,
                part_index,
                "multipart part contains an invalid header",
            ));
        }
        headers.insert(name, value);
    }
    Ok(headers)
}

fn build_part(
    headers: HashMap<String, String>,
    content: &[u8],
    part_index: usize,
) -> Result<MultipartPart, MultipartError> {
    let disposition = headers.get("content-disposition").ok_or_else(|| {
        MultipartError::new(
            MultipartErrorKind::InvalidHeader,
            part_index,
            "multipart part is missing Content-Disposition",
        )
    })?;
    let params = parse_content_disposition(disposition, part_index)?;
    let name = params.get("name").cloned().ok_or_else(|| {
        MultipartError::new(
            MultipartErrorKind::MissingName,
            part_index,
            "multipart part is missing a name parameter",
        )
    })?;
    let filename = params.get("filename").cloned();
    let content_type = headers.get("content-type").cloned();
    if filename.is_some() {
        let path = spool_file(content, part_index)?;
        Ok(MultipartPart {
            name,
            filename,
            content_type,
            size: content.len(),
            text: None,
            spool_path: Some(path),
        })
    } else {
        let text = std::str::from_utf8(content).map_err(|_| {
            MultipartError::new(
                MultipartErrorKind::InvalidUtf8,
                part_index,
                "text multipart field is not valid UTF-8",
            )
        })?;
        Ok(MultipartPart {
            name,
            filename: None,
            content_type,
            size: content.len(),
            text: Some(text.to_string()),
            spool_path: None,
        })
    }
}

fn parse_content_disposition(
    value: &str,
    part_index: usize,
) -> Result<HashMap<String, String>, MultipartError> {
    let mut pieces = value.split(';');
    let Some(kind) = pieces.next() else {
        return Err(MultipartError::new(
            MultipartErrorKind::InvalidHeader,
            part_index,
            "Content-Disposition is empty",
        ));
    };
    if !kind.trim().eq_ignore_ascii_case("form-data") {
        return Err(MultipartError::new(
            MultipartErrorKind::InvalidHeader,
            part_index,
            "Content-Disposition must be form-data",
        ));
    }
    let mut params = HashMap::new();
    for piece in pieces {
        let Some(eq) = piece.find('=') else {
            continue;
        };
        let key = piece[..eq].trim().to_ascii_lowercase();
        let mut raw = piece[eq + 1..].trim();
        if raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2 {
            raw = &raw[1..raw.len() - 1];
        }
        let value = raw.replace("\\\"", "\"").replace("\\\\", "\\");
        params.insert(key, value);
    }
    Ok(params)
}

fn spool_file(content: &[u8], part_index: usize) -> Result<PathBuf, MultipartError> {
    let dir = std::env::temp_dir().join("spectra-api-multipart");
    fs::create_dir_all(&dir).map_err(|error| {
        MultipartError::new(
            MultipartErrorKind::Io,
            part_index,
            format!("failed to create multipart spool directory: {error}"),
        )
    })?;
    let path = dir.join(format!(
        "part-{}-{}-{part_index}.bin",
        std::process::id(),
        unique_suffix()
    ));
    let mut file = fs::File::create(&path).map_err(|error| {
        MultipartError::new(
            MultipartErrorKind::Io,
            part_index,
            format!("failed to create multipart spool file: {error}"),
        )
    })?;
    file.write_all(content).map_err(|error| {
        MultipartError::new(
            MultipartErrorKind::Io,
            part_index,
            format!("failed to write multipart spool file: {error}"),
        )
    })?;
    Ok(path)
}

fn unique_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

pub extern "C" fn parse(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 5) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(body) = read_spectra_string(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(boundary) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let limits = MultipartLimits {
        max_total_bytes: positive_limit(args[2], MultipartLimits::default().max_total_bytes),
        max_parts: positive_limit(args[3], MultipartLimits::default().max_parts),
        max_part_bytes: positive_limit(args[4], MultipartLimits::default().max_part_bytes),
    };
    let mut store = store().lock().expect("multipart store poisoned");
    match Multipart::parse(body.as_bytes(), &boundary, limits) {
        Ok(multipart) => {
            store.clear_error();
            let handle = store.multipart_handle(multipart);
            write_result(ctx, handle)
        }
        Err(error) => {
            store.set_error(error);
            write_result(ctx, 0)
        }
    }
}

pub extern "C" fn part_count(ctx: *mut SpectraHostCallContext) -> i32 {
    multipart_int(ctx, |multipart| multipart.part_count() as SpectraHostValue)
}

pub extern "C" fn field_count(ctx: *mut SpectraHostCallContext) -> i32 {
    multipart_int(ctx, |multipart| multipart.field_count() as SpectraHostValue)
}

pub extern "C" fn file_count(ctx: *mut SpectraHostCallContext) -> i32 {
    multipart_int(ctx, |multipart| multipart.file_count() as SpectraHostValue)
}

pub extern "C" fn text(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(name) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let index = args[2].max(0) as usize;
    let store = store().lock().expect("multipart store poisoned");
    let value = store
        .multiparts
        .get(&args[0])
        .and_then(|multipart| multipart.text(&name, index))
        .unwrap_or("");
    write_result(ctx, alloc_spectra_string(value))
}

pub extern "C" fn part(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let index = args[1].max(0) as usize;
    let mut store = store().lock().expect("multipart store poisoned");
    let part = store
        .multiparts
        .get(&args[0])
        .and_then(|multipart| multipart.part(index))
        .cloned();
    let handle = part.map(|part| store.part_handle(part)).unwrap_or(0);
    write_result(ctx, handle)
}

pub extern "C" fn part_name(ctx: *mut SpectraHostCallContext) -> i32 {
    part_string(ctx, |part| part.name.clone())
}

pub extern "C" fn part_filename(ctx: *mut SpectraHostCallContext) -> i32 {
    part_string(ctx, |part| part.filename.clone().unwrap_or_default())
}

pub extern "C" fn part_content_type(ctx: *mut SpectraHostCallContext) -> i32 {
    part_string(ctx, |part| part.content_type.clone().unwrap_or_default())
}

pub extern "C" fn part_size(ctx: *mut SpectraHostCallContext) -> i32 {
    part_int(ctx, |part| part.size as SpectraHostValue)
}

pub extern "C" fn part_is_file(ctx: *mut SpectraHostCallContext) -> i32 {
    part_int(ctx, |part| i64::from(part.is_file()))
}

pub extern "C" fn file_path(ctx: *mut SpectraHostCallContext) -> i32 {
    part_string(ctx, |part| {
        part.spool_path
            .as_ref()
            .map(|path| path.to_string_lossy().to_string())
            .unwrap_or_default()
    })
}

pub extern "C" fn file_read(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let offset = args[1].max(0) as u64;
    let len = positive_limit(args[2], 64 * 1024);
    let mut store = store().lock().expect("multipart store poisoned");
    let result = store
        .parts
        .get(&args[0])
        .and_then(|part| part.spool_path.as_ref())
        .cloned()
        .map(|path| read_file_chunk(&path, offset, len));
    match result {
        Some(Ok(chunk)) => {
            store.clear_error();
            write_result(ctx, alloc_spectra_string(&chunk))
        }
        Some(Err(error)) => {
            store.set_error(error);
            write_result(ctx, alloc_spectra_string(""))
        }
        None => write_result(ctx, alloc_spectra_string("")),
    }
}

pub extern "C" fn file_spool_to(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(dest) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut store = store().lock().expect("multipart store poisoned");
    let result = store
        .parts
        .get(&args[0])
        .and_then(|part| part.spool_path.as_ref())
        .cloned()
        .map(|source| copy_spooled_file(&source, Path::new(&dest)));
    match result {
        Some(Ok(())) => {
            store.clear_error();
            write_result(ctx, 1)
        }
        Some(Err(error)) => {
            store.set_error(error);
            write_result(ctx, 0)
        }
        None => write_result(ctx, 0),
    }
}

pub extern "C" fn error_code(ctx: *mut SpectraHostCallContext) -> i32 {
    let store = store().lock().expect("multipart store poisoned");
    write_result(ctx, store.last_error_code)
}

pub extern "C" fn error_message(ctx: *mut SpectraHostCallContext) -> i32 {
    let store = store().lock().expect("multipart store poisoned");
    write_result(ctx, alloc_spectra_string(&store.last_error_message))
}

fn positive_limit(value: SpectraHostValue, default: usize) -> usize {
    if value <= 0 {
        default
    } else {
        value as usize
    }
}

fn multipart_int(
    ctx: *mut SpectraHostCallContext,
    lookup: impl FnOnce(&Multipart) -> SpectraHostValue,
) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().expect("multipart store poisoned");
    let value = store.multiparts.get(&args[0]).map(lookup).unwrap_or(0);
    write_result(ctx, value)
}

fn part_int(
    ctx: *mut SpectraHostCallContext,
    lookup: impl FnOnce(&MultipartPart) -> SpectraHostValue,
) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().expect("multipart store poisoned");
    let value = store.parts.get(&args[0]).map(lookup).unwrap_or(0);
    write_result(ctx, value)
}

fn part_string(
    ctx: *mut SpectraHostCallContext,
    lookup: impl FnOnce(&MultipartPart) -> String,
) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().expect("multipart store poisoned");
    let value = store.parts.get(&args[0]).map(lookup).unwrap_or_default();
    write_result(ctx, alloc_spectra_string(&value))
}

fn read_file_chunk(path: &Path, offset: u64, len: usize) -> Result<String, MultipartError> {
    let mut file = fs::File::open(path).map_err(|error| {
        MultipartError::new(
            MultipartErrorKind::Io,
            0,
            format!("failed to open multipart spool file: {error}"),
        )
    })?;
    file.seek(SeekFrom::Start(offset)).map_err(|error| {
        MultipartError::new(
            MultipartErrorKind::Io,
            0,
            format!("failed to seek multipart spool file: {error}"),
        )
    })?;
    let mut buffer = vec![0_u8; len];
    let read = file.read(&mut buffer).map_err(|error| {
        MultipartError::new(
            MultipartErrorKind::Io,
            0,
            format!("failed to read multipart spool file: {error}"),
        )
    })?;
    buffer.truncate(read);
    Ok(String::from_utf8_lossy(&buffer).to_string())
}

fn copy_spooled_file(source: &Path, dest: &Path) -> Result<(), MultipartError> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            MultipartError::new(
                MultipartErrorKind::Io,
                0,
                format!("failed to create multipart destination directory: {error}"),
            )
        })?;
    }
    fs::copy(source, dest).map(|_| ()).map_err(|error| {
        MultipartError::new(
            MultipartErrorKind::Io,
            0,
            format!("failed to copy multipart spool file: {error}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_body() -> String {
        concat!(
            "--BOUNDARY\r\n",
            "Content-Disposition: form-data; name=\"title\"\r\n",
            "\r\n",
            "Hello upload\r\n",
            "--BOUNDARY\r\n",
            "Content-Disposition: form-data; name=\"file\"; filename=\"hello.txt\"\r\n",
            "Content-Type: text/plain\r\n",
            "\r\n",
            "abcdef\r\n",
            "--BOUNDARY--\r\n",
        )
        .to_string()
    }

    #[test]
    fn parses_text_and_file_parts_with_spooled_file_reader() {
        let multipart = Multipart::parse(
            sample_body().as_bytes(),
            "BOUNDARY",
            MultipartLimits {
                max_total_bytes: 4096,
                max_parts: 4,
                max_part_bytes: 1024,
            },
        )
        .expect("multipart parses");
        assert_eq!(multipart.part_count(), 2);
        assert_eq!(multipart.field_count(), 1);
        assert_eq!(multipart.file_count(), 1);
        assert_eq!(multipart.text("title", 0), Some("Hello upload"));
        let file = multipart.part(1).expect("file part");
        assert!(file.is_file());
        assert_eq!(file.filename.as_deref(), Some("hello.txt"));
        assert_eq!(file.content_type.as_deref(), Some("text/plain"));
        assert_eq!(file.size, 6);
        assert!(file.spool_path.as_ref().is_some_and(|path| path.exists()));
    }

    #[test]
    fn enforces_total_part_count_and_per_part_limits() {
        let total = Multipart::parse(
            sample_body().as_bytes(),
            "BOUNDARY",
            MultipartLimits {
                max_total_bytes: 10,
                max_parts: 4,
                max_part_bytes: 1024,
            },
        )
        .expect_err("total limit");
        assert_eq!(total.kind, MultipartErrorKind::TotalTooLarge);

        let count = Multipart::parse(
            sample_body().as_bytes(),
            "BOUNDARY",
            MultipartLimits {
                max_total_bytes: 4096,
                max_parts: 1,
                max_part_bytes: 1024,
            },
        )
        .expect_err("part count limit");
        assert_eq!(count.kind, MultipartErrorKind::TooManyParts);

        let part = Multipart::parse(
            sample_body().as_bytes(),
            "BOUNDARY",
            MultipartLimits {
                max_total_bytes: 4096,
                max_parts: 4,
                max_part_bytes: 4,
            },
        )
        .expect_err("part size limit");
        assert_eq!(part.kind, MultipartErrorKind::PartTooLarge);
    }

    #[test]
    fn rejects_malformed_multipart_bodies() {
        let missing = Multipart::parse(b"plain", "BOUNDARY", MultipartLimits::default())
            .expect_err("missing boundary");
        assert_eq!(missing.kind, MultipartErrorKind::MissingOpeningBoundary);
        let malformed = Multipart::parse(
            b"--BOUNDARY\r\nContent-Disposition form-data\r\n\r\nx\r\n--BOUNDARY--\r\n",
            "BOUNDARY",
            MultipartLimits::default(),
        )
        .expect_err("bad header");
        assert_eq!(malformed.kind, MultipartErrorKind::InvalidHeader);
    }
}
