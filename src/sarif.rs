//! SARIF 2.1.0: stable URIs, Unicode scalar columns, no suggested edit payloads.
use crate::{report::Report, rules::REGISTRY};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

const SCHEMA: &str =
    "https://docs.oasis-open.org/sarif/sarif/v2.1.0/os/schemas/sarif-schema-2.1.0.json";

fn encode(path: &str) -> String {
    let mut result = String::new();
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            result.push(char::from(byte));
        } else {
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            result.push('%');
            result.push(char::from(HEX[(byte >> 4) as usize]));
            result.push(char::from(HEX[(byte & 15) as usize]));
        }
    }
    result
}

fn uri(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let path = if let Some(unc) = normalized.strip_prefix("//?/UNC/") {
        format!("//{unc}")
    } else {
        normalized
            .strip_prefix("//?/")
            .unwrap_or(&normalized)
            .to_owned()
    };
    let bytes = path.as_bytes();
    if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/' {
        format!("file:///{}:{}", char::from(bytes[0]), encode(&path[2..]))
    } else if let Some(unc) = path.strip_prefix("//") {
        format!("file://{}", encode(unc))
    } else if path.starts_with('/') {
        format!("file://{}", encode(&path))
    } else {
        encode(&path)
    }
}

pub fn render(report: &Report) -> Value {
    let names = [
        "TrailingWhitespace",
        "MissingFinalNewline",
        "TabIndentation",
        "LineTooLong",
        "BareTodoComment",
    ];
    let rules: Vec<_> = REGISTRY.iter().enumerate().map(|(index,rule)|json!({
        "id": rule.id, "name": names[index],
        "shortDescription": {"text": rule.message},
        "help": {"text": format!("{}; see docs/rules/{}.md. {}",rule.message,rule.id.to_lowercase(),if rule.fixable {"The CLI can apply this safe fix after validation; this SARIF contains no edit suggestions."} else {"No automatic fix."})},
        "defaultConfiguration": {"level":rule.severity,"enabled":index<3},
        "properties": {"tags":["style"],"fixable":rule.fixable}
    })).collect();
    let paths: BTreeSet<_> = report.diagnostics.iter().map(|d| d.path.as_str()).collect();
    let mut artifacts = Vec::new();
    let mut locations = BTreeMap::new();
    let mut needs_base = false;
    for path in paths {
        let index = artifacts.len();
        if report.stdin {
            artifacts.push(json!({"description":{"text":format!("Standard input ({path}); virtual name only")},"sourceLanguage":"verse","roles":["analysisTarget"]}));
            locations.insert(path, json!({"index":index}));
        } else {
            let uri = uri(path);
            let mut location = json!({"uri":uri});
            if !uri.starts_with("file:") && report.source_root.is_some() {
                location["uriBaseId"] = json!("%SRCROOT%");
                needs_base = true;
            }
            artifacts.push(
                json!({"location":location,"sourceLanguage":"verse","roles":["analysisTarget"]}),
            );
            location["index"] = json!(index);
            locations.insert(path, location);
        }
    }
    let results: Vec<_> = report.diagnostics.iter().map(|d| {
        let start = &d.range.start;let end = &d.range.end;
        json!({
            "ruleId":d.rule_id,"ruleIndex":REGISTRY.iter().position(|r|r.id==d.rule_id).expect("registered diagnostic rule"),
            "level":d.severity,"message":{"text":d.message},
            "locations":[{"physicalLocation":{"artifactLocation":locations[&d.path.as_str()],"region":{
                "startLine":start.line,"startColumn":start.column,"endLine":end.line,"endColumn":end.column,
                "byteOffset":start.byte_offset,"byteLength":end.byte_offset-start.byte_offset
            }}}],"properties":{"fixable":d.fixable}
        })
    }).collect();
    let notifications: Vec<_> = report.execution_errors.iter().map(|error|json!({
        "level":"error","message":{"text":match &error.path {Some(path)=>format!("{path}: {}",error.message),None=>error.message.clone()}}
    })).collect();
    let mut run = json!({
        "tool":{"driver":{"name":"verse-lint","version":env!("CARGO_PKG_VERSION"),"semanticVersion":env!("CARGO_PKG_VERSION"),"informationUri":"https://github.com/eiei114/verse-lint","language":"en-US","rules":rules}},
        "columnKind":"unicodeCodePoints","defaultEncoding":"utf-8","artifacts":artifacts,"results":results,
        "invocations":[{"executionSuccessful":report.summary.complete,"exitCode":report.exit_code,"toolExecutionNotifications":notifications}],
        "properties":{"summary":report.summary}
    });
    if needs_base {
        let root = report
            .source_root
            .as_deref()
            .expect("base requested only with root");
        let base = format!("{}/", root.trim_end_matches('/'));
        run["originalUriBaseIds"] = json!({"%SRCROOT%":{"uri":uri(&base)}});
    }
    json!({"$schema":SCHEMA,"version":"2.1.0","runs":[run]})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encodes_windows_drives_unc_relative_and_reserved_characters() {
        for (path, expected) in [
            (
                "C:\\日 space\\a#%.verse",
                "file:///C:/%E6%97%A5%20space/a%23%25.verse",
            ),
            ("D:/x.verse", "file:///D:/x.verse"),
            (
                "\\\\server\\share\\a b.verse",
                "file://server/share/a%20b.verse",
            ),
            ("\\\\?\\C:\\x.verse", "file:///C:/x.verse"),
            (
                "\\\\?\\UNC\\server\\share\\x.verse",
                "file://server/share/x.verse",
            ),
            ("../a:b?.verse", "../a%3Ab%3F.verse"),
            ("😀.verse", "%F0%9F%98%80.verse"),
        ] {
            assert_eq!(uri(path), expected);
        }
    }
}
