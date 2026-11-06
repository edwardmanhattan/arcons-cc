//! **Spec drift** — the pinned upstream contract still covers what this client models.
//!
//! `docs/spec/minos-api.yaml` is a snapshot of Minos at a named commit. A server release that makes
//! a field *required*, or drops one we still read, is a deliberate change here — never a silent
//! breakage at 03:00 on a wall.
//!
//! Crude on purpose: the YAML is parsed as **text**, with no YAML dependency, because this is a
//! tripwire rather than a validator. It reads the `required:` list and the property names of each
//! schema we consume and compares them with the field lists in `arcons_cc::data::wire`.
//!
//! **Fix the code, not this test.** If the contract genuinely changed meaning, re-vendor
//! `docs/spec/` at the new commit (see `docs/spec/README.md`) and say what changed.

use arcons_cc::data::wire;

fn spec() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/spec/minos-api.yaml");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read the pinned spec at {}: {e}", path.display()))
}

/// The lines of one schema block: it starts at `    Name:` and ends at the next line indented four
/// spaces or less that is not a comment or blank.
fn schema_block<'a>(yaml: &'a str, name: &str) -> Vec<&'a str> {
    let lines: Vec<&str> = yaml.lines().collect();
    let header = format!("    {name}:");
    let start = lines.iter().position(|line| *line == header).unwrap_or_else(|| {
        panic!("{name} is not in the pinned spec at four-space indent. If the schema was renamed upstream, re-vendor docs/spec/ and rename it in arcons_cc::data::wire too.")
    });
    let mut block = Vec::new();
    for line in lines[start + 1..].iter().copied() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            block.push(line);
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if indent <= 4 {
            break;
        }
        block.push(line);
    }
    block
}

/// The `required:` list of a schema block, inline (`required: [a, b]`) or as a block sequence.
fn required_fields(block: &[&str], name: &str) -> Vec<String> {
    let mut fields = Vec::new();
    for (index, line) in block.iter().enumerate() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("required:") else {
            continue;
        };
        let rest = rest.trim();
        if let Some(inline) = rest.strip_prefix('[') {
            let mut inline = inline.to_string();
            // An inline list may wrap: gather until the closing bracket.
            let mut cursor = index;
            while !inline.contains(']') {
                cursor += 1;
                let next = block
                    .get(cursor)
                    .unwrap_or_else(|| panic!("{name}: `required:` list never closes"));
                inline.push(' ');
                inline.push_str(next.trim());
            }
            let inside = inline
                .split_once(']')
                .map(|(inside, _)| inside)
                .unwrap_or(&inline);
            fields.extend(
                inside
                    .split(',')
                    .map(|field| field.trim().to_string())
                    .filter(|f| !f.is_empty()),
            );
            return fields;
        }
        // Block sequence: `- field` at deeper indentation, until one does not start with `-`.
        for line in &block[index + 1..] {
            let trimmed = line.trim_start();
            match trimmed.strip_prefix("- ") {
                Some(field) => fields.push(field.trim().to_string()),
                None => break,
            }
        }
        return fields;
    }
    Vec::new()
}

/// The property names of a schema block: `name:` at eight-space indent.
fn properties(block: &[&str]) -> Vec<String> {
    block
        .iter()
        .filter_map(|line| {
            let indent = line.len() - line.trim_start().len();
            if indent != 8 {
                return None;
            }
            let trimmed = line.trim_start();
            if trimmed.starts_with('#') || trimmed.starts_with('-') {
                return None;
            }
            trimmed
                .split_once(':')
                .map(|(name, _)| name.trim().to_string())
        })
        .filter(|name| !name.is_empty())
        .collect()
}

#[test]
fn the_pinned_spec_and_the_modelled_fields_agree() {
    let yaml = spec();
    for (schema, modelled) in wire::MODELLED {
        let block = schema_block(&yaml, schema);
        let required = required_fields(&block, schema);
        let properties = properties(&block);

        assert!(
            !properties.is_empty(),
            "the drift test parsed no properties for {schema}, so it proves nothing: the YAML layout changed and this parser needs to follow it"
        );
        assert!(
            !required.is_empty(),
            "the drift test parsed no required list for {schema}: every schema we consume is an object with at least one required field, so a reader that finds none has stopped reading"
        );

        // A required field we do not model is the failure this file exists for.
        for field in &required {
            assert!(
                modelled.contains(&field.as_str()),
                "INV drift: docs/spec/minos-api.yaml requires {schema}.{field}, and arcons_cc::data::wire::{schema} does not model it.\n  \
                 The server now guarantees this field, so the client must read it. Add it to the const in src/data.rs and to the decoder.\n  \
                 If the contract changed meaning, re-vendor docs/spec/ deliberately (docs/spec/README.md) and say what changed."
            );
        }

        // A property we model that the snapshot no longer has is the other half.
        for field in *modelled {
            assert!(
                properties.contains(&field.to_string()),
                "INV drift: {schema}.{field} is modelled in arcons_cc::data::wire, and docs/spec/minos-api.yaml no longer has that property.\n  \
                 Fix the code, not this test: either the field was renamed upstream (re-vendor and follow it) or we invented it."
            );
        }
    }
}

#[test]
fn the_drift_parser_actually_reads_the_snapshot() {
    // If these two go wrong, the test above would pass vacuously over an empty parse.
    let yaml = spec();
    let feed_fix = schema_block(&yaml, "FeedFix");
    let required = required_fields(&feed_fix, "FeedFix");
    for structural in [
        "latitude",
        "longitude",
        "recorded_at",
        "received_at",
        "age_seconds",
        "backfilled",
    ] {
        assert!(
            required.iter().any(|field| field == structural),
            "FeedFix.{structural} must be required in the snapshot: a fix without it is not drawable. Got {required:?}"
        );
    }
    let properties = properties(&feed_fix);
    for nullable in ["speed_kn", "speed_ms", "course_deg", "accuracy_m"] {
        assert!(
            properties.iter().any(|p| p == nullable) && !required.iter().any(|r| r == nullable),
            "FeedFix.{nullable} must be an OPTIONAL property: absence is not zero, and a required measurement would make every silent hull undecodable"
        );
    }
}
