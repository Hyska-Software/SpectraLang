use std::ffi::c_char;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::ptr;
use std::slice;

pub const SPECTRA_INTEROP_OK: i32 = 0;
pub const SPECTRA_INTEROP_INVALID_ARGUMENT: i32 = 1;
pub const SPECTRA_INTEROP_IO_ERROR: i32 = 2;
pub const SPECTRA_INTEROP_FORMAT_ERROR: i32 = 3;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SpectraF64Array {
    pub data: *mut f64,
    pub len: usize,
}

impl SpectraF64Array {
    fn null() -> Self {
        Self {
            data: ptr::null_mut(),
            len: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TensorF64 {
    values: Vec<f64>,
}

impl TensorF64 {
    pub fn new(values: Vec<f64>) -> Self {
        Self { values }
    }

    pub fn as_slice(&self) -> &[f64] {
        &self.values
    }

    pub fn sum(&self) -> f64 {
        self.values.iter().sum()
    }

    pub fn write_npy(&self, path: impl AsRef<Path>) -> Result<(), InteropError> {
        write_npy_f64(path.as_ref(), &self.values)
    }

    pub fn read_npy(path: impl AsRef<Path>) -> Result<Self, InteropError> {
        read_npy_f64(path.as_ref()).map(Self::new)
    }
}

#[derive(Debug)]
pub enum InteropError {
    Io(std::io::Error),
    Format(String),
}

impl From<std::io::Error> for InteropError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[no_mangle]
pub extern "C" fn spectra_interop_abi_version() -> u32 {
    1
}

#[no_mangle]
pub extern "C" fn spectra_interop_add_i64(lhs: i64, rhs: i64) -> i64 {
    lhs.saturating_add(rhs)
}

#[no_mangle]
pub extern "C" fn spectra_interop_identity_i8(value: i8) -> i8 {
    value
}
#[no_mangle]
pub extern "C" fn spectra_interop_identity_u8(value: u8) -> u8 {
    value
}
#[no_mangle]
pub extern "C" fn spectra_interop_identity_i16(value: i16) -> i16 {
    value
}
#[no_mangle]
pub extern "C" fn spectra_interop_identity_u16(value: u16) -> u16 {
    value
}
#[no_mangle]
pub extern "C" fn spectra_interop_identity_i32(value: i32) -> i32 {
    value
}
#[no_mangle]
pub extern "C" fn spectra_interop_identity_u32(value: u32) -> u32 {
    value
}
#[no_mangle]
pub extern "C" fn spectra_interop_identity_i64(value: i64) -> i64 {
    value
}
#[no_mangle]
pub extern "C" fn spectra_interop_identity_u64(value: u64) -> u64 {
    value
}
#[no_mangle]
pub extern "C" fn spectra_interop_identity_f32(value: f32) -> f32 {
    value
}
#[no_mangle]
pub extern "C" fn spectra_interop_identity_f64(value: f64) -> f64 {
    value
}

#[no_mangle]
/// # Safety
///
/// `out` must be either null (which is rejected) or a valid, writable pointer
/// to an `i8` for the duration of the call.
pub unsafe extern "C" fn spectra_interop_checked_i64_to_i8(value: i64, out: *mut i8) -> i32 {
    if out.is_null() || value < i8::MIN as i64 || value > i8::MAX as i64 {
        return SPECTRA_INTEROP_INVALID_ARGUMENT;
    }
    *out = value as i8;
    SPECTRA_INTEROP_OK
}

#[no_mangle]
/// # Safety
///
/// When `len` is non-zero, `data` must be non-null, properly aligned, and
/// point to `len` readable `f64` values for the duration of the call. A null
/// pointer is accepted only with `len == 0`.
pub unsafe extern "C" fn spectra_interop_tensor_f64_sum(data: *const f64, len: usize) -> f64 {
    if data.is_null() {
        if len == 0 {
            return 0.0;
        }
        return f64::NAN;
    }
    slice::from_raw_parts(data, len).iter().sum()
}

#[no_mangle]
/// # Safety
///
/// `path` must point to `path_len` readable bytes containing UTF-8, and when
/// `len` is non-zero `data` must point to `len` readable, properly aligned
/// `f64` values. A null `data` pointer is accepted only with `len == 0`.
pub unsafe extern "C" fn spectra_interop_npy_write_f64(
    path: *const c_char,
    path_len: usize,
    data: *const f64,
    len: usize,
) -> i32 {
    let Some(path) = path_from_raw(path, path_len) else {
        return SPECTRA_INTEROP_INVALID_ARGUMENT;
    };
    if data.is_null() && len != 0 {
        return SPECTRA_INTEROP_INVALID_ARGUMENT;
    }
    let values = if len == 0 {
        &[]
    } else {
        slice::from_raw_parts(data, len)
    };
    match write_npy_f64(Path::new(&path), values) {
        Ok(()) => SPECTRA_INTEROP_OK,
        Err(InteropError::Io(_)) => SPECTRA_INTEROP_IO_ERROR,
        Err(InteropError::Format(_)) => SPECTRA_INTEROP_FORMAT_ERROR,
    }
}

#[no_mangle]
/// # Safety
///
/// `path` must point to `path_len` readable bytes containing UTF-8 for the
/// duration of the call.
pub unsafe extern "C" fn spectra_interop_npy_read_f64(
    path: *const c_char,
    path_len: usize,
) -> SpectraF64Array {
    let Some(path) = path_from_raw(path, path_len) else {
        return SpectraF64Array::null();
    };
    match read_npy_f64(Path::new(&path)) {
        Ok(values) => array_from_vec(values),
        Err(_) => SpectraF64Array::null(),
    }
}

#[no_mangle]
/// # Safety
///
/// `array` must be an array previously returned by this library and not have
/// been freed already. Its pointer, length, and allocation must remain valid
/// until this call takes ownership of it.
pub unsafe extern "C" fn spectra_interop_f64_array_free(array: SpectraF64Array) {
    if array.data.is_null() {
        return;
    }
    let _ = Vec::from_raw_parts(array.data, array.len, array.len);
}

fn array_from_vec(mut values: Vec<f64>) -> SpectraF64Array {
    values.shrink_to_fit();
    let array = SpectraF64Array {
        data: values.as_mut_ptr(),
        len: values.len(),
    };
    std::mem::forget(values);
    array
}

unsafe fn path_from_raw(path: *const c_char, path_len: usize) -> Option<String> {
    if path.is_null() {
        return None;
    }
    let bytes = slice::from_raw_parts(path as *const u8, path_len);
    std::str::from_utf8(bytes)
        .ok()
        .map(|value| value.to_string())
}

pub fn write_npy_f64(path: &Path, values: &[f64]) -> Result<(), InteropError> {
    let header = format!(
        "{{'descr': '<f8', 'fortran_order': False, 'shape': ({},), }}",
        values.len()
    );
    let prelude_len = 10usize;
    let newline_len = 1usize;
    let padding = (16 - ((prelude_len + header.len() + newline_len) % 16)) % 16;
    let mut padded_header = header;
    padded_header.extend(std::iter::repeat_n(' ', padding));
    padded_header.push('\n');
    if padded_header.len() > u16::MAX as usize {
        return Err(InteropError::Format("npy header too large".to_string()));
    }

    let mut file = fs::File::create(path)?;
    file.write_all(b"\x93NUMPY")?;
    file.write_all(&[1, 0])?;
    file.write_all(&(padded_header.len() as u16).to_le_bytes())?;
    file.write_all(padded_header.as_bytes())?;
    for value in values {
        file.write_all(&value.to_le_bytes())?;
    }
    Ok(())
}

pub fn read_npy_f64(path: &Path) -> Result<Vec<f64>, InteropError> {
    let mut file = fs::File::open(path)?;
    let mut magic = [0u8; 6];
    file.read_exact(&mut magic)?;
    if &magic != b"\x93NUMPY" {
        return Err(InteropError::Format("invalid npy magic".to_string()));
    }
    let mut version = [0u8; 2];
    file.read_exact(&mut version)?;
    if version != [1, 0] {
        return Err(InteropError::Format(
            "only npy v1.0 is supported".to_string(),
        ));
    }
    let mut header_len = [0u8; 2];
    file.read_exact(&mut header_len)?;
    let header_len = u16::from_le_bytes(header_len) as usize;
    let mut header = vec![0u8; header_len];
    file.read_exact(&mut header)?;
    let header = std::str::from_utf8(&header)
        .map_err(|_| InteropError::Format("npy header is not utf-8".to_string()))?;
    if !header.contains("'descr': '<f8'") && !header.contains("\"descr\": \"<f8\"") {
        return Err(InteropError::Format(
            "only little-endian f64 npy is supported".to_string(),
        ));
    }
    if !header.contains("'fortran_order': False")
        && !header.contains("\"fortran_order\": False")
        && !header.contains("\"fortran_order\": false")
    {
        return Err(InteropError::Format(
            "fortran-order arrays are not supported".to_string(),
        ));
    }
    let len = parse_shape_len(header)?;
    let mut bytes = vec![0u8; len * std::mem::size_of::<f64>()];
    file.read_exact(&mut bytes)?;
    let mut values = Vec::with_capacity(len);
    for chunk in bytes.chunks_exact(8) {
        let mut raw = [0u8; 8];
        raw.copy_from_slice(chunk);
        values.push(f64::from_le_bytes(raw));
    }
    Ok(values)
}

fn parse_shape_len(header: &str) -> Result<usize, InteropError> {
    let shape_pos = header
        .find("'shape':")
        .or_else(|| header.find("\"shape\":"))
        .ok_or_else(|| InteropError::Format("npy shape missing".to_string()))?;
    let after_shape = &header[shape_pos..];
    let open = after_shape
        .find('(')
        .ok_or_else(|| InteropError::Format("npy shape tuple missing".to_string()))?;
    let close = after_shape[open + 1..]
        .find(')')
        .ok_or_else(|| InteropError::Format("npy shape tuple missing".to_string()))?;
    let shape = &after_shape[open + 1..open + 1 + close];
    let first_dim = shape
        .split(',')
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| InteropError::Format("npy shape is empty".to_string()))?;
    first_dim
        .parse::<usize>()
        .map_err(|_| InteropError::Format("npy shape length is invalid".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_width_c_abi_preserves_signed_unsigned_and_float_widths() {
        assert_eq!(spectra_interop_identity_i8(-7), -7);
        assert_eq!(spectra_interop_identity_u8(250), 250);
        assert_eq!(spectra_interop_identity_i16(-300), -300);
        assert_eq!(spectra_interop_identity_u16(60_000), 60_000);
        assert_eq!(spectra_interop_identity_i32(-70_000), -70_000);
        assert_eq!(spectra_interop_identity_u32(4_000_000_000), 4_000_000_000);
        assert_eq!(spectra_interop_identity_i64(-9_000_000_000), -9_000_000_000);
        assert_eq!(
            spectra_interop_identity_u64(9_000_000_000_000_000_000),
            9_000_000_000_000_000_000
        );
        assert_eq!(spectra_interop_identity_f32(1.25), 1.25);
        assert_eq!(spectra_interop_identity_f64(1.0 / 3.0), 1.0 / 3.0);
    }

    #[test]
    fn exact_width_c_abi_rejects_checked_narrowing() {
        let mut output = 0_i8;
        assert_eq!(
            unsafe { spectra_interop_checked_i64_to_i8(127, &mut output) },
            SPECTRA_INTEROP_OK
        );
        assert_eq!(output, 127);
        assert_eq!(
            unsafe { spectra_interop_checked_i64_to_i8(128, &mut output) },
            SPECTRA_INTEROP_INVALID_ARGUMENT
        );
    }

    #[test]
    fn rust_helper_round_trips_npy_f64() {
        let path = std::env::temp_dir().join("spectra_interop_roundtrip.npy");
        let tensor = TensorF64::new(vec![1.0, 2.5, 3.5]);
        tensor.write_npy(&path).expect("write npy");
        let loaded = TensorF64::read_npy(&path).expect("read npy");
        let _ = fs::remove_file(&path);
        assert_eq!(loaded.as_slice(), &[1.0, 2.5, 3.5]);
        assert_eq!(loaded.sum(), 7.0);
    }

    #[test]
    fn c_abi_round_trips_npy_f64() {
        let path = std::env::temp_dir().join("spectra_interop_c_abi.npy");
        let path_text = path.to_string_lossy().to_string();
        let values = [4.0, 5.0, 6.0];
        let status = unsafe {
            spectra_interop_npy_write_f64(
                path_text.as_ptr() as *const c_char,
                path_text.len(),
                values.as_ptr(),
                values.len(),
            )
        };
        assert_eq!(status, SPECTRA_INTEROP_OK);
        let loaded = unsafe {
            spectra_interop_npy_read_f64(path_text.as_ptr() as *const c_char, path_text.len())
        };
        assert_eq!(loaded.len, 3);
        let sum = unsafe { spectra_interop_tensor_f64_sum(loaded.data, loaded.len) };
        unsafe { spectra_interop_f64_array_free(loaded) };
        let _ = fs::remove_file(&path);
        assert_eq!(sum, 15.0);
    }

    #[test]
    fn read_rejects_invalid_npy_magic() {
        let path = std::env::temp_dir().join("spectra_interop_invalid_magic.npy");
        fs::write(&path, b"not numpy").expect("write invalid npy");
        let error = read_npy_f64(&path).expect_err("invalid magic should fail");
        let _ = fs::remove_file(&path);
        assert!(matches!(error, InteropError::Format(_)));
    }

    #[test]
    fn c_abi_rejects_null_path() {
        let status = unsafe { spectra_interop_npy_write_f64(ptr::null(), 0, ptr::null(), 0) };
        assert_eq!(status, SPECTRA_INTEROP_INVALID_ARGUMENT);

        let loaded = unsafe { spectra_interop_npy_read_f64(ptr::null(), 0) };
        assert!(loaded.data.is_null());
        assert_eq!(loaded.len, 0);
    }
}
