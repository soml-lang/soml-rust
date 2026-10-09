/*!
The formatter: it keeps the value, the comments, and the spelling, it is idempotent, and it lays documents out as the spec's formatting rules say.
*/

use soml::{Document, Value};
use std::fs;
use std::path::Path;

fn format(text: &str) -> String {
	soml::format(text).unwrap_or_else(|error| panic!("{error}: {text:?}"))
}

/**
The comment texts of a document, sorted, so a formatted document can be checked to have the same ones.
*/
fn comment_texts(text: &str) -> Vec<String> {
	let mut texts: Vec<String> = text
		.parse::<Document>()
		.expect("valid")
		.comments()
		.into_iter()
		// The formatter removes trailing whitespace and blank lines inside a block comment.
		.map(|comment| {
			comment
				.text
				.split('\n')
				.map(|line| line.trim_end_matches([' ', '\t']))
				.filter(|line| !line.is_empty())
				.collect::<Vec<_>>()
				.join("\n")
		})
		.collect();
	texts.sort();
	texts
}

/**
Checks every property the formatter must keep for one document.
*/
fn check_properties(name: &str, text: &str) -> Result<(), String> {
	let formatted = soml::format(text).map_err(|error| format!("{name}: {error}"))?;
	let value: Value = formatted.parse().map_err(|error| {
		format!("{name}: the formatted text is not valid: {error}\n{formatted}")
	})?;
	let expected: Value = text.parse().expect("valid");

	if value != expected {
		return Err(format!("{name}: the value changed\n{formatted}"));
	}

	let again = soml::format(&formatted).expect("valid");

	if again != formatted {
		return Err(format!(
			"{name}: formatting is not idempotent\n--- once\n{formatted}\n--- twice\n{again}"
		));
	}

	if comment_texts(text) != comment_texts(&formatted) {
		return Err(format!("{name}: the comments changed\n{formatted}"));
	}

	if formatted.lines().any(|line| line.ends_with([' ', '\t']))
		&& !text.contains("'''")
		&& !text.contains("\"\"\"")
	{
		return Err(format!("{name}: trailing whitespace\n{formatted:?}"));
	}

	if !formatted.ends_with('\n') || formatted.ends_with("\n\n") {
		return Err(format!(
			"{name}: does not end with exactly one line feed\n{formatted:?}"
		));
	}

	Ok(())
}

#[test]
fn every_corpus_document_keeps_its_value_comments_and_is_idempotent() {
	let mut failures = Vec::new();

	for category in
		fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/conformance/valid"))
			.expect("the corpus exists")
	{
		for entry in fs::read_dir(category.expect("a category").path()).expect("a category") {
			let path = entry.expect("a case").path();

			// The canonical and formatted forms are companions of a case, not cases.
			if let Some(text) = path
				.to_str()
				.filter(|path| {
					path.ends_with(".soml")
						&& !path.ends_with(".canonical.soml")
						&& !path.ends_with(".formatted.soml")
				})
				.and_then(|_| fs::read_to_string(&path).ok())
				&& let Err(failure) = check_properties(&path.display().to_string(), &text)
			{
				failures.push(failure);
			}
		}
	}

	assert!(
		failures.is_empty(),
		"{} failures:\n{}",
		failures.len(),
		failures.join("\n\n")
	);
}

#[test]
fn lays_out_a_document_and_keeps_its_comments_and_spelling() {
	let text = "\n\n# Head\n\nname:   'api'    # name\npool: {min: 2, max: 0x10,   /* inline */ extra: [1, /* c */ 2, 3 /* before comma */, 4\n]}\nempty: {}\nnoted: { # opening\n}\na: # after colon\n  1\n\n\n\n'tail.key': true  # end\n# Trailing comment\n";

	assert_eq!(
		format(text),
		"# Head\n\nname: 'api' # name\npool: {\n\tmin: 2\n\tmax: 0x10\n\t/* inline */ extra: [\n\t\t1\n\t\t/* c */ 2\n\t\t3 /* before comma */\n\t\t4\n\t]\n}\nempty: {}\nnoted: { # opening\n}\na: # after colon\n\t1\n\n'tail.key': true # end\n# Trailing comment\n"
	);
}

#[test]
fn keeps_a_braced_top_level_and_a_top_level_array() {
	assert_eq!(
		format("# Before\n{\na: 1, b: {}} # After\n# Last\n"),
		"# Before\n{\n\ta: 1\n\tb: {}\n} # After\n# Last\n"
	);
	assert_eq!(
		format("# Before\n{a: 1, b: {}} # After\n# Last\n"),
		"# Before\n{a: 1, b: {}} # After\n# Last\n"
	);
	assert_eq!(
		format("[1,\n[2, 3], {a: null}]"),
		"[\n\t1\n\t[2, 3]\n\t{a: null}\n]\n"
	);
	assert_eq!(format("[1, [2, 3], {a: null}]"), "[1, [2, 3], {a: null}]\n");
	assert_eq!(format("[]"), "[]\n");
	assert_eq!(format("[ # nothing yet\n]"), "[ # nothing yet\n]\n");
}

#[test]
fn a_container_on_one_line_stays_on_one_line() {
	let cases = [
		(
			"a: {b: 1,  c: [1,2,], d: {}}",
			"a: {b: 1, c: [1, 2], d: {}}\n",
		),
		("a: {x:   1 , y: [ 1 ]}", "a: {x: 1, y: [1]}\n"),
		// A line break inside a container splits it, and not the containers on one line in it.
		("m: [\n[1,0], [0, 1]\n]", "m: [\n\t[1, 0]\n\t[0, 1]\n]\n"),
		("[1, [2,\n3]]", "[\n\t1\n\t[\n\t\t2\n\t\t3\n\t]\n]\n"),
		// Also a line break inside a block comment or a block string.
		("a: [1, /* x\ny */ 2]", "a: [\n\t1\n\t/* x\ny */ 2\n]\n"),
		(
			"a: [{b: '''\n x\n '''}, 1]",
			"a: [\n\t{\n\t\tb:\n\t\t\t'''\n\t\t\tx\n\t\t\t'''\n\t}\n\t1\n]\n",
		),
		// Block comments stay among the items and commas, and a comment before the trailing comma stays when the comma goes.
		(
			"[1 /* a */, /* b */ 2, /* c */]",
			"[1 /* a */, /* b */ 2 /* c */]\n",
		),
		("a: [1 /* x */, ]", "a: [1 /* x */]\n"),
		// A container that holds only comments is not empty.
		("a: [[ /* x */ ], 1]", "a: [[/* x */], 1]\n"),
		("[ /* a */ /* b */ ]", "[/* a */ /* b */]\n"),
	];

	for (text, expected) in cases {
		assert_eq!(format(text), expected, "{text:?}");
		assert_eq!(check_properties(text, text), Ok(()));
	}
}

#[test]
fn a_container_on_one_line_after_a_change_stays_on_one_line() {
	let mut document: Document = "a: [1]".parse().expect("valid");
	let array = document
		.get_mut(["a"])
		.and_then(soml::tree::Node::as_array_mut)
		.expect("an array");
	array
		.items_mut()
		.push(soml::tree::Item::new(2).expect("an item"));
	assert_eq!(document.to_string(), "a: [1,2]");
	assert_eq!(document.format(), "a: [1, 2]\n");

	// A line comment in changed decor ends its line, so the container spans lines.
	let mut document: Document = "a: [1, 2]".parse().expect("valid");
	let array = document
		.get_mut(["a"])
		.and_then(soml::tree::Node::as_array_mut)
		.expect("an array");
	*array.closing_decor_mut() = " # c".to_owned();
	assert_eq!(document.format(), "a: [\n\t1\n\t2 # c\n]\n");
	assert_eq!(document.format(), format(&document.to_string()));
}

#[test]
fn removes_blank_lines_inside_brackets_and_at_the_ends() {
	assert_eq!(
		format("\n\na: [\n\n\t1,\n\n\n\t2,\n\n]\n\n\n"),
		"a: [\n\t1\n\n\t2\n]\n"
	);
}

#[test]
fn indents_with_tabs_whatever_the_document_used() {
	assert_eq!(
		format("a: {\n    b: {\n        c: 1\n    }\n}"),
		"a: {\n\tb: {\n\t\tc: 1\n\t}\n}\n"
	);
}

#[test]
fn a_block_string_begins_on_the_line_after_its_key_and_lines_up_with_its_delimiters() {
	let cases = [
		(
			"feedbackNote: \'\'\'\n\tFoo\n\n\tBar\n\t\'\'\'\n",
			"feedbackNote:\n\t\'\'\'\n\tFoo\n\n\tBar\n\t\'\'\'\n",
		),
		// An array item keeps its own indentation.
		(
			"notes: [\n\t\'\'\'\n\t\tFoo\n\t\t\'\'\'\n]\n",
			"notes: [\n\t\'\'\'\n\tFoo\n\t\'\'\'\n]\n",
		),
		// A comment on the key's line stays there.
		(
			"a: /* c */ \'\'\'\n x\n \'\'\' # d\n",
			"a: /* c */\n\t\'\'\'\n\tx\n\t\'\'\' # d\n",
		),
	];

	for (text, expected) in cases {
		assert_eq!(format(text), expected, "{text:?}");
		assert_eq!(format(expected), expected, "{expected:?}");
	}
}

#[test]
fn reindents_a_block_string_and_keeps_its_value() {
	let text = "a: {\n    text: '''\n        first\n          indented\n\n        last\n        ''', b: 1}";
	let formatted = format(text);
	assert_eq!(
		formatted,
		"a: {\n\ttext:\n\t\t'''\n\t\tfirst\n\t\t  indented\n\n\t\tlast\n\t\t'''\n\tb: 1\n}\n"
	);
	assert_eq!(
		formatted.parse::<Value>().expect("valid"),
		text.parse::<Value>().expect("valid")
	);

	// A longer delimiter, escapes, a line of only spaces, which stays as written, as in the JavaScript reference, and a closing delimiter at column 0.
	let text = "a: \"\"\"\"\n\\t\"\"\" quoted\n   \nend\n\"\"\"\"";
	let formatted = format(text);
	assert_eq!(
		formatted,
		"a:\n\t\"\"\"\"\n\t\\t\"\"\" quoted\n   \n\tend\n\t\"\"\"\"\n"
	);
	assert_eq!(
		formatted.parse::<Value>().expect("valid"),
		text.parse::<Value>().expect("valid")
	);
}

#[test]
fn keeps_the_lines_of_a_block_comment_and_their_indentation() {
	assert_eq!(
		format("a: [\n  /* one\n     two */\n  1]"),
		"a: [\n\t/* one\n     two */\n\t1\n]\n"
	);
}

#[test]
fn removes_trailing_whitespace_and_blank_line_runs_inside_a_block_comment() {
	let cases = [
		(
			"a: [\n  /* one   \n\n\n     two */\n  1]",
			"a: [\n\t/* one\n\n     two */\n\t1\n]\n",
		),
		("/* a \t\n\n\n\nb */\na: 1\n", "/* a\n\nb */\na: 1\n"),
		("a: 1 /* x  \n  \n y */\n", "a: 1 /* x\n\n y */\n"),
	];

	for (text, expected) in cases {
		assert_eq!(format(text), expected, "{text:?}");
		assert_eq!(check_properties(text, text), Ok(()));
	}
}

#[test]
fn a_comma_at_the_start_of_a_line_is_an_error() {
	for text in [
		"[1\n,\n2]",
		"{a: 1\n, b: 2}",
		"[1 # c\n, 2]",
		"[\n\t1\n\t,\n]",
	] {
		assert!(text.parse::<Document>().is_err(), "{text:?}");
	}
}

#[test]
fn formats_an_object_left_empty_by_a_change() {
	let mut document: Document = "a: 1 # Note.\n".parse().expect("valid");
	assert!(document.remove(["a"]).expect("removed"));
	assert_eq!(document.format(), "{} # Note.\n");
	assert_eq!(
		soml::format(&document.format()).expect("valid"),
		"{} # Note.\n"
	);
}

#[test]
fn formats_after_changes() {
	let mut document: Document = "server: {host: 'a'}".parse().expect("valid");
	document.set(["server", "port"], 8080).expect("set");
	assert_eq!(document.format(), "server: {host: 'a', port: 8080}\n");
}

#[test]
fn formats_changed_decor_that_ends_in_a_line_comment() {
	let mut document: Document = "a: 1\nb: 2\n".parse().expect("valid");
	let root = document.root_mut().as_object_mut().expect("an object");
	root.members_mut()[1].decor_mut().leading = "# c".to_owned();
	assert_eq!(document.format(), "a: 1\n# c\nb: 2\n");

	let mut document: Document = "[1, 2]".parse().expect("valid");
	let root = document.root_mut().as_array_mut().expect("an array");
	root.items_mut()[1].decor_mut().leading = " # c".to_owned();
	assert_eq!(document.format(), "[\n\t1\n\t# c\n\t2\n]\n");
}

#[test]
fn changed_trailing_decor_that_ends_in_a_line_comment_keeps_the_comment_after_it_in_a_container() {
	// The printer ends the line comment, so formatting the document and formatting its text agree, and no comment swallows the next one.
	let mut document: Document = "{a: 1}".parse().expect("valid");
	let root = document.root_mut().as_object_mut().expect("an object");
	*root.closing_decor_mut() = "/* e */".to_owned();
	root.members_mut()[0].decor_mut().trailing = " # c".to_owned();
	assert_eq!(document.to_string(), "{a: 1 # c\n/* e */}");
	assert_eq!(document.format(), "{\n\ta: 1 # c\n\t/* e */\n}\n");
	assert_eq!(document.format(), format(&document.to_string()));

	let mut document: Document = "[1, 2]".parse().expect("valid");
	let root = document.root_mut().as_array_mut().expect("an array");
	root.items_mut()[0].decor_mut().trailing = " # c".to_owned();
	root.items_mut()[1].decor_mut().leading = "/* a\nb */ ".to_owned();
	assert_eq!(document.format(), "[\n\t1 # c\n\t/* a\nb */ 2\n]\n");
	assert_eq!(document.format(), format(&document.to_string()));

	// The comment stays at the end of its entry's line when nothing is before the next entry.
	let mut document: Document = "[1, 2]".parse().expect("valid");
	let root = document.root_mut().as_array_mut().expect("an array");
	root.items_mut()[0].decor_mut().trailing = " # c".to_owned();
	root.items_mut()[1].decor_mut().leading = String::new();
	assert_eq!(document.to_string(), "[1, # c\n2]");
	assert_eq!(document.format(), "[\n\t1 # c\n\t2\n]\n");
	assert_eq!(document.format(), format(&document.to_string()));

	// A line feed after the line comment is already there, so none is added.
	let mut document: Document = "[1, 2]".parse().expect("valid");
	let root = document.root_mut().as_array_mut().expect("an array");
	root.items_mut()[0].decor_mut().trailing = " # c".to_owned();
	root.items_mut()[1].decor_mut().leading = "\n\n# d\n".to_owned();
	assert_eq!(document.format(), "[\n\t1 # c\n\n\t# d\n\t2\n]\n");
	assert_eq!(document.format(), format(&document.to_string()));
}

#[test]
fn rejects_a_document_that_is_not_valid() {
	assert!(soml::format("a: 1\na: 2").is_err());
}

#[test]
fn a_line_comment_after_a_block_comment_on_one_line_still_ends_the_line() {
	let text = "'':\n\t/* two */ # e */,\n 1";
	assert_eq!(format(text), "'':\n\t/* two */ # e */,\n\t1\n");
	assert!(check_properties("regression", text).is_ok());
}

#[test]
fn a_comment_at_the_end_without_a_line_feed_stays_on_its_line() {
	assert_eq!(format("a: {\nb: 1}  # Note."), "a: {\n\tb: 1\n} # Note.\n");
	assert_eq!(format("a: {b: 1}  # Note."), "a: {b: 1} # Note.\n");
	assert_eq!(format("[1 /* last */\n]"), "[\n\t1 /* last */\n]\n");
	assert_eq!(format("[1 /* last */]"), "[1 /* last */]\n");
	assert_eq!(format("{\na: 1} /* after */"), "{\n\ta: 1\n} /* after */\n");
	assert_eq!(format("{a: 1} /* after */"), "{a: 1} /* after */\n");
	assert_eq!(format("a: { /* empty */\n}"), "a: { /* empty */\n}\n");
	assert_eq!(format("a: { /* empty */ }"), "a: {/* empty */}\n");
}

#[test]
fn a_value_after_a_line_comment_and_a_block_comment_is_indented_by_its_line() {
	assert_eq!(
		format("k: # a\n /* b */ [1\n]\n"),
		"k: # a\n\t/* b */ [\n\t\t1\n\t]\n"
	);
	assert_eq!(
		format("a: {\nk: # a\n /* b */ {x: 1\n}}"),
		"a: {\n\tk: # a\n\t\t/* b */ {\n\t\t\tx: 1\n\t\t}\n}\n"
	);
	assert_eq!(
		format("k: # a\n /* b */ \'\'\'\n x\n \'\'\'"),
		"k: # a\n\t/* b */\n\t\'\'\'\n\tx\n\t\'\'\'\n"
	);
}

#[test]
fn keeps_a_blank_line_that_does_not_touch_a_bracket_or_an_edge() {
	assert_eq!(format("# c\n\n[1]\n"), "# c\n\n[1]\n");
	assert_eq!(format("[1]\n\n# c\n"), "[1]\n\n# c\n");
	assert_eq!(format("a: 1\n\n# c\n"), "a: 1\n\n# c\n");
	assert_eq!(format("[\n1,\n\n# c\n]\n"), "[\n\t1\n\n\t# c\n]\n");
	assert_eq!(format("/* x */ {a: 1}"), "/* x */ {a: 1}\n");
}

#[test]
fn keeps_unicode_whitespace_at_the_end_of_a_line_comment() {
	assert_eq!(format("a: 1 # a\u{a0}  \n"), "a: 1 # a\u{a0}\n");
}

#[test]
fn a_comment_between_a_key_and_its_value_keeps_its_line() {
	let cases = [
		// A comment on its own line stays on its own line, and the value stays below it.
		("a:\n# c\n1\n", "a:\n\t# c\n\t1\n"),
		(
			"a:\n\t# c\n\t# d\n\t1\nb: 2\n",
			"a:\n\t# c\n\t# d\n\t1\nb: 2\n",
		),
		("a:\n\n# c\n\n1\n", "a:\n\t# c\n\t1\n"),
		("x: {a:\n# c\n1}\n", "x: {\n\ta:\n\t\t# c\n\t\t1\n}\n"),
		("a:\n# c\n[1]\n", "a:\n\t# c\n\t[1]\n"),
		("a:\n# c\n'''\n x\n '''", "a:\n\t# c\n\t'''\n\tx\n\t'''\n"),
		// A comment at the end of the key's line stays at the end of that line.
		("a: # c\n1\n", "a: # c\n\t1\n"),
		("a: # c\n# d\n1\n", "a: # c\n\t# d\n\t1\n"),
		("a: /* c */\n1\n", "a: /* c */\n\t1\n"),
		("a: /* c */\n/* d */\n1\n", "a: /* c */\n\t/* d */\n\t1\n"),
		// A comment before the value stays before it, on its line.
		("a:\n/* c */ 1\n", "a:\n\t/* c */ 1\n"),
		("a:\n# c\n/* d */ 1\n", "a:\n\t# c\n\t/* d */ 1\n"),
		// With no comment, or no line break, the value is on the key's line.
		("a:\n1\n", "a: 1\n"),
		("a: /* c */ 1\n", "a: /* c */ 1\n"),
		("a: /* x\ny */ 1", "a: /* x\ny */ 1\n"),
	];

	for (text, expected) in cases {
		assert_eq!(format(text), expected, "{text:?}");
		assert_eq!(check_properties(text, text), Ok(()));
	}
}

#[test]
fn a_comment_before_a_comma_stays_after_the_value() {
	let cases = [
		("[1 /* c */, 2\n]\n", "[\n\t1 /* c */\n\t2\n]\n"),
		("{a: 1 /* c */, b: 2\n}\n", "{\n\ta: 1 /* c */\n\tb: 2\n}\n"),
		(
			"[1 /* c */ /* d */, 2\n]\n",
			"[\n\t1 /* c */ /* d */\n\t2\n]\n",
		),
		// A block comment that spans lines may end on the comma's line.
		("[1 /* a\n */, 2]", "[\n\t1 /* a\n */\n\t2\n]\n"),
		// A comment after the comma stays before the next item.
		(
			"[1 /* a */, /* b */ 2\n]",
			"[\n\t1 /* a */\n\t/* b */ 2\n]\n",
		),
		// On one line, a comment before the comma stays before it.
		("[1 /* c */, 2]\n", "[1 /* c */, 2]\n"),
	];

	for (text, expected) in cases {
		assert_eq!(format(text), expected, "{text:?}");
		assert_eq!(check_properties(text, text), Ok(()));
	}
}

#[test]
fn a_comment_before_a_block_comment_that_ends_on_a_later_line_stays_on_its_line() {
	// Spec formatting rule 4: a comment after a comma or an opening bracket stays before the next item only when it is on that item's line. A block comment that spans lines is part of the line it ends on, so the comments before it are on an earlier line. The expected text is what the JavaScript reference writes.
	let cases = [
		(
			"a: [1, /* a */ /* b\n */ 2]\n",
			"a: [\n\t1 /* a */\n\t/* b\n */ 2\n]\n",
		),
		(
			"a: [/* a */ /* b\n */ 2]\n",
			"a: [ /* a */\n\t/* b\n */ 2\n]\n",
		),
		(
			"a: [1, /* a\n */ /* b\n */ 2]\n",
			"a: [\n\t1 /* a\n */\n\t/* b\n */ 2\n]\n",
		),
		(
			"a: {x: 1, /* a */ /* b\n */ y: 2}\n",
			"a: {\n\tx: 1 /* a */\n\t/* b\n */ y: 2\n}\n",
		),
		(
			"a: [1, /* a */ /* b\n */ /* c */ 2]\n",
			"a: [\n\t1 /* a */\n\t/* b\n */ /* c */ 2\n]\n",
		),
		(
			"a: [1, /* a */ /* b\n */ /* c\n */ 2]\n",
			"a: [\n\t1 /* a */ /* b\n */\n\t/* c\n */ 2\n]\n",
		),
		// A comment after the opening bracket that follows a block comment ending on the item's line stays before the item.
		(
			"a: [1,\n\t/* a */ /* b\n */ 2]\n",
			"a: [\n\t1\n\t/* a */ /* b\n */ 2\n]\n",
		),
		// Before a closing bracket, a line feed, or a value on the key's line, every comment stays on the line it is on.
		(
			"a: [1 /* a */ /* b\n */]\n",
			"a: [\n\t1 /* a */ /* b\n */\n]\n",
		),
		(
			"a: [1, /* a */ /* b\n */\n2]\n",
			"a: [\n\t1 /* a */ /* b\n */\n\t2\n]\n",
		),
		("a: /* a */ /* b\n */ 1\n", "a: /* a */ /* b\n */ 1\n"),
		(
			"a: /* a */ /* b\n */ '''\n x\n '''\n",
			"a: /* a */ /* b\n */\n\t'''\n\tx\n\t'''\n",
		),
		("a: {} /* a */ /* b\n */\n", "a: {} /* a */ /* b\n */\n"),
		("[1] /* a */ /* b\n */", "[1] /* a */ /* b\n */\n"),
	];

	for (text, expected) in cases {
		assert_eq!(format(text), expected, "{text:?}");
		assert_eq!(check_properties(text, text), Ok(()));
	}
}
