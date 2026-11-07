//! Bounded unified-diff rendering for `--fix --diff`.
//!
//! The only edits the safe-fix pipeline can produce are a trailing-trivia
//! deletion inside one physical line and a final-newline insertion at EOF, so a
//! general diff algorithm would be far more machinery than the change needs.
//! Both sides are split into physical lines and compared line by line. When the
//! two sides have the same number of lines — every edit this pipeline can make —
//! the alignment is the identity. When they differ, the common prefix and the
//! common suffix stay context and the middle becomes one replaced block, which is
//! still a valid unified diff and keeps memory flat. No line table is ever larger
//! than four bytes per physical line, because a source is at most 8 MiB.
//!
//! Two display-only normalisations keep the preview readable without changing
//! the on-disk contract (see `docs/fixes.md`): the CR of a CRLF terminator is
//! never printed, and a leading BOM on the first line is never printed. Both
//! stay in the bytes a real `--fix` writes. No other control character reaches
//! the output: everything except TAB is escaped, so a comment cannot smuggle a
//! terminal escape sequence into the preview.

/// Unchanged lines kept on each side of a hunk, the same bound as `diff -U3`.
pub const CONTEXT: usize = 3;

/// The standard unified-diff marker for a last line that has no terminator. It
/// is the only thing that makes a final-newline-only hunk visible at all, so it
/// is not optional noise.
const NO_NEWLINE: &str = "\\ No newline at end of file";

/// One physical line: its text without the terminator, and whether the file
/// ended there without a terminator.
struct Line<'a> {
    text: &'a str,
    terminated: bool,
}

/// A source split into physical lines. Only the start offsets are stored; each
/// line's text is sliced on demand, so the table stays four bytes per line.
struct Table<'a> {
    text: &'a str,
    starts: Vec<u32>,
}

impl<'a> Table<'a> {
    fn new(text: &'a str) -> Self {
        let mut starts = Vec::new();
        if !text.is_empty() {
            starts.push(0);
            for (index, byte) in text.bytes().enumerate() {
                // A trailing '\n' terminates the last line instead of opening an
                // empty one, which is why the bound is strict.
                if byte == b'\n' && index + 1 < text.len() {
                    starts.push(index as u32 + 1);
                }
            }
        }
        Self { text, starts }
    }

    fn len(&self) -> usize {
        self.starts.len()
    }

    fn line(&self, index: usize) -> Line<'a> {
        let start = self.starts[index] as usize;
        let end = self
            .starts
            .get(index + 1)
            .map_or(self.text.len(), |next| *next as usize);
        let raw = &self.text[start..end];
        match raw.strip_suffix('\n') {
            Some(body) => Line {
                text: body.strip_suffix('\r').unwrap_or(body),
                terminated: true,
            },
            None => Line {
                text: raw,
                terminated: false,
            },
        }
    }
}

fn same(left: &Line<'_>, right: &Line<'_>) -> bool {
    left.text == right.text && left.terminated == right.terminated
}

/// How the two sides line up: rows of gated context at the top, one replaced
/// block in the middle, and rows of gated context at the bottom. Equal line
/// counts take the identity alignment and compare every row.
struct Alignment {
    prefix: usize,
    old_middle: usize,
    new_middle: usize,
    suffix: usize,
    equal: bool,
}

/// One aligned position.
enum Row {
    /// The same line on both sides.
    Context(usize),
    /// A replaced line: old index and new index.
    Both(usize, usize),
    Old(usize),
    New(usize),
}

impl Alignment {
    fn new(old: &Table<'_>, new: &Table<'_>) -> Self {
        if old.len() == new.len() {
            return Self {
                prefix: old.len(),
                old_middle: 0,
                new_middle: 0,
                suffix: 0,
                equal: true,
            };
        }
        let mut prefix = 0;
        while prefix < old.len() && prefix < new.len() && same(&old.line(prefix), &new.line(prefix))
        {
            prefix += 1;
        }
        let mut suffix = 0;
        while suffix < old.len() - prefix
            && suffix < new.len() - prefix
            && same(
                &old.line(old.len() - 1 - suffix),
                &new.line(new.len() - 1 - suffix),
            )
        {
            suffix += 1;
        }
        Self {
            prefix,
            old_middle: old.len() - prefix - suffix,
            new_middle: new.len() - prefix - suffix,
            suffix,
            equal: false,
        }
    }

    fn rows(&self) -> usize {
        if self.equal {
            self.prefix
        } else {
            self.prefix + self.old_middle + self.new_middle + self.suffix
        }
    }

    fn row(&self, index: usize, old: &Table<'_>, new: &Table<'_>) -> Row {
        if self.equal {
            return if same(&old.line(index), &new.line(index)) {
                Row::Context(index)
            } else {
                Row::Both(index, index)
            };
        }
        if index < self.prefix {
            return Row::Context(index);
        }
        if index < self.prefix + self.old_middle {
            return Row::Old(index);
        }
        let new_start = self.prefix + self.old_middle;
        if index < new_start + self.new_middle {
            return Row::New(self.prefix + index - new_start);
        }
        let offset = index - new_start - self.new_middle;
        Row::Context(old.len() - self.suffix + offset)
    }

    /// How many rows of each side precede `index`, which is the hunk numbering.
    fn counts_before(&self, index: usize) -> (usize, usize) {
        if self.equal {
            return (index, index);
        }
        let prefix = index.min(self.prefix);
        let old_middle = index.saturating_sub(self.prefix).min(self.old_middle);
        let new_middle = index
            .saturating_sub(self.prefix + self.old_middle)
            .min(self.new_middle);
        let suffix = index
            .saturating_sub(self.prefix + self.old_middle + self.new_middle)
            .min(self.suffix);
        (prefix + old_middle + suffix, prefix + new_middle + suffix)
    }
}

/// Inclusive row ranges to render: one per changed region, widened by
/// [`CONTEXT`] and merged when two regions are close enough that their context
/// would overlap. A gap of `2 * CONTEXT` unchanged lines still merges, which is
/// what `diff -U3` does; one more splits the region into two hunks.
fn hunk_ranges(changed: &[usize], total: usize) -> Vec<(usize, usize)> {
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for &index in changed {
        match ranges.last_mut() {
            Some((_, end)) if index - *end - 1 <= 2 * CONTEXT => *end = index,
            _ => ranges.push((index, index)),
        }
    }
    ranges
        .into_iter()
        .map(|(start, end)| {
            (
                start.saturating_sub(CONTEXT),
                (end + CONTEXT).min(total.saturating_sub(1)),
            )
        })
        .collect()
}

/// A line as the diff prints it: no leading BOM on the first line, TAB kept,
/// every other control character escaped so no terminal sequence is emitted.
fn printable(line: &Line<'_>, first: bool) -> String {
    let text = if first {
        line.text.strip_prefix('\u{feff}').unwrap_or(line.text)
    } else {
        line.text
    };
    let mut printed = String::with_capacity(text.len());
    for character in text.chars() {
        if character == '\t' {
            printed.push('\t');
        } else if character.is_control() {
            printed.push_str(&format!("\\u{{{:04X}}}", u32::from(character)));
        } else {
            printed.push(character);
        }
    }
    printed
}

fn push(out: &mut String, prefix: char, table: &Table<'_>, index: usize) {
    let line = table.line(index);
    out.push(prefix);
    out.push_str(&printable(&line, index == 0));
    out.push('\n');
    if index + 1 == table.len() && !line.terminated {
        out.push_str(NO_NEWLINE);
        out.push('\n');
    }
}

/// The unified diff for one changed file, or an empty string when `before` and
/// `after` read the same. `label` is the `/`-separated config-root-relative path
/// every other output format already reports.
pub fn render(label: &str, before: &str, after: &str) -> String {
    let old = Table::new(before);
    let new = Table::new(after);
    let alignment = Alignment::new(&old, &new);
    let changed: Vec<usize> = (0..alignment.rows())
        .filter(|index| !matches!(alignment.row(*index, &old, &new), Row::Context(_)))
        .collect();
    let ranges = hunk_ranges(&changed, alignment.rows());
    if ranges.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    out.push_str(&format!("--- a/{label}\n+++ b/{label}\n"));
    for (start, end) in ranges {
        let mut old_count = 0;
        let mut new_count = 0;
        for index in start..=end {
            match alignment.row(index, &old, &new) {
                Row::Context(_) => {
                    old_count += 1;
                    new_count += 1;
                }
                Row::Both(..) => {
                    old_count += 1;
                    new_count += 1;
                }
                Row::Old(_) => old_count += 1,
                Row::New(_) => new_count += 1,
            }
        }
        // A hunk with no line on one side names the line before the gap, which
        // is the numbering a reader of `diff -U3` already knows.
        let (old_before, new_before) = alignment.counts_before(start);
        let old_start = if old_count == 0 {
            old_before
        } else {
            old_before + 1
        };
        let new_start = if new_count == 0 {
            new_before
        } else {
            new_before + 1
        };
        out.push_str(&format!(
            "@@ -{old_start},{old_count} +{new_start},{new_count} @@\n"
        ));
        for index in start..=end {
            match alignment.row(index, &old, &new) {
                Row::Context(line) => push(&mut out, ' ', &old, line),
                Row::Both(old_line, new_line) => {
                    push(&mut out, '-', &old, old_line);
                    push(&mut out, '+', &new, new_line);
                }
                Row::Old(line) => push(&mut out, '-', &old, line),
                Row::New(line) => push(&mut out, '+', &new, line),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_text_renders_nothing() {
        assert_eq!(render("a.verse", "A := 1\n", "A := 1\n"), "");
        assert_eq!(render("a.verse", "", ""), "");
    }

    #[test]
    fn a_trailing_whitespace_edit_keeps_three_lines_of_context() {
        assert_eq!(
            render(
                "a.verse",
                "L1\nL2\nL3\nL4\nL5  \nL6\nL7\nL8\n",
                "L1\nL2\nL3\nL4\nL5\nL6\nL7\nL8\n"
            ),
            "--- a/a.verse\n+++ b/a.verse\n@@ -2,7 +2,7 @@\n L2\n L3\n L4\n-L5  \n+L5\n L6\n L7\n L8\n"
        );
    }

    #[test]
    fn context_is_clamped_to_the_first_and_last_line() {
        assert_eq!(
            render("a.verse", "A  \nB\n", "A\nB\n"),
            "--- a/a.verse\n+++ b/a.verse\n@@ -1,2 +1,2 @@\n-A  \n+A\n B\n"
        );
    }

    #[test]
    fn the_final_newline_marker_is_the_only_visible_difference() {
        assert_eq!(
            render("a.verse", "A := 1", "A := 1\n"),
            "--- a/a.verse\n+++ b/a.verse\n@@ -1,1 +1,1 @@\n-A := 1\n\\ No newline at end of file\n+A := 1\n"
        );
    }

    #[test]
    fn both_fix_kinds_on_one_last_line_render_one_changed_row() {
        assert_eq!(
            render("a.verse", "A := 1  ", "A := 1\n"),
            "--- a/a.verse\n+++ b/a.verse\n@@ -1,1 +1,1 @@\n-A := 1  \n\\ No newline at end of file\n+A := 1\n"
        );
    }

    #[test]
    fn an_unterminated_context_line_keeps_its_marker() {
        assert_eq!(
            render("a.verse", "A := 1  \nB := 2", "A := 1\nB := 2"),
            "--- a/a.verse\n+++ b/a.verse\n@@ -1,2 +1,2 @@\n-A := 1  \n+A := 1\n B := 2\n\\ No newline at end of file\n"
        );
    }

    #[test]
    fn trailing_whitespace_only_lines_render_as_an_empty_line() {
        assert_eq!(
            render("a.verse", "   ", "\n"),
            "--- a/a.verse\n+++ b/a.verse\n@@ -1,1 +1,1 @@\n-   \n\\ No newline at end of file\n+\n"
        );
    }

    #[test]
    fn hunk_gap_boundary_matches_a_three_line_context() {
        let old: String = (1..=12)
            .map(|line| {
                if line == 1 || line == 9 {
                    format!("L{line}  \n")
                } else {
                    format!("L{line}\n")
                }
            })
            .collect();
        let new = old.replace("  \n", "\n");
        let rendered = render("a.verse", &old, &new);
        // Rows 0 and 8 are seven unchanged lines apart: two hunks.
        assert!(rendered.contains("@@ -1,4 +1,4 @@"), "{rendered}");
        assert!(rendered.contains("@@ -6,7 +6,7 @@"), "{rendered}");
        assert_eq!(rendered.matches("@@ -").count(), 2, "{rendered}");

        // Rows 0 and 7 are six unchanged lines apart: one hunk.
        let old: String = (1..=12)
            .map(|line| {
                if line == 1 || line == 8 {
                    format!("L{line}  \n")
                } else {
                    format!("L{line}\n")
                }
            })
            .collect();
        let new = old.replace("  \n", "\n");
        let rendered = render("a.verse", &old, &new);
        assert_eq!(rendered.matches("@@ -").count(), 1, "{rendered}");
        assert!(rendered.contains("@@ -1,11 +1,11 @@"), "{rendered}");
    }

    #[test]
    fn a_line_count_change_still_renders_one_replaced_block() {
        // The safe-fix pipeline never changes the line count today, so this
        // guards the fallback rather than a reachable edit: a future fix that
        // adds or removes a line must not split the alignment into nonsense.
        assert_eq!(
            render("a.verse", "A\nB\nC\nD\n", "A\nB\nC2\nC3\nD\n"),
            "--- a/a.verse\n+++ b/a.verse\n@@ -1,4 +1,5 @@\n A\n B\n-C\n+C2\n+C3\n D\n"
        );
        assert_eq!(
            render("a.verse", "A\nB\nC\n", "A\nC\n"),
            "--- a/a.verse\n+++ b/a.verse\n@@ -1,3 +1,2 @@\n A\n-B\n C\n"
        );
        // A removed first line and an added last line are still numbered from 1.
        assert_eq!(
            render("a.verse", "A\n", "B\n"),
            "--- a/a.verse\n+++ b/a.verse\n@@ -1,1 +1,1 @@\n-A\n+B\n"
        );
    }

    #[test]
    fn crlf_and_bom_are_not_printed_but_the_marker_is_not_an_edit() {
        assert_eq!(
            render(
                "a.verse",
                "\u{feff}A := 1  \r\nB := 2\r\n",
                "\u{feff}A := 1\r\nB := 2\r\n"
            ),
            "--- a/a.verse\n+++ b/a.verse\n@@ -1,2 +1,2 @@\n-A := 1  \n+A := 1\n B := 2\n"
        );
    }

    #[test]
    fn only_tab_survives_a_control_character() {
        let rendered = render(
            "a.verse",
            "A\u{7}:=\"\u{1b}[2J\"  \n",
            "A\u{7}:=\"\u{1b}[2J\"\n",
        );
        assert!(
            rendered.contains("-A\\u{0007}:=\"\\u{001B}[2J\"  \n"),
            "{rendered}"
        );
        assert!(!rendered.contains('\u{7}'), "{rendered}");
        assert!(!rendered.contains('\u{1b}'), "{rendered}");
        // A trailing tab is the subject of a V1001 fix, so it stays literal.
        let rendered = render("a.verse", "A\t\n", "A\n");
        assert!(rendered.contains("-A\t\n"), "{rendered}");
    }
}
