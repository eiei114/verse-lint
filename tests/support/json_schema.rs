//! A deliberately small JSON Schema validator for the pinned documents a
//! verification test must check: `docs/diagnostics.schema.json`,
//! `docs/coverage.schema.json` and the vendored SARIF 2.1.0 schema.
//!
//! The repository has no schema-validating dependency and this slice adds none, so
//! the module implements exactly the keyword subset the three pinned documents
//! use. It is shared through `#[path]` by the test targets that need it.
//!
//! Supported: `$ref` to `#` and `#/definitions/<name>`, `type` (single name or
//! array of names), `required`, `properties`, `additionalProperties` (boolean
//! or schema), `items`, `enum`, `const`, `pattern`, `minimum`, `maximum`,
//! `minLength`, `minItems`, `uniqueItems`, `anyOf` and `oneOf`. `$schema`, `$id`,
//! `title`, `description` and `default` are annotations and are ignored.
//!
//! Two deliberate limits:
//!
//! - `format` is not enforced. Draft-07 defines it as an optional annotation, and
//!   `scripts/verify-json.ps1` remains the authoritative check of real
//!   release-binary JSON with PowerShell's schema validator.
//! - `pattern` is enforced only for the patterns these documents actually pin. An
//!   unhandled pattern fails loudly instead of being skipped silently, because a
//!   silently skipped constraint is a check that looks green and proves nothing.
use serde_json::Value;

pub struct Schema {
    root: Value,
}

impl Schema {
    pub fn new(text: &str, name: &str) -> Self {
        Self {
            root: serde_json::from_str(text)
                .unwrap_or_else(|e| panic!("{name} is not valid JSON: {e}")),
        }
    }

    /// Validate one instance, returning the first violation with the instance's
    /// JSON path so a failure names the field that drifted.
    pub fn validate(&self, instance: &Value) -> Result<(), String> {
        self.check(&self.root, instance, "$")
    }

    /// Resolve the two pointer shapes the pinned documents use. Anything else is
    /// a document change this validator cannot read, and it says so.
    fn resolve(&self, reference: &str) -> &Value {
        if reference == "#" {
            return &self.root;
        }
        let name = reference.strip_prefix("#/definitions/").unwrap_or_else(|| {
            panic!("unsupported $ref {reference:?}: only #/definitions/<name> and # are pinned")
        });
        self.root
            .get("definitions")
            .and_then(|definitions| definitions.get(name))
            .unwrap_or_else(|| panic!("$ref {reference:?} names no definition"))
    }

    fn check(&self, schema: &Value, instance: &Value, path: &str) -> Result<(), String> {
        if let Some(boolean) = schema.as_bool() {
            return if boolean {
                Ok(())
            } else {
                Err(format!("{path}: this schema forbids any value"))
            };
        }
        let object = match schema.as_object() {
            Some(object) => object,
            None => panic!("pinned schema node {schema} is neither an object nor a boolean"),
        };

        // Draft-07 ignores every sibling keyword of `$ref`.
        if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
            return self.check(self.resolve(reference), instance, path);
        }
        if let Some(branches) = object.get("anyOf").and_then(Value::as_array) {
            self.branches(branches, instance, path, "anyOf", false)?;
        }
        if let Some(branches) = object.get("oneOf").and_then(Value::as_array) {
            self.branches(branches, instance, path, "oneOf", true)?;
        }
        if let Some(expected) = object.get("type") {
            self.check_type(expected, instance, path)?;
        }
        if let Some(allowed) = object.get("enum").and_then(Value::as_array)
            && !allowed.contains(instance)
        {
            return Err(format!("{path}: {instance} is not one of {allowed:?}"));
        }
        if let Some(expected) = object.get("const")
            && expected != instance
        {
            return Err(format!(
                "{path}: {instance} is not the pinned value {expected}"
            ));
        }
        if let (Some(pattern), Some(text)) = (
            object.get("pattern").and_then(Value::as_str),
            instance.as_str(),
        ) && !pattern_matches(pattern, text)
        {
            return Err(format!(
                "{path}: {text:?} does not match the pinned pattern {pattern:?}"
            ));
        }
        if let (Some(minimum), Some(number)) = (
            object.get("minimum").and_then(Value::as_f64),
            instance.as_f64(),
        ) && number < minimum
        {
            return Err(format!("{path}: {number} is below the minimum {minimum}"));
        }
        if let (Some(maximum), Some(number)) = (
            object.get("maximum").and_then(Value::as_f64),
            instance.as_f64(),
        ) && number > maximum
        {
            return Err(format!("{path}: {number} exceeds the maximum {maximum}"));
        }
        if let (Some(minimum), Some(text)) = (
            object.get("minLength").and_then(Value::as_u64),
            instance.as_str(),
        ) && (text.chars().count() as u64) < minimum
        {
            return Err(format!(
                "{path}: {text:?} is shorter than the minimum length {minimum}"
            ));
        }
        match instance {
            Value::Object(map) => self.check_object(object, map, path)?,
            Value::Array(items) => self.check_array(object, items, path)?,
            _ => {}
        }
        Ok(())
    }

    fn branches(
        &self,
        branches: &[Value],
        instance: &Value,
        path: &str,
        keyword: &str,
        exactly_one: bool,
    ) -> Result<(), String> {
        let mut passes = 0;
        let mut last = String::new();
        for branch in branches {
            match self.check(branch, instance, path) {
                Ok(()) => passes += 1,
                Err(error) => last = error,
            }
        }
        if if exactly_one {
            passes == 1
        } else {
            passes >= 1
        } {
            return Ok(());
        }
        Err(format!(
            "{path}: {keyword} matched {passes} of {} branches; last branch error: {last}",
            branches.len()
        ))
    }

    fn check_type(&self, expected: &Value, instance: &Value, path: &str) -> Result<(), String> {
        let matches = |name: &str| match name {
            "object" => instance.is_object(),
            "array" => instance.is_array(),
            "string" => instance.is_string(),
            "boolean" => instance.is_boolean(),
            "null" => instance.is_null(),
            "number" => instance.is_number(),
            "integer" => instance.as_u64().is_some() || instance.as_i64().is_some(),
            other => panic!("pinned schema uses unsupported type {other:?}"),
        };
        let accepted = match expected {
            Value::String(name) => matches(name),
            Value::Array(names) => names.iter().filter_map(Value::as_str).any(matches),
            other => panic!("pinned schema has a malformed type {other}"),
        };
        if accepted {
            Ok(())
        } else {
            Err(format!("{path}: {instance} is not of type {expected}"))
        }
    }

    fn check_object(
        &self,
        schema: &serde_json::Map<String, Value>,
        map: &serde_json::Map<String, Value>,
        path: &str,
    ) -> Result<(), String> {
        if let Some(required) = schema.get("required").and_then(Value::as_array) {
            for name in required.iter().filter_map(Value::as_str) {
                if !map.contains_key(name) {
                    return Err(format!("{path}: required property {name:?} is missing"));
                }
            }
        }
        let properties = schema.get("properties").and_then(Value::as_object);
        for (name, value) in map {
            let child = format!("{path}.{name}");
            if let Some(property) = properties.and_then(|properties| properties.get(name)) {
                self.check(property, value, &child)?;
                continue;
            }
            match schema.get("additionalProperties") {
                None | Some(Value::Bool(true)) => {}
                Some(Value::Bool(false)) => {
                    return Err(format!("{path}: unexpected property {name:?}"));
                }
                Some(extra) => self.check(extra, value, &child)?,
            }
        }
        Ok(())
    }

    fn check_array(
        &self,
        schema: &serde_json::Map<String, Value>,
        items: &[Value],
        path: &str,
    ) -> Result<(), String> {
        if let Some(minimum) = schema.get("minItems").and_then(Value::as_u64)
            && (items.len() as u64) < minimum
        {
            return Err(format!(
                "{path}: {} item(s) are fewer than the minimum {minimum}",
                items.len()
            ));
        }
        if let Some(Value::Bool(true)) = schema.get("uniqueItems") {
            for (index, item) in items.iter().enumerate() {
                if items[..index].contains(item) {
                    return Err(format!("{path}: item {index} duplicates an earlier entry"));
                }
            }
        }
        if let Some(item_schema) = schema.get("items") {
            for (index, item) in items.iter().enumerate() {
                self.check(item_schema, item, &format!("{path}[{index}]"))?;
            }
        }
        Ok(())
    }
}

/// The `pattern` values the pinned documents actually use, matched by shape
/// rather than by string equality so an equivalent rewrite keeps working.
fn pattern_matches(pattern: &str, text: &str) -> bool {
    match pattern {
        // SARIF `guid`.
        "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-5][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$" => {
            is_guid(text)
        }
        // SARIF `mimeType`.
        "[^/]+/.+" => matches!(text.split_once('/'), Some((kind, subtype))
            if !kind.is_empty() && !subtype.is_empty()),
        // SARIF `dottedQuadFileVersion`.
        "[0-9]+(\\.[0-9]+){3}" => {
            let parts: Vec<&str> = text.split('.').collect();
            parts.len() == 4
                && parts
                    .iter()
                    .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
        }
        // SARIF `language`, e.g. `en-US`.
        "^[a-zA-Z]{2}|^[a-zA-Z]{2}-[a-zA-Z]{2}]?$" => {
            text.len() == 2 && text.bytes().all(|b| b.is_ascii_alphabetic())
                || text.len() == 5 && text.bytes().all(|b| b.is_ascii_alphabetic() || b == b'-')
        }
        // `docs/diagnostics.schema.json` and `docs/coverage.schema.json` rule IDs.
        "^V[0-9]{4}$" => {
            text.len() == 5
                && text.starts_with('V')
                && text[1..].bytes().all(|b| b.is_ascii_digit())
        }
        other => panic!(
            "unhandled pinned schema pattern {other:?}: teach tests/support/json_schema.rs the new shape instead of skipping it"
        ),
    }
}

fn is_guid(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    if parts.len() != 5 {
        return false;
    }
    let hex = |part: &str, min: usize, max: usize| {
        part.len() >= min
            && part.len() <= max
            && part.bytes().all(|b| b.is_ascii_hexdigit())
            && part.as_bytes()[0] != b'x'
    };
    if !(hex(parts[0], 8, 8)
        && hex(parts[1], 4, 4)
        && hex(parts[2], 4, 4)
        && hex(parts[3], 4, 4)
        && hex(parts[4], 12, 12))
    {
        return false;
    }
    matches!(parts[2].as_bytes()[0], b'1'..=b'5')
        && matches!(
            parts[3].as_bytes()[0],
            b'8' | b'9' | b'a' | b'b' | b'A' | b'B'
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_validator_accepts_the_documented_shape_and_rejects_a_drifted_one() {
        let schema = Schema::new(
            r##"{
                "type": "object",
                "additionalProperties": false,
                "required": ["schemaVersion", "files"],
                "properties": {
                    "schemaVersion": {"const": 1},
                    "files": {"type": "array", "minItems": 1, "uniqueItems": true,
                        "items": {"$ref": "#/definitions/file"}}
                },
                "definitions": {
                    "file": {"type": "object", "additionalProperties": false,
                        "required": ["path", "isComplete"],
                        "properties": {"path": {"type": "string", "minLength": 1},
                                       "isComplete": {"type": "boolean"}}}
                }
            }"##,
            "test schema",
        );
        let ok = serde_json::json!({
            "schemaVersion": 1,
            "files": [{"path": "a.verse", "isComplete": true}]
        });
        schema.validate(&ok).expect("documented shape");

        for bad in [
            serde_json::json!({"schemaVersion": 2, "files": [{"path": "a", "isComplete": true}]}),
            serde_json::json!({"schemaVersion": 1, "files": []}),
            serde_json::json!({"schemaVersion": 1, "files": [
                {"path": "a", "isComplete": true}, {"path": "a", "isComplete": true}]}),
            serde_json::json!({"schemaVersion": 1, "files": [{"path": "", "isComplete": true}]}),
            serde_json::json!({"schemaVersion": 1, "files": [{"path": "a"}]}),
            serde_json::json!({"schemaVersion": 1, "files": [{"path": "a", "isComplete": true}],
                               "extra": 1}),
            serde_json::json!({"schemaVersion": 1, "files": [{"path": 1, "isComplete": true}]}),
        ] {
            let error = schema
                .validate(&bad)
                .expect_err("a drifted document must be rejected");
            assert!(!error.is_empty());
        }
    }

    #[test]
    fn one_of_requires_exactly_one_branch_and_any_of_requires_one() {
        let schema = Schema::new(
            r#"{"type": "object", "oneOf": [{"required": ["a"]}, {"required": ["b"]}]}"#,
            "test schema",
        );
        schema
            .validate(&serde_json::json!({"a": 1}))
            .expect("one branch");
        assert!(schema.validate(&serde_json::json!({})).is_err());
        assert!(
            schema
                .validate(&serde_json::json!({"a": 1, "b": 2}))
                .is_err(),
            "oneOf must reject a two-branch match"
        );
        let any = Schema::new(
            r#"{"type": "object", "anyOf": [{"required": ["a"]}, {"required": ["b"]}]}"#,
            "test schema",
        );
        any.validate(&serde_json::json!({"a": 1, "b": 2}))
            .expect("anyOf accepts a double match");
        assert!(any.validate(&serde_json::json!({})).is_err());
    }

    #[test]
    fn every_pinned_pattern_is_matched_by_kind() {
        assert!(pattern_matches("^V[0-9]{4}$", "V1001"));
        for bad in ["V100", "V10011", "v1001", "V100a", ""] {
            assert!(!pattern_matches("^V[0-9]{4}$", bad), "{bad:?}");
        }
        assert!(pattern_matches("[^/]+/.+", "text/plain"));
        assert!(!pattern_matches("[^/]+/.+", "/plain"));
        assert!(pattern_matches("[0-9]+(\\.[0-9]+){3}", "1.2.3.4"));
        assert!(!pattern_matches("[0-9]+(\\.[0-9]+){3}", "1.2.3"));
        assert!(pattern_matches(
            "^[a-zA-Z]{2}|^[a-zA-Z]{2}-[a-zA-Z]{2}]?$",
            "en-US"
        ));
        assert!(pattern_matches(
            "^[a-zA-Z]{2}|^[a-zA-Z]{2}-[a-zA-Z]{2}]?$",
            "en"
        ));
        assert!(!pattern_matches(
            "^[a-zA-Z]{2}|^[a-zA-Z]{2}-[a-zA-Z]{2}]?$",
            "e"
        ));
        assert!(is_guid("6b5433e3-7b52-4c03-bc4c-07f9468bf7cc"));
        assert!(!is_guid("6b5433e3-7b52-cc03-bc4c-07f9468bf7cc"));
        assert!(!is_guid("not-a-guid"));
    }

    #[test]
    #[should_panic(expected = "unhandled pinned schema pattern")]
    fn an_unhandled_pattern_fails_loudly_instead_of_being_skipped() {
        let schema = Schema::new(
            r#"{"type": "object", "properties": {"a": {"type": "string", "pattern": "^(x)+$"}}}"#,
            "test schema",
        );
        schema.validate(&serde_json::json!({"a": "y"})).ok();
    }
}
