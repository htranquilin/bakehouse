//! T-SQL batch splitter. `GO` is a client-side batch separator, never sent to
//! the server. It only counts when it stands alone on a line (optionally with a
//! repeat count: `GO 3`), outside string literals, comments, and brackets.

#[derive(Debug, PartialEq)]
pub struct Batch {
    pub sql: String,
    /// 1-based line number where this batch starts in the original buffer
    /// (used to translate server error line numbers to editor lines).
    pub start_line: u32,
    pub repeat: u32,
}

pub fn split_batches(input: &str) -> Vec<Batch> {
    #[derive(PartialEq)]
    enum S {
        Normal,
        SingleQuote,
        DoubleQuote,
        Bracket,
        LineComment,
        BlockComment(u32), // nesting depth; T-SQL block comments nest
    }

    let mut batches = Vec::new();
    let mut current = String::new();
    let mut current_start_line: u32 = 1;
    let mut line_no: u32 = 1;
    let mut state = S::Normal;
    let mut line_buf = String::new();

    let flush =
        |batches: &mut Vec<Batch>, current: &mut String, start_line: u32, repeat: u32| {
            if !current.trim().is_empty() {
                batches.push(Batch { sql: current.clone(), start_line, repeat });
            }
            current.clear();
        };

    // Process line by line; the state machine carries across lines.
    let mut lines = input.split_inclusive('\n').peekable();
    while let Some(line) = lines.next() {
        line_buf.clear();
        line_buf.push_str(line);

        // A GO line is only recognizable when we ENTER the line outside any
        // string/comment construct, and the line itself is `GO [n]` (plus an
        // optional trailing `;` or line comment).
        if state == S::Normal {
            if let Some(repeat) = parse_go_line(line) {
                flush(&mut batches, &mut current, current_start_line, repeat);
                line_no += 1;
                current_start_line = line_no;
                continue;
            }
        }

        // Advance the state machine over this line's characters.
        let bytes: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i < bytes.len() {
            let c = bytes[i];
            let next = bytes.get(i + 1).copied();
            match state {
                S::Normal => match (c, next) {
                    ('\'', _) => state = S::SingleQuote,
                    ('"', _) => state = S::DoubleQuote,
                    ('[', _) => state = S::Bracket,
                    ('-', Some('-')) => {
                        state = S::LineComment;
                        i += 1;
                    }
                    ('/', Some('*')) => {
                        state = S::BlockComment(1);
                        i += 1;
                    }
                    _ => {}
                },
                S::SingleQuote => {
                    if c == '\'' {
                        if next == Some('\'') {
                            i += 1; // escaped ''
                        } else {
                            state = S::Normal;
                        }
                    }
                }
                S::DoubleQuote => {
                    if c == '"' {
                        state = S::Normal;
                    }
                }
                S::Bracket => {
                    if c == ']' {
                        if next == Some(']') {
                            i += 1; // escaped ]]
                        } else {
                            state = S::Normal;
                        }
                    }
                }
                S::LineComment => {} // ends at end of line
                S::BlockComment(depth) => match (c, next) {
                    ('*', Some('/')) => {
                        i += 1;
                        state = if depth == 1 { S::Normal } else { S::BlockComment(depth - 1) };
                    }
                    ('/', Some('*')) => {
                        i += 1;
                        state = S::BlockComment(depth + 1);
                    }
                    _ => {}
                },
            }
            i += 1;
        }
        if state == S::LineComment {
            state = S::Normal;
        }

        current.push_str(&line_buf);
        if line.ends_with('\n') {
            line_no += 1;
        }
        if current.trim().is_empty() {
            // Only whitespace so far — let the batch "start" track forward so
            // error lines don't point at leading blank lines.
            current_start_line = line_no;
            if lines.peek().is_none() {
                break;
            }
        }
    }
    flush(&mut batches, &mut current, current_start_line, 1);
    batches
}

/// `GO`, `go 5`, `GO;`, `GO -- comment` are separators; anything else is not.
fn parse_go_line(line: &str) -> Option<u32> {
    let trimmed = line.trim();
    let rest = trimmed
        .strip_prefix("GO")
        .or_else(|| trimmed.strip_prefix("go"))
        .or_else(|| trimmed.strip_prefix("Go"))
        .or_else(|| trimmed.strip_prefix("gO"))?;
    // `GO` must be followed by whitespace, `;`, a comment, or end of line —
    // `GO2` or `GOTO` are not separators.
    if !rest.is_empty()
        && !rest.starts_with([' ', '\t', ';'])
        && !rest.starts_with("--")
    {
        return None;
    }
    let rest = rest.trim_start();
    if rest.is_empty() || rest == ";" {
        return Some(1);
    }
    if let Some(comment) = rest.strip_prefix("--").map(|_| 1u32) {
        return Some(comment);
    }
    // A repeat count, optionally followed by ; or a line comment.
    let (num, tail) = rest.split_at(rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len()));
    if num.is_empty() {
        return None;
    }
    let tail = tail.trim();
    if !(tail.is_empty() || tail == ";" || tail.starts_with("--")) {
        return None;
    }
    num.parse::<u32>().ok().map(|n| n.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sqls(input: &str) -> Vec<String> {
        split_batches(input).into_iter().map(|b| b.sql.trim().to_string()).collect()
    }

    #[test]
    fn single_batch_no_go() {
        assert_eq!(sqls("SELECT 1"), vec!["SELECT 1"]);
    }

    #[test]
    fn splits_on_go() {
        assert_eq!(sqls("SELECT 1\nGO\nSELECT 2"), vec!["SELECT 1", "SELECT 2"]);
    }

    #[test]
    fn go_case_insensitive_and_semicolon() {
        assert_eq!(sqls("SELECT 1\ngo\nSELECT 2\nGO;\nSELECT 3"), vec!["SELECT 1", "SELECT 2", "SELECT 3"]);
    }

    #[test]
    fn go_with_repeat() {
        let b = split_batches("INSERT INTO t DEFAULT VALUES\nGO 3\n");
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].repeat, 3);
    }

    #[test]
    fn go_inside_string_not_split() {
        let sql = "SELECT 'line1\nGO\nline2'";
        assert_eq!(sqls(sql), vec![sql]);
    }

    #[test]
    fn go_inside_block_comment_not_split() {
        let sql = "SELECT 1 /* hello\nGO\nworld */ + 1";
        assert_eq!(sqls(sql), vec![sql]);
    }

    #[test]
    fn nested_block_comments() {
        let sql = "/* a /* nested */\nGO\n still comment */ SELECT 9";
        assert_eq!(sqls(sql).len(), 1);
    }

    #[test]
    fn go_inside_brackets_not_split() {
        let sql = "SELECT [weird\nGO\ncolumn] FROM t";
        assert_eq!(sqls(sql), vec![sql]);
    }

    #[test]
    fn escaped_quotes() {
        let sql = "SELECT 'it''s\nGO\nfine'";
        assert_eq!(sqls(sql), vec![sql]);
    }

    #[test]
    fn go_with_trailing_comment() {
        assert_eq!(sqls("SELECT 1\nGO -- next\nSELECT 2").len(), 2);
    }

    #[test]
    fn goto_is_not_go() {
        let sql = "GOTO label\nGO2\nSELECT 1";
        assert_eq!(sqls(sql).len(), 1);
    }

    #[test]
    fn start_lines_track_correctly() {
        let b = split_batches("SELECT 1\nGO\n\nSELECT 2\nGO\nSELECT 3");
        assert_eq!(b[0].start_line, 1);
        assert_eq!(b[1].start_line, 4); // blank line 3 is skipped; SELECT 2 sits on line 4
        assert_eq!(b[2].start_line, 6);
    }

    #[test]
    fn create_view_batches() {
        let input = "IF OBJECT_ID('v') IS NOT NULL DROP VIEW v\nGO\nCREATE VIEW v AS SELECT 1 AS one\nGO\nSELECT * FROM v";
        let b = split_batches(input);
        assert_eq!(b.len(), 3);
        assert!(b[1].sql.trim().starts_with("CREATE VIEW"));
    }

    #[test]
    fn empty_batches_dropped() {
        assert_eq!(sqls("GO\nGO\nSELECT 1\nGO\nGO").len(), 1);
    }
}
