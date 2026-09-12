// `spectralang docs` — the language reference of the running binary.
//
// The reference text, its sha256 and the crate version are embedded at build
// time (tools/spectra-cli/build.rs), so the command never serves a checkout's
// copy of the docs from a stale binary.

include!(concat!(env!("OUT_DIR"), "/docs_reference_generated.rs"));

/// Options for `docs`.
#[derive(Debug)]
struct DocsOptions {
    json: bool,
    section: Option<String>,
}

#[derive(Serialize)]
struct DocsReportJson {
    schema: &'static str,
    success: bool,
    version: &'static str,
    reference_sha256: &'static str,
    sections: Vec<DocsSectionJson>,
}

#[derive(Serialize)]
struct DocsSectionJson {
    heading: String,
    slug: String,
    body: String,
}

#[derive(Serialize)]
struct DocsFailureJson {
    schema: &'static str,
    success: bool,
    error: String,
    available: Vec<String>,
}

const DOCS_SCHEMA: &str = "spectralang.docs.v1";

/// Aliases for common section nicknames checked before fuzzy heading matching.
const DOCS_SECTION_ALIASES: &[(&str, &str)] = &[
    ("stdlib", "Standard Library"),
    ("cli", "CLI Reference"),
    ("errors", "Common Errors"),
    ("keywords", "Reserved Keywords"),
    ("naming", "Naming Conventions"),
    ("packages", "Package Manager Baseline"),
    ("interop", "Interop Baseline"),
    ("examples", "Complete Working Examples"),
];

fn parse_docs_invocation<I>(args: &mut std::iter::Peekable<I>) -> CliResult<DocsOptions>
where
    I: Iterator<Item = String>,
{
    let mut json = false;
    let mut section: Option<String> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--section" => {
                section = Some(
                    args.next()
                        .ok_or_else(|| usage_error("Missing name after --section."))?,
                );
            }
            "--help" | "-h" => {
                return Err(usage_error(&format!(
                    "Use '{} help docs' for docs command help.",
                    program_name()
                )));
            }
            other => {
                return Err(usage_error(&format!("Unknown docs option '{other}'.")));
            }
        }
    }

    Ok(DocsOptions { json, section })
}

/// Split the embedded reference into `## `-delimited sections.
fn reference_sections(text: &'static str) -> Vec<(&'static str, &'static str)> {
    let mut sections: Vec<(&'static str, &'static str)> = Vec::new();
    let mut heading_start: Option<usize> = None;
    let mut heading_end = 0usize;
    let mut body_start = 0usize;
    let mut line_start = 0usize;

    loop {
        let line_end = text[line_start..]
            .find('\n')
            .map(|offset| line_start + offset)
            .unwrap_or(text.len());
        let line = &text[line_start..line_end];

        if let Some(rest) = line.strip_prefix("## ") {
            if let Some(start) = heading_start {
                sections.push((&text[start..heading_end], &text[body_start..line_start]));
            }
            heading_start = Some(line_start + 3);
            heading_end = line_end;
            body_start = (line_end + 1).min(text.len());
            let _ = rest;
        }

        if line_end >= text.len() {
            break;
        }
        line_start = line_end + 1;
    }

    if let Some(start) = heading_start {
        sections.push((&text[start..heading_end], &text[body_start..]));
    }
    sections
}

fn docs_slug(heading: &str) -> String {
    let mut slug = String::with_capacity(heading.len());
    let mut pending_dash = false;
    for character in heading.chars() {
        if character.is_ascii_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            pending_dash = false;
            slug.push(character.to_ascii_lowercase());
        } else {
            pending_dash = true;
        }
    }
    slug
}

fn docs_normalize(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut pending_space = false;
    for character in text.chars() {
        if character.is_ascii_alphanumeric() {
            if pending_space && !normalized.is_empty() {
                normalized.push(' ');
            }
            pending_space = false;
            normalized.push(character.to_ascii_lowercase());
        } else {
            pending_space = true;
        }
    }
    normalized
}

/// Resolve `--section` against slugs, aliases and normalized headings.
fn find_docs_section<'a>(
    sections: &[(&'a str, &'a str)],
    query: &str,
) -> Option<usize> {
    let query_slug = docs_slug(query);
    if let Some(index) = sections
        .iter()
        .position(|(heading, _)| docs_slug(heading) == query_slug)
    {
        return Some(index);
    }

    for (alias, target) in DOCS_SECTION_ALIASES {
        if alias == &query_slug {
            if let Some(index) = sections
                .iter()
                .position(|(heading, _)| heading.contains(target))
            {
                return Some(index);
            }
        }
    }

    let normalized_query = docs_normalize(query);
    if normalized_query.is_empty() {
        return None;
    }
    sections
        .iter()
        .position(|(heading, _)| docs_normalize(heading).contains(&normalized_query))
}

fn emit_docs_json<T: Serialize>(value: &T) -> CliResult<()> {
    let mut stdout = io::stdout();
    serde_json::to_writer(&mut stdout, value)
        .map_err(|error| CliError::io(format!("Failed to serialize docs output: {error}")))?;
    stdout
        .write_all(b"\n")
        .map_err(|error| CliError::io(format!("Failed to write docs output: {error}")))?;
    Ok(())
}

fn execute_docs(options: DocsOptions) -> CliResult<()> {
    let sections = reference_sections(REFERENCE_TEXT);

    let selected = match &options.section {
        Some(query) => match find_docs_section(&sections, query) {
            Some(index) => vec![sections[index]],
            None => {
                let available: Vec<String> = sections
                    .iter()
                    .map(|(heading, _)| (*heading).to_string())
                    .collect();
                if options.json {
                    emit_docs_json(&DocsFailureJson {
                        schema: DOCS_SCHEMA,
                        success: false,
                        error: format!("Unknown section '{query}'."),
                        available,
                    })?;
                }
                return Err(usage_error(&format!(
                    "Unknown docs section '{query}'. Use --json to list available sections."
                )));
            }
        },
        None => sections.clone(),
    };

    if options.json {
        let report = DocsReportJson {
            schema: DOCS_SCHEMA,
            success: true,
            version: REFERENCE_VERSION,
            reference_sha256: REFERENCE_SHA256,
            sections: selected
                .iter()
                .map(|(heading, body)| DocsSectionJson {
                    heading: (*heading).to_string(),
                    slug: docs_slug(heading),
                    body: body.trim().to_string(),
                })
                .collect(),
        };
        return emit_docs_json(&report);
    }

    let mut stdout = io::stdout();
    for (index, (heading, body)) in selected.iter().enumerate() {
        if index > 0 {
            stdout
                .write_all(b"\n")
                .map_err(|error| CliError::io(format!("Failed to write docs: {error}")))?;
        }
        writeln!(stdout, "## {heading}")
            .map_err(|error| CliError::io(format!("Failed to write docs: {error}")))?;
        write!(stdout, "{}", body.trim_start_matches('\n'))
            .map_err(|error| CliError::io(format!("Failed to write docs: {error}")))?;
    }
    Ok(())
}
