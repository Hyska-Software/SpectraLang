use crate::{alloc_spectra_string, read_args, read_spectra_string, write_result};
use ring::{hmac, rand, signature};
use serde_json::Value;
use spectra_runtime::ffi::{SpectraHostCallContext, HOST_STATUS_INVALID_ARGUMENT};
use std::time::{SystemTime, UNIX_EPOCH};

const BASE64URL_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
const MIN_HS256_KEY_BYTES: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JwtAlgorithm {
    Hs256,
    Rs256,
    Es256,
}

impl JwtAlgorithm {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_uppercase().as_str() {
            "HS256" => Some(Self::Hs256),
            "RS256" => Some(Self::Rs256),
            "ES256" => Some(Self::Es256),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hs256 => "HS256",
            Self::Rs256 => "RS256",
            Self::Es256 => "ES256",
        }
    }
}

fn base64url_encode(input: &[u8]) -> String {
    let mut output = String::with_capacity((input.len() * 4).div_ceil(3));
    for chunk in input.chunks(3) {
        let first = chunk[0];
        output.push(BASE64URL_ALPHABET[(first >> 2) as usize] as char);
        let second = if chunk.len() > 1 { chunk[1] } else { 0 };
        output.push(BASE64URL_ALPHABET[((first & 0x03) << 4 | second >> 4) as usize] as char);
        if chunk.len() > 1 {
            let third = if chunk.len() > 2 { chunk[2] } else { 0 };
            output.push(BASE64URL_ALPHABET[((second & 0x0f) << 2 | third >> 6) as usize] as char);
            if chunk.len() > 2 {
                output.push(BASE64URL_ALPHABET[(third & 0x3f) as usize] as char);
            }
        }
    }
    output
}

fn base64_value(value: u8) -> Option<u8> {
    match value {
        b'A'..=b'Z' => Some(value - b'A'),
        b'a'..=b'z' => Some(value - b'a' + 26),
        b'0'..=b'9' => Some(value - b'0' + 52),
        b'-' | b'+' => Some(62),
        b'_' | b'/' => Some(63),
        _ => None,
    }
}

fn base64url_decode(input: &str) -> Option<Vec<u8>> {
    let first_padding = input.find('=');
    let (encoded, padding) = match first_padding {
        Some(index) => {
            let suffix = &input[index..];
            if suffix.bytes().any(|byte| byte != b'=') || suffix.len() > 2 {
                return None;
            }
            (&input[..index], suffix.len())
        }
        None => (input, 0),
    };
    if encoded.len() % 4 == 1 {
        return None;
    }
    if padding != 0 {
        let remainder = encoded.len() % 4;
        if (padding == 1 && remainder != 3) || (padding == 2 && remainder != 2) {
            return None;
        }
    }

    let mut output = Vec::with_capacity(encoded.len() * 3 / 4);
    for (chunk_index, chunk) in encoded.as_bytes().chunks(4).enumerate() {
        let values: Vec<u8> = chunk
            .iter()
            .copied()
            .map(base64_value)
            .collect::<Option<Vec<_>>>()?;
        let is_last = chunk_index + 1 == encoded.len().div_ceil(4);
        match values.as_slice() {
            [first, second] => {
                if !is_last || second & 0x0f != 0 {
                    return None;
                }
                output.push(first << 2 | second >> 4);
            }
            [first, second, third] => {
                if !is_last || third & 0x03 != 0 {
                    return None;
                }
                output.push(first << 2 | second >> 4);
                output.push(second << 4 | third >> 2);
            }
            [first, second, third, fourth] => {
                output.push(first << 2 | second >> 4);
                output.push(second << 4 | third >> 2);
                output.push(third << 6 | fourth);
            }
            _ => return None,
        }
    }
    Some(output)
}

fn decode_key(value: &str) -> Option<Vec<u8>> {
    let value = value.trim();
    if value.starts_with("-----BEGIN ") {
        let body: String = value
            .lines()
            .filter(|line| !line.starts_with("-----BEGIN ") && !line.starts_with("-----END "))
            .flat_map(str::chars)
            .filter(|character| !character.is_ascii_whitespace())
            .collect();
        base64url_decode(&body)
    } else {
        base64url_decode(value)
    }
}

fn sign_bytes(algorithm: JwtAlgorithm, key: &str, input: &[u8]) -> Option<Vec<u8>> {
    match algorithm {
        JwtAlgorithm::Hs256 => {
            if key.len() < MIN_HS256_KEY_BYTES {
                return None;
            }
            let signing_key = hmac::Key::new(hmac::HMAC_SHA256, key.as_bytes());
            Some(hmac::sign(&signing_key, input).as_ref().to_vec())
        }
        JwtAlgorithm::Rs256 => {
            let private_key = decode_key(key)?;
            let key_pair = signature::RsaKeyPair::from_pkcs8(&private_key)
                .or_else(|_| signature::RsaKeyPair::from_der(&private_key))
                .ok()?;
            let rng = rand::SystemRandom::new();
            let mut output = vec![0; key_pair.public().modulus_len()];
            key_pair
                .sign(&signature::RSA_PKCS1_SHA256, &rng, input, &mut output)
                .ok()?;
            Some(output)
        }
        JwtAlgorithm::Es256 => {
            let private_key = decode_key(key)?;
            let rng = rand::SystemRandom::new();
            let key_pair = signature::EcdsaKeyPair::from_pkcs8(
                &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
                &private_key,
                &rng,
            )
            .ok()?;
            Some(key_pair.sign(&rng, input).ok()?.as_ref().to_vec())
        }
    }
}

fn verify_bytes(algorithm: JwtAlgorithm, key: &str, input: &[u8], signature_bytes: &[u8]) -> bool {
    match algorithm {
        JwtAlgorithm::Hs256 => {
            if key.len() < MIN_HS256_KEY_BYTES {
                return false;
            }
            let signing_key = hmac::Key::new(hmac::HMAC_SHA256, key.as_bytes());
            hmac::verify(&signing_key, input, signature_bytes).is_ok()
        }
        JwtAlgorithm::Rs256 => {
            let Some(public_key) = decode_key(key) else {
                return false;
            };
            signature::UnparsedPublicKey::new(&signature::RSA_PKCS1_2048_8192_SHA256, public_key)
                .verify(input, signature_bytes)
                .is_ok()
        }
        JwtAlgorithm::Es256 => {
            let Some(public_key) = decode_key(key) else {
                return false;
            };
            signature::UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_FIXED, public_key)
                .verify(input, signature_bytes)
                .is_ok()
        }
    }
}

fn numeric_date(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
}

fn validate_string_claim(claims: &serde_json::Map<String, Value>, name: &str) -> bool {
    claims
        .get(name)
        .is_none_or(|value| value.as_str().is_some_and(|value| !value.is_empty()))
}

fn validate_audience(claims: &serde_json::Map<String, Value>, expected: &str) -> bool {
    let Some(audience) = claims.get("aud") else {
        return expected.is_empty();
    };
    if expected.is_empty() {
        return audience.as_str().is_some_and(|value| !value.is_empty())
            || audience.as_array().is_some_and(|values| {
                !values.is_empty()
                    && values
                        .iter()
                        .all(|value| value.as_str().is_some_and(|value| !value.is_empty()))
            });
    }
    audience.as_str().is_some_and(|value| value == expected)
        || audience.as_array().is_some_and(|values| {
            values
                .iter()
                .any(|value| value.as_str().is_some_and(|value| value == expected))
        })
}

fn validate_claims(
    claims: &Value,
    expected_issuer: &str,
    expected_audience: &str,
    now_ms: i64,
) -> bool {
    let Some(claims) = claims.as_object() else {
        return false;
    };
    let now_seconds = if now_ms == 0 {
        let Ok(duration) = SystemTime::now().duration_since(UNIX_EPOCH) else {
            return false;
        };
        duration.as_secs().min(i64::MAX as u64) as i64
    } else if now_ms > 0 {
        now_ms / 1_000
    } else {
        return false;
    };

    if let Some(exp) = claims.get("exp") {
        let Some(exp) = numeric_date(exp) else {
            return false;
        };
        if now_seconds >= exp {
            return false;
        }
    }
    if let Some(nbf) = claims.get("nbf") {
        let Some(nbf) = numeric_date(nbf) else {
            return false;
        };
        if now_seconds < nbf {
            return false;
        }
    }
    if !validate_string_claim(claims, "sub") || !validate_string_claim(claims, "jti") {
        return false;
    }
    if let Some(issuer) = claims.get("iss") {
        let Some(issuer) = issuer.as_str() else {
            return false;
        };
        if !expected_issuer.is_empty() && issuer != expected_issuer {
            return false;
        }
    } else if !expected_issuer.is_empty() {
        return false;
    }
    validate_audience(claims, expected_audience)
}

pub fn sign(algorithm: JwtAlgorithm, key: &str, claims_json: &str) -> Option<String> {
    let claims: Value = serde_json::from_str(claims_json).ok()?;
    if !claims.is_object() {
        return None;
    }
    let header = serde_json::json!({
        "alg": algorithm.as_str(),
        "typ": "JWT",
    });
    let header_segment = base64url_encode(&serde_json::to_vec(&header).ok()?);
    let claims_segment = base64url_encode(&serde_json::to_vec(&claims).ok()?);
    let signing_input = format!("{header_segment}.{claims_segment}");
    let signature = sign_bytes(algorithm, key, signing_input.as_bytes())?;
    Some(format!("{signing_input}.{}", base64url_encode(&signature)))
}

pub fn verify(
    token: &str,
    algorithm: JwtAlgorithm,
    key: &str,
    expected_issuer: &str,
    expected_audience: &str,
    now_ms: i64,
) -> bool {
    let mut parts = token.split('.');
    let Some(header_segment) = parts.next() else {
        return false;
    };
    let Some(claims_segment) = parts.next() else {
        return false;
    };
    let Some(signature_segment) = parts.next() else {
        return false;
    };
    if parts.next().is_some()
        || header_segment.is_empty()
        || claims_segment.is_empty()
        || signature_segment.is_empty()
    {
        return false;
    }
    let Some(header_bytes) = base64url_decode(header_segment) else {
        return false;
    };
    let Some(claims_bytes) = base64url_decode(claims_segment) else {
        return false;
    };
    let Some(signature_bytes) = base64url_decode(signature_segment) else {
        return false;
    };
    let Ok(header) = serde_json::from_slice::<Value>(&header_bytes) else {
        return false;
    };
    if header.get("alg").and_then(Value::as_str) != Some(algorithm.as_str()) {
        return false;
    }
    let Ok(claims) = serde_json::from_slice::<Value>(&claims_bytes) else {
        return false;
    };
    if !validate_claims(&claims, expected_issuer, expected_audience, now_ms) {
        return false;
    }
    let signing_input = format!("{header_segment}.{claims_segment}");
    verify_bytes(algorithm, key, signing_input.as_bytes(), &signature_bytes)
}

pub extern "C" fn jwt_sign(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let (Some(algorithm), Some(key), Some(claims)) = (
        read_spectra_string(args[0]),
        read_spectra_string(args[1]),
        read_spectra_string(args[2]),
    ) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(algorithm) = JwtAlgorithm::parse(&algorithm) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(token) = sign(algorithm, &key, &claims) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, alloc_spectra_string(&token))
}

pub extern "C" fn jwt_verify(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 6) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let (Some(token), Some(algorithm), Some(key), Some(issuer), Some(audience)) = (
        read_spectra_string(args[0]),
        read_spectra_string(args[1]),
        read_spectra_string(args[2]),
        read_spectra_string(args[3]),
        read_spectra_string(args[4]),
    ) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(algorithm) = JwtAlgorithm::parse(&algorithm) else {
        return write_result(ctx, 0);
    };
    let now_ms = args[5];
    write_result(
        ctx,
        if verify(&token, algorithm, &key, &issuer, &audience, now_ms) {
            1
        } else {
            0
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::KeyPair;

    const HS_KEY: &str = "spectra-jwt-test-secret-with-at-least-32-bytes";
    const RSA_PRIVATE_KEY: &str = "MIIEpAIBAAKCAQEAzqgEdTJMHcg0eCeBjaWLrAadNBnGFKbqGsajtRDc1yzFFpVJBen++QjUXhMAat8n1Gen2DwRHRpd8V7yk3ca77kgAypbuYn45PXhsFCT0/Ew+YTAencqNoP03G+yipaBWzISPM3ROVTxnVuLJKED53GjTDKHVcZe1k4ZJP/QTTCyFCzCYvbgBI/vbbxlLyFHnqHEsdZtKPTUbvcYXjkMv6LgI4BYLzGIu5TrvwXTFIegmv8B/LtM1L/R8Kgzs4wRgTyENgu1PH1EgQMcQLrYcTu2uDXLCAmO0VujHuS6coqMjhD3KU4bQWO3ruVyd7/Ygab51D4CxpJao6BD+3+3jQIDJgRFAoIBAAmXY0xHfBoDnUTIELKqo8eGKwuI03CCcuHhX2b8k4lwn4oR8+pqWvfv+i0BwYnFDw1by+P6Jy5Wz8Sk4dOIqdzWXfhiiQJVbItrtqZBcJtaNd0mIsc9RkC/oTWdDnbh8hn44z65vQtZ7BmOsvzKrgNGvYtAHhLjxny2KVacGFouDzWi90FkTBzKXrsTnXeomilT/F4wBIwOYZ8HyNIdHla4rwcZPQ/fP0nNSfLvMTi1E4hi8UcL0tFuNKK553d6bIyNTLlLTotdYWzVOTdT57DzHMfaVZuo6Y2IiRTjNHc7r0mK2I2WMetf4y5TpBRb8LpUi/KwpQxj9nsU45ijSw0CgYEA82ThbvEgF+yVsZIwjAHgh87mGatQpdU3zAGEHckrMLzvDZ8sa71dwQvfW59sNUpPnyEFIMqnK09cNrjTPxAyTFWVYUGJHkW4S0n1nqW/rG/6OJAKylCZr80C9qglfEHOW7LkFTgytcIvkes4n6IDXDz5szdFMcSDyzDOsAclmx0CgYEA2VwJlfq9/LzP5j4PMmL4BoaatXHheT6XI0y7m9S2hyp2lTiZVc9s5yRTRaXfgCH32VGVY6+8Jmf1MR+tCT3iwCzQaRCbYw1o4792f4p4imrderGZ8tj2pAt8GRDZ2rUqyA0NMzqsqzIakwnciE3dTbY3oMERWuPAjvpoP5nrczECgYEA1Pfvn5vpR7qdGzvOWeVgiDmh5GRVPhttET0PY2dYu7RzqJ+ZSYNurUC28xTu46wiRNe283noPzDhd4OtaNUIaJeInAUcJuFVikoiC/wkKZWGBkS116PvUTrGErnGwKICG7a5zefb0h/lhYdGx5Vj6bq30GtDqrQ6Clyvq0UZpmECgYA9sjhvF08uo+9La9FgF0nOLWr6i+NfBRF4Yh8WojrTbroDwHMTY4kkGWnluH7bD8vPGgvW4a7pe64fLZeqvhmxfb59lJLNtooIl/VyNQ6EbGaWYNyXjFBo2lmFJPyooTY1jT5fj2rVz3jZCJyT9HMYkWLOD4xJAqGZArYzs+aSbQKBgQDdyXEYPc80UMQ+Broq8yN57t6y1nhRP7cGt1oAYJgVQEH0sJ5r44XUsl2A7CQciZ5KmGoXsKEh2quRoeT8WhgCpwdN8/s/dmHw4cl3meNtId6TfMQglYXbMKVq8KIo4AEDbteSYl5TaM4QFXSi6XZ/BzOJSfCv3zWM7NGMbW8/VQ==";
    const RSA_PUBLIC_KEY: &str = "MIIBCgKCAQEAzqgEdTJMHcg0eCeBjaWLrAadNBnGFKbqGsajtRDc1yzFFpVJBen++QjUXhMAat8n1Gen2DwRHRpd8V7yk3ca77kgAypbuYn45PXhsFCT0/Ew+YTAencqNoP03G+yipaBWzISPM3ROVTxnVuLJKED53GjTDKHVcZe1k4ZJP/QTTCyFCzCYvbgBI/vbbxlLyFHnqHEsdZtKPTUbvcYXjkMv6LgI4BYLzGIu5TrvwXTFIegmv8B/LtM1L/R8Kgzs4wRgTyENgu1PH1EgQMcQLrYcTu2uDXLCAmO0VujHuS6coqMjhD3KU4bQWO3ruVyd7/Ygab51D4CxpJao6BD+3+3jQIDJgRF";

    #[test]
    fn jwt_hs256_validates_claims_and_rejects_tampering() {
        let token = sign(
            JwtAlgorithm::Hs256,
            HS_KEY,
            r#"{"exp":1700000100,"nbf":1699999900,"iss":"issuer","aud":"api","sub":"user-1","jti":"token-1"}"#,
        )
        .expect("HS256 token");
        assert!(verify(
            &token,
            JwtAlgorithm::Hs256,
            HS_KEY,
            "issuer",
            "api",
            1_700_000_000_000,
        ));
        assert!(!verify(
            &token,
            JwtAlgorithm::Hs256,
            HS_KEY,
            "wrong-issuer",
            "api",
            1_700_000_000_000,
        ));
        assert!(!verify(
            &token,
            JwtAlgorithm::Hs256,
            HS_KEY,
            "issuer",
            "api",
            1_700_000_101_000,
        ));
        let mut tampered = token.clone().into_bytes();
        let payload_index = tampered.iter().position(|byte| *byte == b'.').unwrap() + 2;
        tampered[payload_index] = if tampered[payload_index] == b'a' {
            b'b'
        } else {
            b'a'
        };
        assert!(!verify(
            &String::from_utf8(tampered).unwrap(),
            JwtAlgorithm::Hs256,
            HS_KEY,
            "issuer",
            "api",
            1_700_000_000_000,
        ));
    }

    #[test]
    fn jwt_rs256_signs_and_verifies_with_der_keys() {
        let token =
            sign(JwtAlgorithm::Rs256, RSA_PRIVATE_KEY, r#"{"iss":"issuer"}"#).expect("RS256 token");
        assert!(verify(
            &token,
            JwtAlgorithm::Rs256,
            RSA_PUBLIC_KEY,
            "issuer",
            "",
            1_700_000_000_000,
        ));
        assert!(!verify(
            &token,
            JwtAlgorithm::Rs256,
            RSA_PUBLIC_KEY,
            "other",
            "",
            1_700_000_000_000,
        ));
    }

    #[test]
    fn jwt_es256_signs_and_verifies_with_generated_p256_keys() {
        let rng = rand::SystemRandom::new();
        let private_key = signature::EcdsaKeyPair::generate_pkcs8(
            &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
            &rng,
        )
        .expect("P-256 key");
        let key_pair = signature::EcdsaKeyPair::from_pkcs8(
            &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
            private_key.as_ref(),
            &rng,
        )
        .expect("P-256 key pair");
        let private_key = base64url_encode(private_key.as_ref());
        let public_key = base64url_encode(key_pair.public_key().as_ref());
        let token =
            sign(JwtAlgorithm::Es256, &private_key, r#"{"sub":"user-1"}"#).expect("ES256 token");
        assert!(verify(
            &token,
            JwtAlgorithm::Es256,
            &public_key,
            "",
            "",
            1_700_000_000_000,
        ));
    }

    #[test]
    fn base64url_decoder_accepts_pem_and_rejects_noncanonical_tail_bits() {
        assert_eq!(base64url_decode("AQID"), Some(vec![1, 2, 3]));
        assert_eq!(base64url_decode("AQI="), Some(vec![1, 2]));
        assert_eq!(base64url_decode("AB"), None);
        let pem = format!(
            "-----BEGIN KEY-----\n{}\n-----END KEY-----",
            base64url_encode(b"key")
        );
        assert_eq!(decode_key(&pem), Some(b"key".to_vec()));
    }
}
