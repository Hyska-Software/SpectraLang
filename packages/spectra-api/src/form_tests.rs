#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_urlencoded_plus_arrays_and_nested_fields() {
        let form = Form::parse(
            "name=Ada+Lovelace&age=36&active=on&tags[]=math&tags[]=api&profile[city]=London",
        )
        .expect("form parses");
        assert_eq!(form.len(), 5);
        assert_eq!(form.first("name"), Some("Ada Lovelace"));
        assert_eq!(form.values("tags"), ["math".to_string(), "api".to_string()]);
        assert_eq!(form.first("profile.city"), Some("London"));
        assert_eq!(form.int("age", 0), Ok(36));
        assert_eq!(form.bool("active", 0), Ok(true));
    }

    #[test]
    fn rejects_malformed_percent_utf8_control_and_keys() {
        let bad_hex = Form::parse("name=%GG").expect_err("bad percent rejected");
        assert_eq!(bad_hex.kind, FormParseErrorKind::InvalidPercentEncoding);
        let short = Form::parse("name=%A").expect_err("short percent rejected");
        assert_eq!(short.kind, FormParseErrorKind::InvalidPercentEncoding);
        let control = Form::parse("name=bad\nvalue").expect_err("control rejected");
        assert_eq!(control.kind, FormParseErrorKind::ControlCharacter);
        let key = Form::parse("profile[name=ada").expect_err("bad key rejected");
        assert_eq!(key.kind, FormParseErrorKind::MalformedKey);
    }

    #[test]
    fn binds_schema_arrays_nested_fields_and_duplicate_scalar_errors() {
        let form =
            Form::parse("name=Ada&age=36&active=yes&tags[]=math&tags[]=api&profile[city]=London")
                .expect("form");
        let schema = FormSchema::new()
            .with_field("name", FormValueType::String, true, false)
            .expect("name")
            .with_field("age", FormValueType::Int, true, false)
            .expect("age")
            .with_field("active", FormValueType::Bool, true, false)
            .expect("active")
            .with_field("tags", FormValueType::String, false, true)
            .expect("tags")
            .with_field("profile.city", FormValueType::String, true, false)
            .expect("city");
        let binding = schema.bind(&form);
        assert!(binding.ok());
        assert_eq!(binding.get("name", 0), Some("Ada"));
        assert_eq!(binding.int("age", 0), Ok(36));
        assert_eq!(binding.bool("active", 0), Ok(true));
        assert_eq!(binding.count("tags"), 2);
        assert_eq!(binding.get("profile.city", 0), Some("London"));

        let duplicate = Form::parse("name=Ada&name=Grace&age=36&active=true&profile[city]=London")
            .expect("duplicate form");
        let failed = schema.bind(&duplicate);
        assert!(!failed.ok());
        assert!(failed.error_message().contains("duplicate scalar"));
        assert!(failed.error_message().contains("name"));
    }

    #[test]
    fn binding_reports_missing_required_and_type_mismatch_fields() {
        let schema = FormSchema::new()
            .with_field("email", FormValueType::String, true, false)
            .expect("email")
            .with_field("age", FormValueType::Int, false, false)
            .expect("age");

        let missing = schema.bind(&Form::parse("age=42").expect("missing"));
        assert!(!missing.ok());
        assert!(missing.error_message().contains("email"));

        let mismatch = schema.bind(&Form::parse("email=a%40b.test&age=old").expect("mismatch"));
        assert!(!mismatch.ok());
        assert!(mismatch.error_message().contains("age"));
        assert!(mismatch.error_message().contains("expected int"));
    }
}
