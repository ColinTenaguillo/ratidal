use std::path::Path;

fn sources_in(dir: &str) -> Vec<(String, String)> {
    fn walk(dir: &Path, out: &mut Vec<(String, String)>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    out.push((path.display().to_string(), text));
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(Path::new(dir), &mut out);
    assert!(!out.is_empty(), "no sources found in {dir} — is the path right?");
    out
}

/// Byte offset just past the `}` closing the block that starts at `s[0] == '{'`.
///
/// Counts braces only in real code: string, raw-string, and char literals and
/// comments are skipped, because a brace inside one is not a delimiter. A
/// naive counter is silently fooled by a fixture like `"unexpected { input"`,
/// which desyncs the depth and swallows every line after the test module —
/// the guard then passes no matter what it is pointed at.
fn end_of_block(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut i = 0usize;
    let mut depth = 0usize;

    while i < b.len() {
        match b[i] {
            b'{' => {
                depth += 1;
                i += 1;
            }
            b'}' => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
                i += 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            b'/' if b.get(i + 1) == Some(&b'/') => {
                i += s[i..].find('\n')? + 1;
            }
            // Block comment; Rust nests these.
            b'/' if b.get(i + 1) == Some(&b'*') => {
                let mut nest = 1usize;
                i += 2;
                while i < b.len() && nest > 0 {
                    if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
                        nest += 1;
                        i += 2;
                    } else if b[i] == b'*' && b.get(i + 1) == Some(&b'/') {
                        nest -= 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
            }
            // Raw string: r"…", r#"…"#, br##"…"##, and so on.
            b'r' | b'b' if raw_string_start(b, i).is_some() => {
                let (hashes, body) = raw_string_start(b, i)?;
                let mut close = String::with_capacity(hashes + 1);
                close.push('"');
                for _ in 0..hashes {
                    close.push('#');
                }
                i = body + s[body..].find(&close)? + close.len();
            }
            b'"' => {
                i += 1;
                while i < b.len() {
                    match b[i] {
                        b'\\' => i += 2,
                        b'"' => {
                            i += 1;
                            break;
                        }
                        _ => i += 1,
                    }
                }
            }
            // Char literal, distinguished from a lifetime (`'a`) by the
            // closing quote.
            b'\'' => {
                let end = if b.get(i + 1) == Some(&b'\\') {
                    // Escape: step over the escaped character before looking
                    // for the closer, or `'\''` finds its own escaped quote.
                    s[i + 3..].find('\'').map(|n| i + 3 + n + 1)
                } else if b.get(i + 2) == Some(&b'\'') {
                    Some(i + 3)
                } else {
                    None
                };
                i = end.unwrap_or(i + 1);
            }
            _ => i += 1,
        }
    }
    None
}

/// If a raw-string literal starts at `i`, return `(hash count, body offset)`.
fn raw_string_start(b: &[u8], i: usize) -> Option<(usize, usize)> {
    let mut j = i;
    if b[j] == b'b' {
        j += 1;
    }
    if b.get(j) != Some(&b'r') {
        return None;
    }
    j += 1;
    let hashes_start = j;
    while b.get(j) == Some(&b'#') {
        j += 1;
    }
    if b.get(j) != Some(&b'"') {
        return None;
    }
    Some((j - hashes_start, j + 1))
}

/// Find the next `#[cfg(test)]` marker that is real code — not a substring
/// inside a string literal, and not inside a comment.
///
/// Two rules, and both are needed. A real attribute sits alone at the start of
/// its line, which rules out a match inside a fixture string. And it is not
/// commented out — a marker at a line start inside a `/* … */` block matches
/// the first rule perfectly, then `without_tests` finds no `{` after it and
/// discards the rest of the file, taking any real violation with it. That is
/// the same blind-guard failure this file has already produced twice.
fn find_marker(text: &str) -> Option<usize> {
    let mut offset = 0usize;
    // Depth of `/* … */` nesting carried ACROSS lines.
    let mut block_depth = 0usize;

    for line in text.split_inclusive('\n') {
        let indent = line.len() - line.trim_start().len();

        if block_depth == 0 && line.trim_start().starts_with("#[cfg(test)]") {
            return Some(offset + indent);
        }

        // Advance comment state over this line: count block openers and
        // closers, and stop at a line comment (which cannot open a block).
        let bytes = line.as_bytes();
        let mut i = 0usize;
        while i + 1 < bytes.len() {
            if block_depth == 0 && bytes[i] == b'/' && bytes[i + 1] == b'/' {
                break;
            } else if bytes[i] == b'/' && bytes[i + 1] == b'*' {
                block_depth += 1;
                i += 2;
            } else if bytes[i] == b'*' && bytes[i + 1] == b'/' && block_depth > 0 {
                block_depth -= 1;
                i += 2;
            } else {
                i += 1;
            }
        }

        offset += line.len();
    }
    None
}

/// Strip test modules: a test may legitimately reach for anything.
fn without_tests(text: &str) -> String {
    // Remove each `#[cfg(test)] mod … { … }` block by brace matching, rather
    // than truncating at the first marker. Truncating hides everything after
    // the test module — a violation placed below it would sail through, which
    // makes the whole suite unable to fail.
    let mut out = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(marker) = find_marker(rest) {
        out.push_str(&rest[..marker]);
        let after = &rest[marker..];

        let Some(open) = after.find('{') else {
            // No block follows: drop the remainder, nothing to scan.
            return out;
        };
        match end_of_block(&after[open..]) {
            Some(e) => rest = &after[open + e..],
            // Unbalanced braces: stop rather than risk scanning test code.
            None => return out,
        }
    }

    out.push_str(rest);
    out
}

#[test]
fn components_never_import_the_shell() {
    // Dependencies point inward. The shell knows the components; a component
    // that knows the shell has inverted the arrow.
    for dir in ["src/auth", "src/library", "src/playback", "src/tidal", "src/domain"] {
        for (path, text) in sources_in(dir) {
            let body = without_tests(&text);
            assert!(
                !body.contains("crate::shell") && !body.contains("use super::super::shell"),
                "{path} imports the shell; components must not depend on the UI"
            );
        }
    }
}

#[test]
fn playback_never_imports_ratatui() {
    // The audio path must be testable without a terminal.
    for (path, text) in sources_in("src/playback") {
        let body = without_tests(&text);
        assert!(
            !body.contains("ratatui"),
            "{path} imports ratatui; playback must not depend on the UI toolkit"
        );
    }
}

#[test]
fn the_domain_depends_on_nothing_of_ours() {
    // domain/ is the core everything points at; it points at nothing.
    for (path, text) in sources_in("src/domain") {
        let body = without_tests(&text);
        for forbidden in ["crate::tidal", "crate::auth", "crate::playback",
                          "crate::library", "crate::shell", "ratatui", "reqwest"] {
            assert!(
                !body.contains(forbidden),
                "{path} references {forbidden}; the domain must stay dependency-free"
            );
        }
    }
}

#[test]
fn component_internals_stay_crate_private() {
    // The design's whole argument for the facade is that internals are
    // `pub(crate)`, so nothing outside can couple to them. Without this test
    // that claim was decorative: every submodule was `pub mod`, and an example
    // was importing `SegmentReader` from outside the crate — the exact
    // coupling the facade exists to prevent.
    //
    // `store` is deliberately exempt: the shell persists and clears tokens
    // through it, so it is part of `auth`'s public surface.
    let exempt = [("src/auth/mod.rs", "store")];

    for facade in [
        "src/auth/mod.rs",
        "src/playback/mod.rs",
        "src/tidal/mod.rs",
    ] {
        let Ok(text) = std::fs::read_to_string(facade) else {
            panic!("{facade} is missing — did a module move?");
        };
        for (n, line) in without_tests(&text).lines().enumerate() {
            let code = line.split("//").next().unwrap_or("").trim();
            if let Some(rest) = code.strip_prefix("pub mod ") {
                let name = rest.trim_end_matches(';');
                if exempt.contains(&(facade, name)) {
                    continue;
                }
                panic!(
                    "{facade}:{} declares `pub mod {name};` — a component's \
                     submodules must be `pub(crate)` so only the facade's \
                     re-exports escape the crate",
                    n + 1
                );
            }
        }
    }
}

#[test]
fn no_unwrap_outside_tests_and_main() {
    // A panic in raw mode wrecks the user's shell.
    for dir in ["src/auth", "src/library", "src/playback", "src/tidal",
                "src/domain", "src/shell", "src/config"] {
        for (path, text) in sources_in(dir) {
            let body = without_tests(&text);
            for (n, line) in body.lines().enumerate() {
                let code = line.split("//").next().unwrap_or("");
                assert!(
                    !code.contains(".unwrap()"),
                    "{path}:{} uses .unwrap(); return a Result instead",
                    n + 1
                );
            }
        }
    }
}

// The guard needs its own guard. A stripper that silently swallows the rest of
// a file makes every test above pass no matter what the code does, so these
// pin the two failure modes that have actually bitten: truncating at the marker,
// and counting braces that live inside literals.

#[test]
fn without_tests_keeps_code_after_a_test_module() {
    let src = concat!(
        "pub fn a() {}\n",
        "#[cfg(test)]\nmod tests { #[test] fn t() {} }\n",
        "use crate::shell::Bad;\n"
    );
    assert!(
        without_tests(src).contains("crate::shell::Bad"),
        "code after a test module must stay visible"
    );
}

#[test]
fn without_tests_ignores_braces_inside_string_literals() {
    // An unbalanced brace in a fixture used to desync the depth counter and
    // swallow every line below it.
    let src = concat!(
        "#[cfg(test)]\nmod tests {\n",
        "    fn t() { let s = \"unexpected { input\"; }\n}\n",
        "use crate::shell::Bad;\n"
    );
    assert!(
        without_tests(src).contains("crate::shell::Bad"),
        "a brace inside a string must not be treated as a delimiter"
    );
}

#[test]
fn without_tests_ignores_braces_inside_raw_strings_and_comments() {
    let src = concat!(
        "#[cfg(test)]\nmod tests {\n",
        "    fn t() { let j = r#\"{\"a\": \"{\"}\"#; }\n",
        "    // a stray } in a comment\n}\n",
        "use crate::shell::Bad;\n"
    );
    assert!(
        without_tests(src).contains("crate::shell::Bad"),
        "raw strings and comments must not affect brace depth"
    );
}

#[test]
fn without_tests_ignores_a_marker_inside_a_block_comment() {
    // A commented-out marker sits at a line start just like a real attribute.
    // Treating it as real means no `{` follows, so the whole remainder gets
    // discarded — silently hiding the violation below it.
    let src = concat!(
        "pub fn a() {}\n",
        "/*\n#[cfg(test)]\n*/\n",
        "use crate::shell::Bad;\n"
    );
    assert!(
        without_tests(src).contains("crate::shell::Bad"),
        "a marker inside a block comment must not strip the file"
    );
}

#[test]
fn without_tests_ignores_a_marker_inside_a_line_comment() {
    let src = concat!(
        "pub fn a() {}\n",
        "// #[cfg(test)]\n",
        "use crate::shell::Bad;\n"
    );
    assert!(
        without_tests(src).contains("crate::shell::Bad"),
        "a marker inside a line comment must not strip the file"
    );
}

#[test]
fn without_tests_still_finds_a_marker_after_a_block_comment_closes() {
    // The converse: comment state must not leak past the closing `*/`, or a
    // real test module after one would be scanned as production code.
    let src = concat!(
        "/* a comment */\n",
        "#[cfg(test)]\nmod tests { fn t() { let _ = x.unwrap(); } }\n",
        "use crate::shell::Bad;\n"
    );
    let body = without_tests(src);
    assert!(!body.contains(".unwrap()"), "the real test module must still be stripped");
    assert!(body.contains("crate::shell::Bad"), "code after it must survive");
}

#[test]
fn without_tests_handles_escaped_quote_char_literals() {
    // `'\''` is four bytes: a naive scan for the closing quote finds the
    // ESCAPED one and stops a byte early, leaving a stray quote to reprocess.
    let src = concat!(
        "#[cfg(test)]\nmod tests {\n",
        "    fn t() { let q = '\\''; let s = \"{\"; }\n}\n",
        "use crate::shell::Bad;\n"
    );
    assert!(
        without_tests(src).contains("crate::shell::Bad"),
        "an escaped-quote char literal must not desync the scan"
    );
}

#[test]
fn without_tests_still_removes_the_test_body() {
    let src = concat!(
        "pub fn a() {}\n",
        "#[cfg(test)]\nmod tests { fn t() { let _ = x.unwrap(); } }\n"
    );
    let body = without_tests(src);
    assert!(!body.contains(".unwrap()"), "test bodies must still be stripped");
    assert!(body.contains("pub fn a()"), "real code must survive");
}

#[test]
fn without_tests_ignores_the_marker_inside_a_string_literal() {
    // A source file containing the literal text "#[cfg(test)]" inside a
    // string must not be mistaken for a real attribute — that would strip
    // everything below it, hiding violations.
    let src = concat!(
        "pub fn a() {}\n",
        "const MARKER: &str = \"#[cfg(test)]\";\n",
        "use crate::shell::Bad;\n"
    );
    assert!(
        without_tests(src).contains("crate::shell::Bad"),
        "a marker inside a string literal must not truncate the scan"
    );
}

#[test]
fn end_of_block_does_not_panic_on_a_stray_leading_close_brace() {
    assert_eq!(end_of_block("}"), None);
    assert_eq!(end_of_block("} { }"), None);
}

#[test]
fn without_tests_handles_two_test_modules() {
    let src = concat!(
        "#[cfg(test)]\nmod a { fn x() {} }\n",
        "pub fn between() {}\n",
        "#[cfg(test)]\nmod b { fn y() {} }\n",
        "use crate::shell::Bad;\n"
    );
    let body = without_tests(src);
    assert!(body.contains("pub fn between()"), "code between test modules survives");
    assert!(
        body.contains("crate::shell::Bad"),
        "code after the last test module survives"
    );
}
