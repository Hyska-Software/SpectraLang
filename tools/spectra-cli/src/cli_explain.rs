// `spectralang explain --json <code>` — repair-oriented diagnostics lookup.
//
// Descriptions come from the embedded copy of
// `docs/diagnostics/error-code-reference.md` (build.rs), so the CLI and the
// documentation cannot drift: one file, two renderings.

include!(concat!(env!("OUT_DIR"), "/error_codes_generated.rs"));

/// Options for `explain`.
#[derive(Debug)]
struct ExplainOptions {
    code: Option<String>,
    json: bool,
    list: bool,
}

#[derive(Debug, Clone)]
struct ExplainEntry {
    code: String,
    section: String,
    phase: String,
    description: String,
    fix: Option<String>,
}

#[derive(Serialize)]
struct ExplainReportJson {
    schema: &'static str,
    success: bool,
    code: String,
    section: String,
    phase: String,
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    fix: Option<String>,
    reference_sha256: &'static str,
}

#[derive(Serialize)]
struct ExplainFailureJson {
    schema: &'static str,
    success: bool,
    code: String,
    error: String,
    near_matches: Vec<String>,
    reference_sha256: &'static str,
}

#[derive(Serialize)]
struct ExplainListJson {
    schema: &'static str,
    success: bool,
    reference_sha256: &'static str,
    codes: Vec<String>,
}

const EXPLAIN_SCHEMA: &str = "spectralang.explain.v1";

fn parse_explain_invocation<I>(args: &mut std::iter::Peekable<I>) -> CliResult<ExplainOptions>
where
    I: Iterator<Item = String>,
{
    let mut json = false;
    let mut list = false;
    let mut code: Option<String> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--list" => list = true,
            "--help" | "-h" => {
                return Err(usage_error(&format!(
                    "Use '{} help explain' for explain command help.",
                    program_name()
                )));
            }
            other if other.starts_with('-') => {
                return Err(usage_error(&format!("Unknown explain option '{other}'.")));
            }
            other => {
                if code.is_some() {
                    return Err(usage_error("explain accepts a single error code."));
                }
                code = Some(other.to_string());
            }
        }
    }

    if !list && code.is_none() {
        return Err(usage_error("explain requires an error code (or --list)."));
    }

    Ok(ExplainOptions { code, json, list })
}

/// Parse markdown tables out of the embedded error-code reference.
///
/// Rows look like `| \`E004\` | semantic | meaning | hint/action |`; the
/// section a row lives in is tracked for context.
fn explain_entries(text: &'static str) -> Vec<ExplainEntry> {
    let mut entries: Vec<ExplainEntry> = Vec::new();
    let mut section = String::new();

    for line in text.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            section = heading.trim().to_string();
            continue;
        }
        let Some(rest) = line.strip_prefix('|') else {
            continue;
        };
        let cells: Vec<&str> = rest.split('|').map(str::trim).collect();
        if cells.len() < 4 {
            continue;
        }
        let raw_code = cells[0];
        let Some(code) = raw_code
            .strip_prefix('`')
            .and_then(|value| value.strip_suffix('`'))
        else {
            continue;
        };
        // Only single codes are explainable; ranges (`E2101-E2120`) and
        // non-code families (`lint(<rule>)`) are skipped.
        if code.is_empty()
            || code.contains('-')
            || !code
                .chars()
                .next()
                .is_some_and(|first| first.is_ascii_uppercase())
            || raw_code.contains('(')
        {
            continue;
        }
        let phase = cells[1];
        let description = cells[2];
        let fix = cells
            .get(3)
            .map(|value| value.trim())
            .filter(|value| !value.is_empty() && *value != description);

        if let Some(existing) = entries.iter_mut().find(|entry| entry.code == code) {
            // Prefer the row that documents a repair action when the first
            // occurrence did not.
            if existing.fix.is_none() && fix.is_some() {
                existing.section = section.clone();
                existing.phase = phase.to_string();
                existing.description = description.to_string();
                existing.fix = fix.map(str::to_string);
            }
            continue;
        }

        entries.push(ExplainEntry {
            code: code.to_string(),
            section: section.clone(),
            phase: phase.to_string(),
            description: description.to_string(),
            fix: fix.map(str::to_string),
        });
    }

    entries
}

fn levenshtein_distance(left: &str, right: &str) -> usize {
    let left: Vec<char> = left.chars().collect();
    let right: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    let mut current = vec![0usize; right.len() + 1];

    for (row, left_char) in left.iter().enumerate() {
        current[0] = row + 1;
        for (column, right_char) in right.iter().enumerate() {
            let cost = if left_char == right_char { 0 } else { 1 };
            current[column + 1] = (previous[column + 1] + 1)
                .min(current[column] + 1)
                .min(previous[column] + cost);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}

fn explain_near_matches(entries: &[ExplainEntry], query: &str) -> Vec<String> {
    let upper = query.to_ascii_uppercase();
    let mut matches: Vec<(usize, String)> = entries
        .iter()
        .filter_map(|entry| {
            let distance = levenshtein_distance(&upper, &entry.code);
            if distance <= 3 || entry.code.starts_with(&upper) {
                Some((distance, entry.code.clone()))
            } else {
                None
            }
        })
        .collect();
    matches.sort();
    matches.into_iter().map(|(_, code)| code).take(8).collect()
}

fn emit_explain_json<T: Serialize>(value: &T) -> CliResult<()> {
    let mut stdout = io::stdout();
    serde_json::to_writer(&mut stdout, value)
        .map_err(|error| CliError::io(format!("Failed to serialize explain output: {error}")))?;
    stdout
        .write_all(b"\n")
        .map_err(|error| CliError::io(format!("Failed to write explain output: {error}")))?;
    Ok(())
}

fn execute_explain(options: ExplainOptions) -> CliResult<()> {
    let entries = explain_entries(ERROR_CODES_TEXT);

    if options.list {
        if options.json {
            return emit_explain_json(&ExplainListJson {
                schema: EXPLAIN_SCHEMA,
                success: true,
                reference_sha256: ERROR_CODES_SHA256,
                codes: entries.iter().map(|entry| entry.code.clone()).collect(),
            });
        }
        for entry in &entries {
            println!("{}  {}", entry.code, entry.description);
        }
        return Ok(());
    }

    let query = options
        .code
        .as_deref()
        .expect("parse enforces a code unless --list is used")
        .to_ascii_uppercase();

    let Some(entry) = entries.iter().find(|entry| entry.code == query) else {
        let near_matches = explain_near_matches(&entries, &query);
        if options.json {
            emit_explain_json(&ExplainFailureJson {
                schema: EXPLAIN_SCHEMA,
                success: false,
                code: query.clone(),
                error: format!("Unknown error code '{query}'."),
                near_matches: near_matches.clone(),
                reference_sha256: ERROR_CODES_SHA256,
            })?;
        }
        let hint = if near_matches.is_empty() {
            String::new()
        } else {
            format!(" Did you mean {}?", near_matches.join(", "))
        };
        return Err(usage_error(&format!(
            "Unknown error code '{}'.{} Use --list to see every documented code.",
            query, hint
        )));
    };

    if options.json {
        return emit_explain_json(&ExplainReportJson {
            schema: EXPLAIN_SCHEMA,
            success: true,
            code: entry.code.clone(),
            section: entry.section.clone(),
            phase: entry.phase.clone(),
            description: entry.description.clone(),
            fix: entry.fix.clone(),
            reference_sha256: ERROR_CODES_SHA256,
        });
    }

    println!("{}: {}", entry.code, entry.description);
    if let Some(fix) = &entry.fix {
        println!("fix: {fix}");
    }
    println!("phase: {} | section: {}", entry.phase, entry.section);
    Ok(())
}
