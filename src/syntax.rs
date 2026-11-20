use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

use tree_sitter::{Language, ParseOptions, Parser};
use tree_sitter_language::LanguageFn;

use crate::{
    lex::{self, Token},
    source::{Failure, Source},
};

unsafe extern "C" {
    fn tree_sitter_verse() -> *const ();
}

/// Documented in `docs/parser-design.md` as "two-second parse budget".
pub const PARSE_BUDGET: Duration = Duration::from_secs(2);
/// Documented in `docs/parser-design.md` as "CST depth 512".
pub const MAX_CST_DEPTH: usize = 512;
/// Documented in `docs/parser-design.md` as "one million fingerprint events".
pub const MAX_FINGERPRINT_EVENTS: usize = 1_000_000;

/// Budget predicate, kept separate from the closure so tests can prove the
/// mechanism directly instead of making a real parse take two seconds.
fn parse_budget_exceeded(elapsed: Duration, budget: Duration) -> bool {
    elapsed > budget
}

/// Depth and fingerprint-event guard shared by the single CST walk.
fn tree_limit(depth: usize, events: usize) -> Option<&'static str> {
    (depth > MAX_CST_DEPTH || events >= MAX_FINGERPRINT_EVENTS)
        .then_some("syntax tree resource limit exceeded")
}

#[derive(Debug)]
pub struct Document {
    pub tokens: Vec<Token>,
    shape: Vec<(u16, bool)>,
    // Retained CST. `Tree` is owned and lifetime-free, so retaining it changes
    // no signature, and every guard above still runs on it before it is kept.
    tree: tree_sitter::Tree,
    body_start: usize,
}

const EMPTY_ARCHETYPE_BRACE_MESSAGE: &str =
    "an empty indented archetype may have absorbed a braced expression on the next line";

fn empty_archetype_brace_continuation(
    node: tree_sitter::Node<'_>,
    source: &Source,
) -> Option<usize> {
    if node.kind() != "object_construction" {
        return None;
    }
    let head = node.child_by_field_name("type")?;
    if head.kind() != "object_construction" || head.child_by_field_name("fields").is_some() {
        return None;
    }
    let mut head_children = head.walk();
    let colon = head
        .children(&mut head_children)
        .find(|child| child.kind() == ":")?;
    let mut children = node.walk();
    let brace = node
        .children(&mut children)
        .find(|child| child.kind() == "{")?;
    (brace.start_position().row > colon.start_position().row)
        .then_some(source.body_start() + brace.start_byte())
}

const INDENTED_EXPRESSION_BOUNDARY_MESSAGE: &str =
    "ambiguous clause boundary or line-leading postfix continuation";

fn ends_with_empty_case_arm(node: tree_sitter::Node<'_>) -> bool {
    if node.kind() != "case_expression" {
        return false;
    }
    let mut cursor = node.walk();
    let Some(body) = node
        .named_children(&mut cursor)
        .filter(|child| child.kind() == "case_arm")
        .last()
        .and_then(|arm| arm.child_by_field_name("body"))
        .filter(|body| body.kind() == "indented_block")
    else {
        return false;
    };
    let mut cursor = body.walk();
    !body
        .named_children(&mut cursor)
        .any(|child| !child.is_extra())
}

/// The option's value is its first named child that is not a comment, so a
/// comment between `option` and `:` cannot hide the indented form.
fn option_value_is_indented(option: tree_sitter::Node<'_>) -> bool {
    let mut cursor = option.walk();
    option
        .named_children(&mut cursor)
        .find(|child| !child.is_extra())
        .is_some_and(|value| value.kind() == "indented_option_value")
}

const CASE_INDENT_BOUNDARY_MESSAGE: &str = "ambiguous indentation around an empty case arm";

// A case frame's first arm column is sufficient for most layouts. For a
// shallower empty wildcard, verify its ownership against the source line
// containing the colon, which can differ from the enclosing scanner frame.
fn shallow_empty_case_arm(
    node: tree_sitter::Node<'_>,
    bytes: &[u8],
) -> Result<Option<(usize, usize)>, usize> {
    if node.kind() != "case_expression" {
        return Ok(None);
    }
    let mut cursor = node.walk();
    let mut arms = node
        .named_children(&mut cursor)
        .filter(|child| child.kind() == "case_arm");
    let Some(first) = arms.next() else {
        return Ok(None);
    };
    let Some(last) = arms.last() else {
        return Ok(None);
    };
    let start = last.start_byte();
    let width = last.start_position().column;
    let first_width = first.start_position().column;
    if width >= first_width {
        return Ok(None);
    }

    let Some(pattern) = last.child_by_field_name("pattern") else {
        return Ok(None);
    };
    let Some(body) = last.child_by_field_name("body") else {
        return Ok(None);
    };
    let mut cursor = body.walk();
    if &bytes[pattern.byte_range()] != b"_"
        || body.kind() != "indented_block"
        || body
            .named_children(&mut cursor)
            .any(|child| !child.is_extra())
    {
        return Ok(None);
    }

    let mut cursor = node.walk();
    let colon = node.children(&mut cursor).find(|child| child.kind() == ":");
    let Some(colon) = colon else {
        return Err(start);
    };
    let header = &bytes[..colon.start_byte()];
    let header_line = header
        .rsplit(|byte| matches!(byte, b'\n' | b'\r'))
        .next()
        .unwrap_or_default();
    let header_width = header_line.iter().take_while(|&&b| b == b' ').count();
    let mut cursor = last.walk();
    let Some(arrow) = last
        .children(&mut cursor)
        .find(|child| child.kind() == "=>")
    else {
        return Err(start);
    };
    let tail = &bytes[arrow.end_byte()..];
    let next = tail
        .split(|byte| matches!(byte, b'\n' | b'\r'))
        .skip(1)
        .find(|line| !line.trim_ascii().is_empty());
    if width <= header_width
        || header_line.get(header_width) == Some(&b'\t')
        || next.is_some_and(|line| {
            let next_width = line.iter().take_while(|&&b| b == b' ').count();
            next_width > header_width || line.get(next_width) == Some(&b'\t')
        })
    {
        return Err(start);
    }
    Ok(Some((start, first_width)))
}

fn ambiguous_expression_boundary(
    node: tree_sitter::Node<'_>,
    bytes: &[u8],
    in_checked_block: bool,
) -> Option<usize> {
    if node.kind() == "for_block_clause_list" {
        let mut cursor = node.walk();
        let mut previous = None::<tree_sitter::Node<'_>>;
        for child in node.children(&mut cursor).filter(|child| !child.is_extra()) {
            if child.kind() == "," {
                previous = None;
            } else {
                if let Some(left) = previous {
                    let left_text = bytes[left.byte_range()].trim_ascii_end();
                    let end = left.start_byte() + left_text.len();
                    if !bytes[end..child.start_byte()].contains(&b'\n') {
                        return Some(child.start_byte());
                    }
                }
                previous = Some(child);
            }
        }
        return None;
    }
    // The pinned grammar can attach the start of the next clause to a
    // previous expression as a postfix. Check the operator's own source span
    // so parentheses inside an explicitly delimited argument stay supported.
    let (field, opener) = match node.kind() {
        "call_expression" => ("function", "("),
        "failable_call_expression" => ("function", "["),
        "index_expression" => ("object", "["),
        "object_construction" | "map_literal" => ("type", "{"),
        _ => return None,
    };
    let left = node.child_by_field_name(field)?;
    if !in_checked_block
        && !ends_with_empty_case_arm(left)
        && !left
            .child_by_field_name("clauses")
            .is_some_and(|clauses| clauses.kind() == "for_block_clause_list")
        && !(left.kind() == "option_expression"
            && (left.child_by_field_name("additional_value").is_some()
                || option_value_is_indented(left)))
    {
        return None;
    }
    let mut cursor = node.walk();
    let next = node
        .children(&mut cursor)
        .find(|child| child.kind() == opener)?;
    let end = left.start_byte() + bytes[left.byte_range()].trim_ascii_end().len();
    bytes[end..next.start_byte()]
        .contains(&b'\n')
        .then_some(next.start_byte())
}

impl Document {
    pub fn parse(source: &Source) -> Result<Self, Failure> {
        Self::parse_with_budget(source, PARSE_BUDGET)
    }

    fn parse_with_budget(source: &Source, budget: Duration) -> Result<Self, Failure> {
        let tokens = lex::scan(source)?;
        // SAFETY: the function is linked from the pinned generated grammar and
        // returns a static TSLanguage with ABI 15, supported by tree-sitter 0.25.
        let language = Language::new(unsafe { LanguageFn::from_raw(tree_sitter_verse) });
        let mut parser = Parser::new();
        parser
            .set_language(&language)
            .map_err(|e| Failure::new(0, format!("grammar ABI failure: {e}")))?;
        let bytes = &source.text().as_bytes()[source.body_start()..];
        let started = Instant::now();
        let mut cancel =
            |_: &tree_sitter::ParseState| parse_budget_exceeded(started.elapsed(), budget);
        let tree = parser
            .parse_with_options(
                &mut |i, _| bytes.get(i..).unwrap_or_default(),
                None,
                Some(ParseOptions::new().progress_callback(&mut cancel)),
            )
            .ok_or_else(|| Failure::new(0, "parse budget exceeded; source was not changed"))?;
        // Same guard as formatter: newlines are not separators, so a line with
        // two statements and no `;` (`A := 1 B := 2`) parses as siblings without
        // ERROR; no safe lint/fix claim for that structure.
        // Extracted into its own scope so no CST cursor outlives it and keeps the
        // tree borrowed by the `Document` that now owns it.
        check_adjacent_statements(&tree, source, &tokens)?;
        let shape = validate(&tree, source, &tokens)?;
        Ok(Self {
            tokens,
            shape,
            tree,
            body_start: source.body_start(),
        })
    }

    /// Root of the retained CST. Offsets are relative to [`Self::body_start`].
    /// Retaining the tree changes no guard: this is the same tree the coverage
    /// validation, budget and comment-boundary walk already ran on.
    pub fn root(&self) -> tree_sitter::Node<'_> {
        self.tree.root_node()
    }

    /// Byte offset of the first body byte, accounting for a UTF-8 BOM. CST byte
    /// offsets are relative to it.
    pub fn body_start(&self) -> usize {
        self.body_start
    }

    pub fn equivalent(&self, source: &Source, other: &Self, output: &Source) -> bool {
        self.shape == other.shape
            && self
                .tokens
                .iter()
                .filter(|t| !t.kind.is_trivia())
                .map(|t| (t.kind, t.text(source)))
                .eq(other
                    .tokens
                    .iter()
                    .filter(|t| !t.kind.is_trivia())
                    .map(|t| (t.kind, t.text(output))))
    }
}

/// `@name Def` on one line: an attribute prefixes the definition after it,
/// so the two siblings are not an ambiguous split. Only a declaration (or a
/// further attribute) may follow; a braced literal or any other expression
/// after an attribute still fails the guard, so a misparsed `@name{...}` can
/// never pass as two harmless siblings.
fn prefix_attribute(left: tree_sitter::Node<'_>, right: tree_sitter::Node<'_>) -> bool {
    left.kind() == "attribute"
        && matches!(
            right.kind(),
            "attribute"
                | "type_definition"
                | "function_definition"
                | "extension_function_definition"
                | "var_declaration"
                | "constant_declaration"
                | "field_declaration"
        )
}

/// True when nothing but spaces and comments lies between the last
/// significant token that ends by `left_end` and `right_start`: the two items
/// share one logical line. A block comment that spans lines is not a line
/// break (`A := 1 <# a\n #> B := 2` is one line of two statements), so only a
/// newline outside comments, or one of `separators`, separates them.
fn shares_logical_line(
    tokens: &[Token],
    source: &Source,
    left_end: usize,
    right_start: usize,
    separators: &[&str],
) -> bool {
    let last = tokens[..tokens.partition_point(|t| t.range.end <= left_end)]
        .iter()
        .rev()
        .find(|t| !t.kind.is_trivia() && !t.kind.is_comment())
        .map_or(0, |t| t.range.end);
    !tokens[tokens.partition_point(|t| t.range.start < last)..]
        .iter()
        .take_while(|t| t.range.start < right_start)
        .any(|t| {
            t.kind == lex::Kind::Newline
                || (t.kind == lex::Kind::Code && separators.contains(&t.text(source)))
        })
}

/// Containers whose items must each start on their own logical line (or be
/// separated by `;`, or `,` where the container allows it).
fn item_separators(kind: &str) -> Option<&'static [&'static str]> {
    match kind {
        "indented_block" | "colon_block" | "brace_block" => Some(&[";"]),
        "class_definition"
        | "struct_definition"
        | "interface_definition"
        | "module_definition"
        | "field_initializer_block" => Some(&[";"]),
        // Condition items may also use `,`, and an item's own `:=` binds its
        // name to its value.
        "if_block_condition" => Some(&[";", ",", ":="]),
        // Indented `for:` clauses: one per line, or separated by `,`.
        "for_block_clause_list" => Some(&[","]),
        _ => None,
    }
}

/// The first item of `node` that shares a logical line with the item before
/// it. Header fields of a type definition (its specifiers and bases) are not
/// items, and a prefix attribute may share the line of the item it prefixes.
fn unseparated_item(
    node: tree_sitter::Node<'_>,
    tokens: &[Token],
    source: &Source,
) -> Option<usize> {
    let separators = item_separators(node.kind())?;
    let mut cursor = node.walk();
    let mut previous: Option<tree_sitter::Node<'_>> = None;
    if !cursor.goto_first_child() {
        return None;
    }
    loop {
        let child = cursor.node();
        let header = matches!(cursor.field_name(), Some("specifiers" | "base"));
        if child.is_named() && !child.is_extra() && !header {
            if let Some(left) = previous {
                let end = left.end_byte() + source.body_start();
                let start = child.start_byte() + source.body_start();
                if shares_logical_line(tokens, source, end, start, separators)
                    && !prefix_attribute(left, child)
                {
                    return Some(start);
                }
            }
            previous = Some(child);
        }
        if !cursor.goto_next_sibling() {
            return None;
        }
    }
}

/// Where the tree still carries an error that no visible node shows: descend
/// through the first child that has one and report the end of the deepest
/// such node, which is where a hidden MISSING token sits.
fn hidden_error_offset(root: tree_sitter::Node<'_>) -> Option<usize> {
    if !root.has_error() {
        return None;
    }
    let mut node = root;
    loop {
        let mut cursor = node.walk();
        let Some(child) = node
            .children(&mut cursor)
            .find(|child| child.has_error() || child.is_missing())
        else {
            return Some(node.end_byte());
        };
        node = child;
    }
}

const SPLIT_TYPE_ANNOTATION_MESSAGE: &str = "type annotation continues on the next line; `name:` followed by an indented block is a block, not a type";

/// A declaration whose type starts on a later line than its `:`. Verse reads
/// `point:` followed by an indented block as a construction, so a tree that
/// joined the two lines into one typed definition is not trusted.
fn split_type_annotation(
    node: tree_sitter::Node<'_>,
    tokens: &[Token],
    source: &Source,
) -> Option<usize> {
    if !matches!(
        node.kind(),
        "constant_declaration" | "field_declaration" | "var_declaration" | "positional_parameter"
    ) {
        return None;
    }
    let ty = node.child_by_field_name("type")?;
    let mut cursor = node.walk();
    let colon = node
        .children(&mut cursor)
        .take_while(|child| child.start_byte() < ty.start_byte())
        .filter(|child| child.kind() == ":")
        .last()?;
    let from = colon.end_byte() + source.body_start();
    let to = ty.start_byte() + source.body_start();
    tokens[tokens.partition_point(|t| t.range.start < from)..]
        .iter()
        .take_while(|t| t.range.start < to)
        .any(|t| t.kind == lex::Kind::Newline)
        .then_some(to)
}

const HIDDEN_LINE_BREAK_OPENER_MESSAGE: &str = "call or parameter opener follows a line break hidden behind a comment; the line break ends the expression";

/// A `(` that opens a call or a parameter list after a line break. The
/// scanner only reads such an opener on the callee's line, but a comment
/// between the line break and the `(` hides that break from it
/// (`F` then a line break then `<# note #>(1)`), so the tree can join what
/// the official grammar keeps apart.
fn opener_after_line_break(
    node: tree_sitter::Node<'_>,
    tokens: &[Token],
    source: &Source,
) -> Option<usize> {
    if !matches!(
        node.kind(),
        "call_expression"
            | "function_definition"
            | "extension_function_definition"
            | "type_definition"
            | "generic_type"
            | "function_parameter"
    ) {
        return None;
    }
    let mut cursor = node.walk();
    let mut previous: Option<tree_sitter::Node<'_>> = None;
    for child in node.children(&mut cursor) {
        if child.is_extra() {
            continue;
        }
        if child.kind() == "("
            && let Some(left) = previous
        {
            let from = left.end_byte() + source.body_start();
            let to = child.start_byte() + source.body_start();
            if tokens[tokens.partition_point(|t| t.range.start < from)..]
                .iter()
                .take_while(|t| t.range.start < to)
                .any(|t| t.kind == lex::Kind::Newline)
            {
                return Some(to);
            }
        }
        previous = Some(child);
    }
    None
}

const UNSEPARATED_ITEMS_MESSAGE: &str =
    "ambiguous adjacent statements on one logical line; separate them with a newline or `;`";

/// The adjacent-statement guard that already ran before this slice, extracted
/// verbatim so [`Document::parse`] can keep the tree it validated.
fn check_adjacent_statements(
    tree: &tree_sitter::Tree,
    source: &Source,
    tokens: &[Token],
) -> Result<(), Failure> {
    let mut children = tree.root_node().walk();
    let mut previous: Option<tree_sitter::Node<'_>> = None;
    for node in tree.root_node().named_children(&mut children) {
        if node.is_extra() {
            continue;
        }
        if let Some(left) = previous {
            let end = left.end_byte() + source.body_start();
            let start = node.start_byte() + source.body_start();
            if shares_logical_line(tokens, source, end, start, &[";"])
                && !prefix_attribute(left, node)
            {
                return Err(Failure::new(
                    start,
                    "ambiguous adjacent top-level statements; inline binary function bodies are unsupported by the pinned grammar",
                ));
            }
        }
        previous = Some(node);
    }
    Ok(())
}

/// The coverage walk that already ran before this slice. It is extracted
/// unchanged so [`Document::parse`] can keep the tree it validated.
fn validate(
    tree: &tree_sitter::Tree,
    source: &Source,
    tokens: &[Token],
) -> Result<Vec<(u16, bool)>, Failure> {
    let bytes = &source.text().as_bytes()[source.body_start()..];
    let mut shape = Vec::new();
    // Require exact CST spans for independently lexed block comments.
    // Opaque string/interpolation tokens are intentionally not included.
    let mut unmatched_comments: HashSet<_> = tokens
        .iter()
        .filter(|token| token.kind == lex::Kind::BlockComment)
        .map(|token| (token.range.start, token.range.end))
        .collect();
    let mut cursor = tree.walk();
    let mut depth = 0;
    let mut continuation_guard_depth = None;
    loop {
        let node = cursor.node();
        if node.kind() == "block_comment" {
            unmatched_comments.remove(&(
                node.start_byte() + source.body_start(),
                node.end_byte() + source.body_start(),
            ));
        }
        if node.is_error() || node.is_missing() {
            return Err(Failure::new(
                node.start_byte() + source.body_start(),
                "unsupported or incomplete Verse syntax; use UEFN for compiler diagnostics",
            ));
        }
        if let Err(offset) = shallow_empty_case_arm(node, bytes) {
            return Err(Failure::new(
                source.body_start() + offset,
                CASE_INDENT_BOUNDARY_MESSAGE,
            ));
        }
        if let Some(start) = leading_unary_continuation(node, source) {
            return Err(Failure::new(
                start,
                "binary `+`/`-` operator starts a new line; a leading unary operand is ambiguous with a continued expression in the pinned grammar",
            ));
        }
        if continuation_guard_depth.is_none()
            && (matches!(
                node.kind(),
                "for_block_clause_list" | "indented_option_value"
            ) || node.child_by_field_name("additional_value").is_some())
        {
            continuation_guard_depth = Some(depth);
        }
        if let Some(offset) =
            ambiguous_expression_boundary(node, bytes, continuation_guard_depth.is_some())
        {
            return Err(Failure::new(
                source.body_start() + offset,
                INDENTED_EXPRESSION_BOUNDARY_MESSAGE,
            ));
        }
        if let Some(start) = unseparated_item(node, tokens, source) {
            return Err(Failure::new(start, UNSEPARATED_ITEMS_MESSAGE));
        }
        if let Some(start) = split_type_annotation(node, tokens, source) {
            return Err(Failure::new(start, SPLIT_TYPE_ANNOTATION_MESSAGE));
        }
        if let Some(start) = opener_after_line_break(node, tokens, source) {
            return Err(Failure::new(start, HIDDEN_LINE_BREAK_OPENER_MESSAGE));
        }
        if let Some(start) = empty_archetype_brace_continuation(node, source) {
            return Err(Failure::new(start, EMPTY_ARCHETYPE_BRACE_MESSAGE));
        }
        if let Some(message) = tree_limit(depth, shape.len()) {
            return Err(Failure::new(
                node.start_byte() + source.body_start(),
                message,
            ));
        }
        shape.push((node.kind_id(), true));
        if cursor.goto_first_child() {
            depth += 1;
            continue;
        }
        loop {
            shape.push((cursor.node().kind_id(), false));
            if continuation_guard_depth == Some(depth) {
                continuation_guard_depth = None;
            }
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                // The cursor never visits hidden terminals, so a MISSING
                // `_indent`/`_dedent` inserted during recovery is invisible to
                // the walk above; the tree's own error flag still records it.
                if let Some(offset) = hidden_error_offset(tree.root_node()) {
                    return Err(Failure::new(
                        offset + source.body_start(),
                        "unsupported or incomplete Verse syntax; use UEFN for compiler diagnostics",
                    ));
                }
                if let Some((start, _)) = unmatched_comments.into_iter().min() {
                    return Err(Failure::new(
                        start,
                        "unsupported block comment boundary: lexer and CST disagree; source was not changed",
                    ));
                }
                return Ok(shape);
            }
            depth -= 1;
        }
    }
}

/// Same guard as formatter: newlines are not separators in the pinned
/// grammar, so a statement line starting with unary `+`/`-` can be absorbed as
/// the right operand of the previous line's expression without ERROR. Returns
/// the operator offset when a binary `+`/`-` is separated from its left operand
/// by a line break. Only the gap between the operands is scanned.
fn leading_unary_continuation(node: tree_sitter::Node<'_>, source: &Source) -> Option<usize> {
    if node.kind() != "binary_expression" {
        return None;
    }
    let left = node.child_by_field_name("left")?;
    let operator = node.child_by_field_name("operator")?;
    if !matches!(operator.kind(), "+" | "-") {
        return None;
    }
    // A dedent can put trailing newline bytes in the left node.
    let bytes = source.text().as_bytes();
    let left_start = left.start_byte() + source.body_start();
    let left_end = left.end_byte() + source.body_start();
    let content_end = left_start + bytes[left_start..left_end].trim_ascii_end().len();
    let start = operator.start_byte() + source.body_start();
    bytes[content_end..start].contains(&b'\n').then_some(start)
}

#[cfg(test)]
#[path = "doc-drift-tests.rs"]
mod doc_drift_tests;

#[cfg(test)]
#[path = "range-binding-tests.rs"]
mod range_binding_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(text: &str) -> (Source, Document) {
        let source = Source::from_bytes(text.as_bytes()).unwrap();
        let document = Document::parse(&source).unwrap();
        (source, document)
    }

    #[test]
    fn recognizes_different_indent_widths_without_calling_them_invalid() {
        let (a, ad) = parsed("calc := class:\n  Value:int = 1\n  Get():int =\n    Value\n");
        let (b, bd) = parsed("calc := class:\n    Value:int = 1\n    Get():int =\n        Value\n");
        assert!(ad.equivalent(&a, &bd, &b));
    }

    #[test]
    fn catches_changes_to_block_ownership_with_identical_code_tokens() {
        let (a, ad) = parsed(
            "calc := class:\n    First():void =\n        Print(\"one\")\n    Second():void =\n        Print(\"two\")\n",
        );
        let (b, bd) = parsed(
            "calc := class:\n    First():void =\n        Print(\"one\")\nSecond():void =\n    Print(\"two\")\n",
        );
        assert!(!ad.equivalent(&a, &bd, &b));
    }

    #[test]
    fn typed_constants_retain_method_and_file_ownership() {
        let text = "sample := class:\n    First():void =\n        Label:string=\"hello\"\n        Count:int=2\n    Second():void =\n        Other:int=3\nOutside:int=4\n";
        let (_, _) = parsed(text);
        let mut parser = Parser::new();
        // SAFETY: same statically linked grammar as production.
        let language = Language::new(unsafe { LanguageFn::from_raw(tree_sitter_verse) });
        parser.set_language(&language).unwrap();
        let tree = parser.parse(text, None).unwrap();
        let mut pending = vec![tree.root_node()];
        let mut constants = Vec::new();
        while let Some(node) = pending.pop() {
            if node.kind() == "constant_declaration" {
                let name = node
                    .child_by_field_name("name")
                    .unwrap()
                    .utf8_text(text.as_bytes())
                    .unwrap();
                let parent = node.parent().unwrap();
                let scope = if parent.kind() == "source_file" {
                    "file"
                } else {
                    assert_eq!(parent.kind(), "indented_block");
                    let function = parent.parent().unwrap();
                    assert_eq!(function.kind(), "function_definition");
                    assert_eq!(function.parent().unwrap().kind(), "class_definition");
                    function
                        .child_by_field_name("name")
                        .unwrap()
                        .utf8_text(text.as_bytes())
                        .unwrap()
                };
                constants.push((name, scope));
            }
            let mut cursor = node.walk();
            pending.extend(node.named_children(&mut cursor));
        }
        constants.sort_unstable();
        assert_eq!(
            constants,
            [
                ("Count", "First"),
                ("Label", "First"),
                ("Other", "Second"),
                ("Outside", "file")
            ]
        );
    }

    #[test]
    fn indented_if_condition_keeps_each_line_as_its_own_item() {
        let text = "F():void =\n    if:\n        Value := Values[0]\n        set Seen[Value] = true\n        not Blocked[Value]\n    then:\n        Print(\"ok\")\n";
        let (_, _) = parsed(text);
        let mut parser = Parser::new();
        // SAFETY: same statically linked grammar as production.
        let language = Language::new(unsafe { LanguageFn::from_raw(tree_sitter_verse) });
        parser.set_language(&language).unwrap();
        let tree = parser.parse(text, None).unwrap();
        assert!(!tree.root_node().has_error());
        let mut pending = vec![tree.root_node()];
        let mut condition = None;
        while let Some(node) = pending.pop() {
            if node.kind() == "if_block_condition" {
                condition = Some(node);
                break;
            }
            let mut cursor = node.walk();
            pending.extend(node.named_children(&mut cursor));
        }
        let condition = condition.unwrap();
        let mut cursor = condition.walk();
        let items: Vec<_> = condition
            .named_children(&mut cursor)
            .map(|node| node.utf8_text(text.as_bytes()).unwrap().to_owned())
            .collect();
        assert_eq!(
            items,
            [
                "Value",
                "Values[0]",
                "set Seen[Value] = true",
                "not Blocked[Value]"
            ]
        );
    }

    #[test]
    fn a_line_starting_with_unary_plus_or_minus_is_refused_not_joined() {
        for text in [
            "F():void =\n    X := A\n    -2 < A\n",
            "F()<suspends>:void =\n    spawn:\n        G()\n    -2\n",
            "F()<suspends>:void =\r\n    spawn:\r\n        G()\r\n    +2\r\n",
            "F():void =\n    if:\n        A := B[]\n        -2 < A\n    then:\n        G()\n",
            "F():void =\n    case(T):\n        t.A =>\n            X := A\n            +1\n        _ => {}\n",
        ] {
            let source = Source::from_bytes(text.as_bytes()).unwrap();
            let failure = Document::parse(&source).err().unwrap();
            assert!(
                failure.message.contains("starts a new line"),
                "{text}: {failure:?}"
            );
        }
        for text in [
            "F():void =\n    X := A - 2\n",
            "F():void =\n    X := A\n    Y := -2\n",
        ] {
            let source = Source::from_bytes(text.as_bytes()).unwrap();
            assert!(Document::parse(&source).is_ok(), "{text}");
        }
    }

    #[test]
    fn spawn_body_keeps_only_its_expression() {
        let text = "F()<suspends>:void =\n    spawn:\n        G()\n    H()\n    spawn{I()}\n";
        let (_, _) = parsed(text);
        let mut parser = Parser::new();
        // SAFETY: same statically linked grammar as production.
        let language = Language::new(unsafe { LanguageFn::from_raw(tree_sitter_verse) });
        parser.set_language(&language).unwrap();
        let tree = parser.parse(text, None).unwrap();
        let root = tree.root_node();
        assert!(!root.has_error());
        assert_eq!(root.named_child_count(), 1);
        let function = root.named_child(0).unwrap();
        let body = function.child_by_field_name("body").unwrap();
        assert_eq!(body.named_child_count(), 3);
        for (index, expected) in [(0, "G()"), (2, "I()")] {
            let spawn = body.named_child(index).unwrap();
            assert_eq!(spawn.kind(), "spawn_expression");
            let call = spawn.named_child(0).unwrap();
            assert_eq!(call.kind(), "call_expression");
            assert_eq!(call.utf8_text(text.as_bytes()).unwrap(), expected);
        }
        assert_eq!(
            body.named_child(1)
                .unwrap()
                .utf8_text(text.as_bytes())
                .unwrap(),
            "H()"
        );
    }

    #[test]
    fn case_arm_bodies_keep_their_statements() {
        let text = "F(T:t):void =\n    case(T):\n        t.A =>\n            C := 1\n            G(C)\n        t.B => {}\n        _ => H()\n";
        let (_, _) = parsed(text);
        let mut parser = Parser::new();
        // SAFETY: same statically linked grammar as production.
        let language = Language::new(unsafe { LanguageFn::from_raw(tree_sitter_verse) });
        parser.set_language(&language).unwrap();
        let tree = parser.parse(text, None).unwrap();
        assert!(!tree.root_node().has_error());
        let mut pending = vec![tree.root_node()];
        let mut bodies = Vec::new();
        while let Some(node) = pending.pop() {
            if node.kind() == "case_arm" {
                let body = node.child_by_field_name("body").unwrap();
                bodies.push((
                    body.kind().to_owned(),
                    body.named_child_count(),
                    body.utf8_text(text.as_bytes()).unwrap().trim().to_owned(),
                ));
            }
            let mut cursor = node.walk();
            pending.extend(node.named_children(&mut cursor));
        }
        bodies.sort();
        assert_eq!(
            bodies,
            [
                ("brace_block".to_owned(), 0, "{}".to_owned()),
                ("call_expression".to_owned(), 1, "H()".to_owned()),
                (
                    "indented_block".to_owned(),
                    2,
                    "C := 1\n            G(C)".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn set_condition_and_key_value_iterator_keep_their_owners() {
        let text = "F():void =\n    if (set Values[Index] = Value):\n        Print(\"ok\")\n    for (Key -> Value : Values):\n        Print(Value)\n";
        let (_, _) = parsed(text);
        let mut parser = Parser::new();
        // SAFETY: same statically linked grammar as production.
        let language = Language::new(unsafe { LanguageFn::from_raw(tree_sitter_verse) });
        parser.set_language(&language).unwrap();
        let tree = parser.parse(text, None).unwrap();
        let mut pending = vec![tree.root_node()];
        let mut nodes = Vec::new();
        while let Some(node) = pending.pop() {
            let mut cursor = node.walk();
            pending.extend(node.named_children(&mut cursor));
            nodes.push(node);
        }

        let set = nodes
            .iter()
            .find(|node| node.kind() == "set_statement")
            .unwrap();
        assert_eq!(set.parent().unwrap().kind(), "if_condition");
        assert_eq!(
            set.child_by_field_name("target")
                .unwrap()
                .utf8_text(text.as_bytes())
                .unwrap(),
            "Values[Index]"
        );

        let iterator = nodes
            .iter()
            .find(|node| node.kind() == "for_iterator")
            .unwrap();
        assert_eq!(iterator.parent().unwrap().kind(), "for_clause_list");
        for (field, expected) in [("key", "Key"), ("value", "Value"), ("iterable", "Values")] {
            assert_eq!(
                iterator
                    .child_by_field_name(field)
                    .unwrap()
                    .utf8_text(text.as_bytes())
                    .unwrap(),
                expected,
                "field {field}"
            );
        }
    }

    #[test]
    fn tuple_type_is_owned_by_the_archetype_head() {
        let text = "c := class:\n    E:event(tuple(int, logic)) = event(tuple(int, logic)){}\n    N:int = 1\n";
        let (_, document) = parsed(text);
        let root = document.root();
        assert!(!root.has_error());

        let class = root
            .named_child(0)
            .unwrap()
            .child_by_field_name("value")
            .unwrap();
        assert_eq!(class.named_child_count(), 2);
        let field = class.named_child(0).unwrap();
        let declared_type = field.child_by_field_name("type").unwrap();
        assert_eq!(
            declared_type
                .child_by_field_name("argument")
                .unwrap()
                .kind(),
            "tuple_type"
        );
        let initializer = field.child_by_field_name("default").unwrap();
        assert_eq!(initializer.kind(), "object_construction");
        let head = initializer.child_by_field_name("type").unwrap();
        assert_eq!(head.kind(), "call_expression");
        let arguments = head.child_by_field_name("arguments").unwrap();
        assert_eq!(arguments.named_child_count(), 1);
        assert_eq!(arguments.named_child(0).unwrap().kind(), "tuple_type");
        assert_eq!(class.named_child(1).unwrap().kind(), "field_declaration");
    }

    #[test]
    fn indented_for_retains_clauses_body_and_sibling() {
        let text = "F():void=\n    for:\n        Key -> Item : Items\n        Choice := Pick[Key]\n        Choice > 0\n    do:\n        Use(Item, Choice)\n    Done()\n";
        let (_, document) = parsed(text);
        let root = document.root();
        let body = root
            .named_child(0)
            .unwrap()
            .child_by_field_name("body")
            .unwrap();
        assert_eq!(body.named_child_count(), 2);
        let expression = body.named_child(0).unwrap();
        assert_eq!(expression.kind(), "for_expression");
        let clauses = expression.child_by_field_name("clauses").unwrap();
        assert_eq!(clauses.kind(), "for_block_clause_list");
        let mut cursor = clauses.walk();
        assert_eq!(
            clauses
                .named_children(&mut cursor)
                .map(|child| child.kind())
                .collect::<Vec<_>>(),
            ["for_iterator", "for_filter_binding", "binary_expression"]
        );
        assert_eq!(
            expression.child_by_field_name("body").unwrap().kind(),
            "colon_block"
        );
        assert_eq!(body.named_child(1).unwrap().kind(), "call_expression");
    }

    #[test]
    fn a_comment_never_hides_a_line_break_from_a_clause_or_an_opener() {
        // The newline inside or before the comment is not a separator for the
        // joined `for` clauses, and the line break before the comment still
        // ends `F` before a `(` that follows the comment.
        for (text, message) in [
            (
                "F():void =\n    for:\n        X : Xs <# keep\n        comment #> Y : Ys\n    do:\n        Use(X, Y)\n",
                "ambiguous adjacent statements on one logical line",
            ),
            ("A := F\n<# note #>(1)\n", "opener follows a line break"),
        ] {
            let source = Source::from_bytes(text.as_bytes()).unwrap();
            let error = Document::parse(&source).unwrap_err();
            assert!(error.message.contains(message), "{text}: {error:?}");
        }
        for text in [
            "A := F\n<# note #>\n(1)\n",
            "A := F <# note #>(1)\n",
            "F():void =\n    for:\n        X : Xs, <# keep\n        comment #> Y : Ys\n    do:\n        Use(X, Y)\n",
        ] {
            let source = Source::from_bytes(text.as_bytes()).unwrap();
            assert!(Document::parse(&source).is_ok(), "{text}");
        }
    }

    #[test]
    fn indented_for_refuses_ambiguous_clause_boundaries() {
        // A parenthesis on the next line cannot join the previous clause: it
        // is its own filter clause, as in the official grammar.
        let (_, document) = parsed(
            "F():void =\n    for:\n        Item : Items\n        (0)\n    do:\n        Use(Item)\n",
        );
        let clauses = document
            .root()
            .named_child(0)
            .unwrap()
            .child_by_field_name("body")
            .unwrap()
            .named_child(0)
            .unwrap()
            .child_by_field_name("clauses")
            .unwrap();
        let mut cursor = clauses.walk();
        assert_eq!(
            clauses
                .named_children(&mut cursor)
                .map(|child| child.kind())
                .collect::<Vec<_>>(),
            ["for_iterator", "parenthesized_expression"]
        );
        for joined in ["[0]", "{Value := 0}"] {
            let text = format!(
                "F():void =\n    for:\n        Item : Items\n        {joined}\n    do:\n        Use(Item)\n"
            );
            let source = Source::from_bytes(text.as_bytes()).unwrap();
            let error = Document::parse(&source).unwrap_err();
            assert!(
                error.message.contains(INDENTED_EXPRESSION_BOUNDARY_MESSAGE),
                "{joined}: {error:?}"
            );
        }
    }

    #[test]
    fn indented_for_result_refuses_next_line_index() {
        let text = "F():void =\n    Selection := for:\n        Item : Items\n    do:\n        Item\n    [0]\n";
        let source = Source::from_bytes(text.as_bytes()).unwrap();
        let error = Document::parse(&source).unwrap_err();
        assert!(
            error.message.contains(INDENTED_EXPRESSION_BOUNDARY_MESSAGE),
            "{error:?}"
        );
    }

    #[test]
    fn mutable_fields_accept_both_initializer_spellings() {
        let text = "Parcel := module{\nCrate := class:\n  @editable\n  var Slots:[]int:=array{}\n  var Count:int=0\n}\n";
        let (_, document) = parsed(text);
        let root = document.root();
        let definition = root
            .named_child(0)
            .unwrap()
            .child_by_field_name("value")
            .unwrap();
        assert_eq!(definition.kind(), "module_definition");
        let class = definition
            .named_child(0)
            .unwrap()
            .child_by_field_name("value")
            .unwrap();
        assert_eq!(class.kind(), "class_definition");
        for (index, value) in [(1, "array_literal"), (2, "integer_literal")] {
            let variable = class.named_child(index).unwrap();
            assert_eq!(variable.kind(), "var_declaration");
            assert_eq!(variable.child_by_field_name("value").unwrap().kind(), value);
        }
    }

    #[test]
    fn option_values_own_their_expressions_and_leave_following_calls_separate() {
        let text = "F():void=\n  First := option{1,2}\n  Second := option:\n    Pick[]\n  Use(First,Second)\n";
        let (_, document) = parsed(text);
        let root = document.root();
        let body = root
            .named_child(0)
            .unwrap()
            .child_by_field_name("body")
            .unwrap();
        assert_eq!(body.named_child_count(), 3);
        let braced = body
            .named_child(0)
            .unwrap()
            .child_by_field_name("value")
            .unwrap();
        assert_eq!(braced.kind(), "option_expression");
        assert_eq!(braced.named_child_count(), 2);
        assert_eq!(
            braced
                .child_by_field_name("additional_value")
                .unwrap()
                .kind(),
            "integer_literal"
        );
        let indented = body
            .named_child(1)
            .unwrap()
            .child_by_field_name("value")
            .unwrap();
        assert_eq!(indented.kind(), "option_expression");
        let value = indented.named_child(0).unwrap();
        assert_eq!(value.kind(), "indented_option_value");
        assert_eq!(
            value.named_child(0).unwrap().kind(),
            "failable_call_expression"
        );
        assert_eq!(body.named_child(2).unwrap().kind(), "call_expression");
    }

    #[test]
    fn indented_option_refuses_a_line_leading_index() {
        let text = "F():void =\n    Value := option:\n        Pick[]\n    [0]\n";
        let source = Source::from_bytes(text.as_bytes()).unwrap();
        let error = Document::parse(&source).unwrap_err();
        assert!(
            error.message.contains(INDENTED_EXPRESSION_BOUNDARY_MESSAGE),
            "{error:?}"
        );
    }

    #[test]
    fn type_arguments_keep_the_archetype_and_logic_literal_distinct() {
        let text = "Parcel := module{\nCrate := class:\n    Ready:event(logic)=event(logic){}\n    Collected:event([]int)=event([]int){}\n    F():void =\n        Use(logic{true})\n}\n";
        let (_, document) = parsed(text);
        let root = document.root();
        let module = root
            .named_child(0)
            .unwrap()
            .child_by_field_name("value")
            .unwrap();
        let class = module
            .named_child(0)
            .unwrap()
            .child_by_field_name("value")
            .unwrap();
        for (index, expected) in [(0, "builtin_type"), (1, "array_type")] {
            let initializer = class
                .named_child(index)
                .unwrap()
                .child_by_field_name("default")
                .unwrap();
            assert_eq!(initializer.kind(), "object_construction");
            let head = initializer.child_by_field_name("type").unwrap();
            assert_eq!(head.kind(), "call_expression");
            let arguments = head.child_by_field_name("arguments").unwrap();
            assert_eq!(arguments.named_child(0).unwrap().kind(), expected);
        }
        let body = class
            .named_child(2)
            .unwrap()
            .child_by_field_name("body")
            .unwrap();
        let call = body.named_child(0).unwrap();
        let arguments = call.child_by_field_name("arguments").unwrap();
        assert_eq!(arguments.named_child(0).unwrap().kind(), "logic_literal");
    }

    #[test]
    fn mixed_else_if_chains_preserve_alternative_and_sibling_ownership() {
        let text = "F():void=\n  if:\n    Ready?\n  then:\n    Start()\n  else if (Delay > 0):\n    Retry()\n  else if:\n    Expired?\n  then:\n    Stop()\n  else:\n    Wait()\n  Done()\n";
        let (_, document) = parsed(text);
        let root = document.root();
        let body = root
            .named_child(0)
            .unwrap()
            .child_by_field_name("body")
            .unwrap();
        assert_eq!(body.named_child_count(), 2);
        let condition = body.named_child(0).unwrap();
        assert_eq!(condition.kind(), "if_expression");
        let alternative = condition.child_by_field_name("alternative").unwrap();
        assert_eq!(alternative.kind(), "else_if_clause");
        let middle = alternative.named_child(0).unwrap();
        assert_eq!(
            middle.child_by_field_name("condition").unwrap().kind(),
            "if_condition"
        );
        let final_branch = middle.child_by_field_name("alternative").unwrap();
        assert_eq!(final_branch.kind(), "else_if_clause");
        assert_eq!(
            final_branch
                .child_by_field_name("condition")
                .unwrap()
                .kind(),
            "if_block_condition"
        );
        assert_eq!(
            final_branch
                .child_by_field_name("alternative")
                .unwrap()
                .kind(),
            "colon_block"
        );
        assert_eq!(body.named_child(1).unwrap().kind(), "call_expression");
    }

    #[test]
    fn an_empty_case_arm_refuses_a_next_line_index_or_brace() {
        for tail in ["[0]", "{Value := 1}"] {
            let text = format!(
                "F():void =\n    case(Mode):\n        0 => Use()\n        _ =>\n    {tail}\n"
            );
            let source = Source::from_bytes(text.as_bytes()).unwrap();
            let error = Document::parse(&source).unwrap_err();
            assert!(
                error.message.contains(INDENTED_EXPRESSION_BOUNDARY_MESSAGE),
                "{tail}: {error:?}"
            );
        }
    }

    #[test]
    fn shallower_empty_wildcard_is_a_sibling_and_ends_the_case() {
        let text = "Paint():void =\n    case(Tone):\n        1 =>\n            Mix()\n     _ =>\n    Seal()\n";
        assert!(Document::parse(&Source::from_bytes(text.as_bytes()).unwrap()).is_ok());
        let mut parser = Parser::new();
        // SAFETY: same statically linked grammar as production.
        parser
            .set_language(&Language::new(unsafe {
                LanguageFn::from_raw(tree_sitter_verse)
            }))
            .unwrap();
        let tree = parser.parse(text, None).unwrap();
        let function = tree.root_node().named_child(0).unwrap();
        let body = function.child_by_field_name("body").unwrap();
        assert_eq!(body.named_child_count(), 2);
        let case = body.named_child(0).unwrap();
        assert_eq!(case.kind(), "case_expression");
        assert_eq!(case.named_child_count(), 3);
        let first = case.named_child(1).unwrap();
        let wildcard = case.named_child(2).unwrap();
        assert_eq!(wildcard.kind(), "case_arm");
        assert_eq!(wildcard.start_position().column, 5);
        assert_eq!(
            first
                .child_by_field_name("body")
                .unwrap()
                .named_child_count(),
            1
        );
        let empty = wildcard.child_by_field_name("body").unwrap();
        assert_eq!(empty.kind(), "indented_block");
        assert_eq!(empty.named_child_count(), 0);
        assert_eq!(body.named_child(1).unwrap().kind(), "call_expression");
    }

    #[test]
    fn shallow_wildcard_refuses_unverified_case_boundaries() {
        for text in [
            // The wildcard must extend the line that contains the colon.
            "Paint():void =\n    case(Tone):\n        1 => Mix()\n    _ =>\n    Seal()\n",
            // This next line would start the wildcard's actual body.
            "Paint():void =\n    case(Tone):\n        1 => Mix()\n     _ =>\n      Seal()\n",
            // The multiline header puts the colon farther in than the wildcard.
            "Paint():void =\n    case(\n        Tone\n      ):\n        1 => Mix()\n     _ =>\n    Seal()\n",
        ] {
            let source = Source::from_bytes(text.as_bytes()).unwrap();
            assert!(Document::parse(&source).is_err(), "{text}");
        }
    }

    #[test]
    fn empty_case_arm_closes_before_an_outer_sibling() {
        let text = "Parcel := module{\nCrate := class:\n    Start():void =\n        case(Mode):\n            0 => Use()\n            _ =>\n\n    Stop():void =\n        Finish()\n}\n";
        let (_, document) = parsed(text);
        let root = document.root();
        let module = root
            .named_child(0)
            .unwrap()
            .child_by_field_name("value")
            .unwrap();
        let class = module
            .named_child(0)
            .unwrap()
            .child_by_field_name("value")
            .unwrap();
        assert_eq!(class.named_child_count(), 2);
        let body = class
            .named_child(0)
            .unwrap()
            .child_by_field_name("body")
            .unwrap();
        let case = body.named_child(0).unwrap();
        assert_eq!(case.kind(), "case_expression");
        let arm = case.named_child(2).unwrap();
        let empty = arm.child_by_field_name("body").unwrap();
        assert_eq!(empty.kind(), "indented_block");
        assert_eq!(empty.named_child_count(), 0);
        let next = class.named_child(1).unwrap();
        assert_eq!(next.kind(), "function_definition");
    }

    #[test]
    fn case_inline_set_owns_its_assignment() {
        let text = "F():void=\n    var Count:int = 0\n    case(Count):\n        0 => set Count = 1\n        _ => {}\n";
        let (_, document) = parsed(text);
        let root = document.root();
        let block = root
            .named_child(0)
            .unwrap()
            .child_by_field_name("body")
            .unwrap();
        let case = block.named_child(1).unwrap();
        assert_eq!(case.kind(), "case_expression");
        let arm = case.named_child(1).unwrap();
        assert_eq!(arm.kind(), "case_arm");
        assert_eq!(
            arm.child_by_field_name("body").unwrap().kind(),
            "set_statement"
        );
        assert_eq!(case.named_child(2).unwrap().kind(), "case_arm");
    }

    #[test]
    fn braced_expression_list_belongs_to_its_declaration() {
        let text = "F():void=\n    Store()\n    Values:={Read(),4}\n    Record:={Count:=2}\n    Use(Values,Record)\n";
        let (_, document) = parsed(text);
        let root = document.root();
        let body = root
            .named_child(0)
            .unwrap()
            .child_by_field_name("body")
            .unwrap();
        assert_eq!(body.named_child_count(), 4);
        assert_eq!(body.named_child(0).unwrap().kind(), "call_expression");
        let assigned = body
            .named_child(1)
            .unwrap()
            .child_by_field_name("value")
            .unwrap();
        assert_eq!(assigned.kind(), "braced_expression_list");
        assert_eq!(assigned.named_child_count(), 2);
        let record = body
            .named_child(2)
            .unwrap()
            .child_by_field_name("value")
            .unwrap();
        assert_eq!(record.kind(), "struct_literal");
    }

    #[test]
    fn line_leading_braced_list_after_a_call_is_refused() {
        let text = "F():void =\n    Store()\n    {Read(), 4}\n";
        let source = Source::from_bytes(text.as_bytes()).unwrap();
        assert!(Document::parse(&source).is_err());
    }

    #[test]
    fn indented_for_keeps_indexed_set_in_its_header() {
        let text = "F():void=\n    for:\n        Item : Items\n        not Seen[Item]\n        set Marks[Item] += array{Item}\n    do:\n        Use(Item)\n";
        let (_, document) = parsed(text);
        let root = document.root();
        let body = root
            .named_child(0)
            .unwrap()
            .child_by_field_name("body")
            .unwrap();
        let loop_node = body.named_child(0).unwrap();
        let clauses = loop_node.child_by_field_name("clauses").unwrap();
        assert_eq!(clauses.named_child_count(), 3);
        let assignment = clauses.named_child(2).unwrap();
        assert_eq!(assignment.kind(), "set_statement");
        assert_eq!(
            assignment.child_by_field_name("operator").unwrap().kind(),
            "+="
        );
        assert_eq!(
            loop_node.child_by_field_name("body").unwrap().kind(),
            "colon_block"
        );
    }

    #[test]
    fn an_adjacent_call_stays_inside_the_first_indented_iterator() {
        let text = "F():void=\n    for:\n        Item : Fetch(Values)\n        not Seen[Item]\n        set Marks[Item] += array{Item}\n    do:\n        Use(Item)\n";
        let (_, document) = parsed(text);
        let root = document.root();
        let body = root
            .named_child(0)
            .unwrap()
            .child_by_field_name("body")
            .unwrap();
        let expression = body.named_child(0).unwrap();
        let clauses = expression.child_by_field_name("clauses").unwrap();
        assert_eq!(clauses.named_child_count(), 3);
        let iterator = clauses.named_child(0).unwrap();
        assert_eq!(iterator.kind(), "for_iterator");
        let iterable = iterator.child_by_field_name("iterable").unwrap();
        assert_eq!(iterable.kind(), "call_expression");
        assert_eq!(
            iterable
                .child_by_field_name("function")
                .unwrap()
                .utf8_text(text.as_bytes())
                .unwrap(),
            "Fetch"
        );
        assert_eq!(clauses.named_child(1).unwrap().kind(), "unary_expression");
    }

    #[test]
    fn for_filter_binding_owns_its_value_and_keeps_body_separate() {
        let text =
            "F():void =\n    for (I -> S : Ss, U := Us[I], U > 0):\n        G(S, U)\n    H()\n";
        let (_, document) = parsed(text);
        let root = document.root();
        assert!(!root.has_error());

        let body = root
            .named_child(0)
            .unwrap()
            .child_by_field_name("body")
            .unwrap();
        assert_eq!(body.named_child_count(), 2);
        let loop_node = body.named_child(0).unwrap();
        let clauses = loop_node.child_by_field_name("clauses").unwrap();
        assert_eq!(clauses.named_child_count(), 3);
        assert_eq!(clauses.named_child(0).unwrap().kind(), "for_iterator");
        let binding = clauses.named_child(1).unwrap();
        assert_eq!(binding.kind(), "for_filter_binding");
        assert_eq!(
            binding
                .child_by_field_name("name")
                .unwrap()
                .utf8_text(text.as_bytes())
                .unwrap(),
            "U"
        );
        assert_eq!(
            binding
                .child_by_field_name("value")
                .unwrap()
                .utf8_text(text.as_bytes())
                .unwrap(),
            "Us[I]"
        );
        assert_eq!(clauses.named_child(2).unwrap().kind(), "binary_expression");
        assert_eq!(
            loop_node
                .child_by_field_name("body")
                .unwrap()
                .named_child_count(),
            1
        );
        assert_eq!(
            body.named_child(1)
                .unwrap()
                .utf8_text(text.as_bytes())
                .unwrap(),
            "H()"
        );
    }

    #[test]
    fn constructor_owns_archetype_without_reclassifying_a_type_function() {
        let text = "Make<public><constructor>(Old:item)<transacts> := item:\n    Count := Old.Count\nAlias(T:type) := T\nOther():int = 1\n";
        let (_, document) = parsed(text);
        let root = document.root();
        assert!(!root.has_error());

        assert_eq!(root.named_child_count(), 3);
        let constructor = root.named_child(0).unwrap();
        assert_eq!(constructor.kind(), "type_definition");
        assert_eq!(
            constructor
                .child_by_field_name("type_params")
                .unwrap()
                .named_child_count(),
            1
        );
        assert_eq!(
            constructor
                .child_by_field_name("effect_specifiers")
                .unwrap()
                .named_child_count(),
            1
        );
        let body = constructor.child_by_field_name("value").unwrap();
        assert_eq!(body.kind(), "object_construction");
        assert_eq!(
            body.child_by_field_name("fields")
                .unwrap()
                .named_child_count(),
            1
        );
        assert_eq!(root.named_child(1).unwrap().kind(), "type_definition");
        assert_eq!(root.named_child(2).unwrap().kind(), "function_definition");
    }

    #[test]
    fn commas_in_indented_enum_keep_variants_and_next_definition_separate() {
        let text = "shade := enum:\n    Light,\n    Grey\n    Dark,\nNext:int = 1\n";
        let (_, document) = parsed(text);
        let root = document.root();
        assert!(!root.has_error());

        assert_eq!(root.named_child_count(), 2);
        let variants = root
            .named_child(0)
            .unwrap()
            .child_by_field_name("value")
            .unwrap();
        assert_eq!(variants.kind(), "enum_definition");
        assert_eq!(variants.named_child_count(), 3);
        for (index, name) in ["Light", "Grey", "Dark"].into_iter().enumerate() {
            let variant = variants.named_child(index).unwrap();
            assert_eq!(variant.kind(), "enum_variant");
            assert_eq!(
                variant
                    .child_by_field_name("name")
                    .unwrap()
                    .utf8_text(text.as_bytes())
                    .unwrap(),
                name
            );
        }
        assert_eq!(root.named_child(1).unwrap().kind(), "constant_declaration");
    }

    #[test]
    fn spaced_effect_specifiers_stay_on_the_signature() {
        let text = "F(N:int) <transacts><decides>:int =\n    F(0) < transacts > G(1)\n    N\nA := F(0) < G(1)\n";
        let (_, document) = parsed(text);
        let root = document.root();
        assert!(!root.has_error());

        assert_eq!(root.named_child_count(), 2);
        let function = root.named_child(0).unwrap();
        assert_eq!(function.kind(), "function_definition");
        let list = function.child_by_field_name("effect_specifiers").unwrap();
        assert_eq!(list.kind(), "specifier_list");
        assert_eq!(list.named_child_count(), 2);
        for specifier in [list.named_child(0).unwrap(), list.named_child(1).unwrap()] {
            assert_eq!(specifier.kind(), "specifier");
        }
        let body = function.child_by_field_name("body").unwrap();
        assert_eq!(body.named_child_count(), 2);
        let first_statement = body.named_child(0).unwrap();
        assert_eq!(first_statement.kind(), "binary_expression");
        assert_eq!(
            first_statement
                .child_by_field_name("operator")
                .unwrap()
                .kind(),
            "<"
        );
        assert_eq!(
            body.named_child(1)
                .unwrap()
                .utf8_text(text.as_bytes())
                .unwrap(),
            "N"
        );
        let comparison = root
            .named_child(1)
            .unwrap()
            .child_by_field_name("value")
            .unwrap();
        assert_eq!(comparison.kind(), "binary_expression");
        assert_eq!(
            comparison.child_by_field_name("operator").unwrap().kind(),
            "<"
        );
    }

    #[test]
    fn empty_archetype_ends_before_its_sibling_statement() {
        let text = "F():item =\n    A:item = item:\n    B:item = item:\n        Count := 1\n    return A\n";
        let (_, document) = parsed(text);
        let root = document.root();
        assert!(!root.has_error());

        let body = root
            .named_child(0)
            .unwrap()
            .child_by_field_name("body")
            .unwrap();
        assert_eq!(body.named_child_count(), 3);
        let empty = body
            .named_child(0)
            .unwrap()
            .child_by_field_name("value")
            .unwrap();
        assert_eq!(empty.kind(), "object_construction");
        assert!(empty.child_by_field_name("fields").is_none());
        let filled = body
            .named_child(1)
            .unwrap()
            .child_by_field_name("value")
            .unwrap();
        assert_eq!(filled.kind(), "object_construction");
        assert_eq!(
            filled
                .child_by_field_name("fields")
                .unwrap()
                .named_child_count(),
            1
        );
        assert_eq!(body.named_child(2).unwrap().kind(), "return_expression");
    }

    #[test]
    fn refuses_expression_continuations_after_an_empty_indented_archetype() {
        for (suffix, message) in [
            ("{Count := 1}", EMPTY_ARCHETYPE_BRACE_MESSAGE),
            ("-2", "a leading unary operand is ambiguous"),
        ] {
            let text = format!("F():void =\n    A:item = item:\n    {suffix}\n");
            let source = Source::from_bytes(text.as_bytes()).unwrap();
            let refusal = Document::parse(&source).expect_err("continuation must refuse");
            assert!(refusal.message.contains(message), "{refusal:?}");
        }
    }

    #[test]
    fn guards_recovery_and_unsupported_constructs() {
        for text in [
            "<# missing",
            "<#> comment\n    body\nCount := 1\n",
            "A := <p>text</p>\n",
            "得点 := 1\n",
            "A:int=\n",
            "A:int\n",
            "using { Demo..Helpers }\n",
            "using { Demo. }\n",
            "using { }\n",
            "shade := enum{Light,,Dark}\n",
            "shade := enum{,Light}\n",
            "A := 1 B := 2\n",
        ] {
            let source = Source::from_bytes(text.as_bytes()).unwrap();
            assert!(Document::parse(&source).is_err(), "{text}");
        }
    }

    /// `docs/parser-design.md` advertises a two-second parse budget. The
    /// predicate is asserted directly, and the real cancel closure is exercised
    /// with a zero budget: making a real parse take two seconds would flake on
    /// a loaded runner, but a zero budget must refuse as soon as any work has
    /// happened, which is deterministic.
    #[test]
    fn parse_budget_predicate_trips_only_after_the_budget_elapses() {
        let budget = Duration::from_secs(2);
        assert!(!parse_budget_exceeded(Duration::ZERO, budget));
        assert!(!parse_budget_exceeded(Duration::from_millis(1_999), budget));
        // The bound is exclusive: exactly the budget is still allowed.
        assert!(!parse_budget_exceeded(budget, budget));
        assert!(parse_budget_exceeded(
            budget + Duration::from_nanos(1),
            budget
        ));
        assert!(parse_budget_exceeded(Duration::from_millis(2_001), budget));
        assert_eq!(PARSE_BUDGET, budget);
    }

    /// A zero budget refuses a real parse instead of returning a partial tree.
    /// The input is large enough that parsing must consume measurable time, so
    /// this does not depend on how fast the machine is.
    #[test]
    fn zero_parse_budget_refuses_a_real_parse() {
        let mut text = String::new();
        for index in 0..2_000 {
            text.push_str(&format!("V{index}:int={index}\n"));
        }
        let source = Source::from_bytes(text.as_bytes()).unwrap();
        let failure = match Document::parse_with_budget(&source, Duration::ZERO) {
            Err(failure) => failure,
            Ok(_) => panic!("a spent budget must refuse the parse"),
        };
        assert!(failure.message.contains("parse budget exceeded"));
        assert_eq!(failure.offset, 0);
        // The same source parses once the documented budget is restored.
        Document::parse(&source).expect("the documented budget accepts this source");
    }

    /// The `Some` branch is a real refusal, not a warning: a source whose CST
    /// is one level past the documented depth must fail, and the depth at the
    /// boundary must parse.
    #[test]
    fn cst_depth_limit_refuses_one_level_past_512_and_accepts_512() {
        // Each `X.Y` member access adds two CST levels. 510 repeats reaches
        // exactly depth 512; one more repeat reaches 513, one past the bound.
        let at_limit = format!("A := X{}\n", ".Y".repeat(510));
        let source = Source::from_bytes(at_limit.as_bytes()).unwrap();
        match Document::parse(&source) {
            Ok(document) => assert!(!document.tokens.is_empty()),
            Err(failure) => panic!("512 CST levels must be supported: {failure}"),
        }

        let past_limit = format!("A := X{}\n", ".Y".repeat(511));
        let source = Source::from_bytes(past_limit.as_bytes()).unwrap();
        let failure = match Document::parse(&source) {
            Err(failure) => failure,
            Ok(_) => panic!("a CST deeper than 512 levels must be refused"),
        };
        assert!(failure.message.contains("syntax tree resource limit"));
        assert!(failure.offset > 0);
    }

    /// `docs/parser-design.md` advertises one million fingerprint events. The
    /// guard refuses at exactly one million recorded events (`>=`), which is
    /// the pre-existing behavior: 999,999 pass and 1,000,000 fail. A real input
    /// needs roughly a million CST events, so the shared guard is asserted here
    /// and the full-size input is gated in `tests/limits-cli.rs`.
    #[test]
    fn fingerprint_event_limit_guard_refuses_at_one_million_events() {
        assert_eq!(tree_limit(0, 0), None);
        assert_eq!(tree_limit(MAX_CST_DEPTH, MAX_FINGERPRINT_EVENTS - 1), None);
        assert_eq!(
            tree_limit(MAX_CST_DEPTH, MAX_FINGERPRINT_EVENTS),
            Some("syntax tree resource limit exceeded")
        );
        assert_eq!(
            tree_limit(MAX_CST_DEPTH + 1, 0),
            Some("syntax tree resource limit exceeded")
        );
    }
}
