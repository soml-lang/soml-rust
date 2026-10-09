/*!
`Error`: its message, its position, and where a deserialization error points.
*/

#![allow(clippy::tabs_in_doc_comments)]

use serde::Deserialize;
use soml::{LineColumn, Value};

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct Server {
	host: String,
	port: u16,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct Config {
	server: Server,
}

#[test]
fn an_error_has_a_message_and_a_position() {
	let error = "a: 1\nb: 2\nb: 3"
		.parse::<Value>()
		.expect_err("a duplicate key");

	assert_eq!(error.message(), "Duplicate key b");
	assert_eq!(error.position(), Some(LineColumn { line: 3, column: 1 }));
	assert_eq!(error.offset(), Some(10));
	assert_eq!(error.to_string(), "Duplicate key b at line 3, column 1");
}

#[test]
fn a_line_and_column_displays_as_in_an_error() {
	let error = soml::from_str::<Value>("a: 1\na: 2").unwrap_err();
	let position = error.position().expect("a position");
	assert_eq!(position.to_string(), "line 2, column 1");
	assert_eq!(error.to_string(), format!("Duplicate key a at {position}"));
}

#[test]
fn debug_shows_the_message_and_the_position() {
	let error = "a: 1\na: 2".parse::<Value>().expect_err("a duplicate key");
	assert_eq!(
		format!("{error:?}"),
		"Error { kind: Syntax, message: \"Duplicate key a\", line: 2, column: 1 }"
	);

	let error = soml::to_string(&Value::Int(1)).expect_err("a scalar is not a document");
	assert_eq!(
		format!("{error:?}"),
		"Error { kind: Write, message: \"A document must be an object or an array, not an int\" }"
	);
}

#[test]
fn an_error_without_a_position_displays_only_its_message() {
	let error = soml::to_string(&Value::Float(f64::NAN)).expect_err("NaN");
	assert_eq!(error.to_string(), "NaN is not a SOML value");
	assert_eq!(error.position(), None);
	assert_eq!(error.offset(), None);
}

#[test]
fn an_error_is_a_small_sendable_std_error() {
	fn assert_error<T: std::error::Error + Send + Sync + 'static>() {}
	assert_error::<soml::Error>();
	assert_eq!(
		std::mem::size_of::<soml::Error>(),
		std::mem::size_of::<usize>()
	);

	let boxed: Box<dyn std::error::Error + Send + Sync> =
		Box::new(soml::from_str::<Value>("").expect_err("an empty document"));
	assert_eq!(
		boxed.to_string(),
		"A document must contain an object or an array, but this one is empty at line 1, column 1"
	);
}

#[test]
fn a_type_mismatch_points_at_the_value() {
	let error =
		soml::from_str::<Server>("host: 'x'\nport: 'eighty'").expect_err("a string is not a u16");
	assert_eq!(
		error.message(),
		"invalid type: string \"eighty\", expected u16"
	);
	assert_eq!(error.position(), Some(LineColumn { line: 2, column: 7 }));
	assert_eq!(error.offset(), Some(16));
}

#[test]
fn an_invalid_value_points_at_the_value() {
	assert_eq!(
		soml::from_str::<Server>("host: 'x'\nport: 70000")
			.expect_err("too large for a u16")
			.to_string(),
		"invalid value: integer `70000`, expected u16 at line 2, column 7"
	);
}

#[test]
fn a_nested_type_mismatch_points_at_the_nested_value() {
	assert_eq!(
		soml::from_str::<Config>("server: {\n\thost: 1,\n\tport: 80,\n}")
			.expect_err("an int is not a string")
			.to_string(),
		"invalid type: integer `1`, expected a string at line 2, column 8"
	);
}

#[test]
fn a_type_mismatch_in_an_array_points_at_the_item() {
	assert_eq!(
		soml::from_str::<Vec<u16>>("[1,\n  2,\n  'three']")
			.expect_err("a string is not a u16")
			.to_string(),
		"invalid type: string \"three\", expected u16 at line 3, column 3"
	);
}

#[test]
fn a_top_level_type_mismatch_points_at_the_document() {
	assert_eq!(
		soml::from_str::<Vec<i32>>("# a comment\na: 1")
			.expect_err("an object is not a sequence")
			.to_string(),
		"invalid type: map, expected a sequence at line 2, column 1"
	);
	assert_eq!(
		soml::from_str::<Server>("  [1]")
			.expect_err("an array is not a struct")
			.to_string(),
		"invalid type: sequence, expected struct Server at line 1, column 3"
	);
}

#[test]
fn an_unknown_field_points_at_its_key() {
	#[derive(Deserialize, Debug)]
	#[serde(deny_unknown_fields)]
	#[allow(dead_code)]
	struct Strict {
		a: i32,
	}

	let error = soml::from_str::<Strict>("a: 1\n  extra: 2").expect_err("an unknown field");
	assert_eq!(error.message(), "unknown field `extra`, expected `a`");
	assert_eq!(error.position(), Some(LineColumn { line: 2, column: 3 }));
	assert_eq!(error.offset(), Some(7));
}

#[test]
fn an_unknown_field_inside_braces_points_at_its_key() {
	#[derive(Deserialize, Debug)]
	#[serde(deny_unknown_fields)]
	#[allow(dead_code)]
	struct Strict {
		a: i32,
	}

	#[derive(Deserialize, Debug)]
	#[allow(dead_code)]
	struct Outer {
		inner: Strict,
	}

	assert_eq!(
		soml::from_str::<Outer>("inner: {a: 1, 'b c': 2}")
			.expect_err("an unknown field")
			.to_string(),
		"unknown field `b c`, expected `a` at line 1, column 15"
	);
}

#[test]
fn a_missing_field_points_at_the_object() {
	assert_eq!(
		soml::from_str::<Config>("server: {\n\thost: 'x',\n}")
			.expect_err("port is missing")
			.to_string(),
		"missing field `port` at line 1, column 9"
	);
}

#[test]
fn a_missing_field_in_a_braced_document_points_at_its_brace() {
	assert_eq!(
		soml::from_str::<Server>("\n{host: 'x'}")
			.expect_err("port is missing")
			.to_string(),
		"missing field `port` at line 2, column 1"
	);
}

#[test]
fn a_missing_field_in_a_brace_less_document_points_at_its_first_entry() {
	// A brace-less object has no brace, so it starts at its first key, after any comments.
	assert_eq!(
		soml::from_str::<Server>("# The server.\nhost: 'x'")
			.expect_err("port is missing")
			.to_string(),
		"missing field `port` at line 2, column 1"
	);
}

#[test]
fn a_missing_top_level_field_in_an_empty_object() {
	assert_eq!(
		soml::from_str::<Server>("{}")
			.expect_err("both fields are missing")
			.to_string(),
		"missing field `host` at line 1, column 1"
	);
}

#[test]
fn a_column_counts_unicode_scalar_values_in_a_type_error() {
	// The emoji is one column and four bytes.
	let error =
		soml::from_str::<Server>("{host: '😀', port: 'x'}").expect_err("a string is not a u16");
	assert_eq!(
		error.position(),
		Some(LineColumn {
			line: 1,
			column: 19
		})
	);
	assert_eq!(error.offset(), Some(21));
}

#[test]
fn a_syntax_error_comes_before_a_type_error() {
	assert_eq!(
		soml::from_str::<Server>("host: 1\nport: 1\nport: 2")
			.expect_err("a duplicate key")
			.to_string(),
		"Duplicate key port at line 3, column 1"
	);
}

#[test]
fn a_dot_after_a_key_is_an_error_at_the_dot() {
	const SPECIFIC: &str = "A bare key cannot contain “.”. Quote it, as in";
	const GENERAL: &str = "A key cannot contain “.” unless it is quoted. Quote the whole key, or use braces to nest, as in a: {b: …}";
	const QUOTE_THE_KEY: &str =
		"A key that contains characters other than letters, digits, “_”, and “-” must be quoted";

	// The messages and positions are those of the JavaScript reference.
	for (text, message, column) in [
		(
			"a.b: 1",
			format!("{SPECIFIC} 'a.b', or use braces to nest, as in a: {{b: …}}"),
			2,
		),
		(
			"example.com: 1",
			format!("{SPECIFIC} 'example.com', or use braces to nest, as in example: {{com: …}}"),
			8,
		),
		(
			"a.b.c: 1",
			format!("{SPECIFIC} 'a.b.c', or use braces to nest, as in a: {{b: {{c: …}}}}"),
			2,
		),
		(
			"3.14: 'x'",
			format!("{SPECIFIC} '3.14', or use braces to nest, as in 3: {{14: …}}"),
			2,
		),
		(
			"1.5s: 1",
			format!("{SPECIFIC} '1.5s', or use braces to nest, as in 1: {{5s: …}}"),
			2,
		),
		(
			"{a.b: 1}",
			format!("{SPECIFIC} 'a.b', or use braces to nest, as in a: {{b: …}}"),
			3,
		),
		(
			"[{a.b: 1}]",
			format!("{SPECIFIC} 'a.b', or use braces to nest, as in a: {{b: …}}"),
			4,
		),
		(
			"x: {a.b: 1}",
			format!("{SPECIFIC} 'a.b', or use braces to nest, as in a: {{b: …}}"),
			6,
		),
		(
			"abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz.b: 1",
			format!(
				"{SPECIFIC} 'abcdefghijklmnopqrstuvwxyzabcdefghijklmn…', or use braces to nest, as in abcdefghijklmnopqrstuvwxyzabcdefghijklmn…"
			),
			53,
		),
		// The suggestions are only given when both are valid: a bare key with a word on both sides of every dot, and a `:` right after it.
		("'a'.b: 1", GENERAL.to_owned(), 4),
		("\"a\".b: 1", GENERAL.to_owned(), 4),
		("a.'b': 1", GENERAL.to_owned(), 2),
		("a.: 1", GENERAL.to_owned(), 2),
		("a..b: 1", GENERAL.to_owned(), 2),
		("a.b", GENERAL.to_owned(), 2),
		("a.b c: 1", GENERAL.to_owned(), 2),
		("a.b :1", GENERAL.to_owned(), 2),
		("a.$b: 1", GENERAL.to_owned(), 2),
		(
			".a: 1",
			format!("Expected a key, but found “.”. {QUOTE_THE_KEY}, as in '.a'"),
			1,
		),
		(
			".env: 1",
			format!("Expected a key, but found “.”. {QUOTE_THE_KEY}, as in '.env'"),
			1,
		),
		(
			"files: {.eslintrc.json: 1}",
			format!("Expected a key, but found “.”. {QUOTE_THE_KEY}, as in '.eslintrc.json'"),
			9,
		),
	] {
		let error = text.parse::<Value>().expect_err(text);
		assert_eq!(error.message(), message, "{text:?}");
		assert_eq!(
			error.position(),
			Some(LineColumn { line: 1, column }),
			"{text:?}"
		);
	}
}

#[test]
fn a_quoted_key_with_a_dot_is_one_key() {
	let value: Value = "'a.b': 1\n\"c.d\": {'e.f': 2}".parse().expect("valid");
	assert_eq!(value, soml::soml!({"a.b": 1, "c.d": {"e.f": 2}}));
}

#[test]
fn a_map_key_error_points_at_the_key() {
	assert_eq!(
		soml::from_str::<std::collections::BTreeMap<u16, i32>>("1: 1\n  x: 2")
			.expect_err("x is not an int")
			.to_string(),
		"Expected the key “x” to be an integer in decimal, like 404 at line 2, column 3"
	);
}

#[test]
fn a_map_key_error_in_a_nested_object_points_at_the_key() {
	assert_eq!(
		soml::from_str::<std::collections::BTreeMap<String, std::collections::BTreeMap<u16, i32>>>(
			"ports: {http: 1}"
		)
		.expect_err("http is not an int")
		.to_string(),
		"Expected the key “http” to be an integer in decimal, like 404 at line 1, column 9"
	);
}

#[test]
fn a_data_error_hides_the_control_characters_of_a_key() {
	let error = soml::from_str::<std::collections::BTreeMap<u16, i32>>("\"\\u{1b}]0;x\\u{7}\": 1")
		.expect_err("the key is not an int");
	assert!(!error.message().chars().any(char::is_control), "{error:?}");

	assert_eq!(
		soml::Error::with_position("One\nTwo", "a: 1", 0).message(),
		"One\nTwo"
	);
}

#[test]
fn an_error_from_from_slice_counts_columns_the_same_way() {
	let error = soml::from_slice::<Server>("{host: '😀', port: 'x'}".as_bytes())
		.expect_err("a string is not a u16");
	assert_eq!(
		error.position(),
		Some(LineColumn {
			line: 1,
			column: 19
		})
	);
	assert_eq!(error.offset(), Some(21));
}

#[test]
fn a_type_error_names_an_instant_or_a_duration() {
	#[allow(dead_code)]
	#[derive(Debug, serde::Deserialize)]
	struct Config {
		count: u32,
	}

	assert_eq!(
		soml::from_str::<Config>("count: 5s")
			.unwrap_err()
			.to_string(),
		"invalid type: a duration, expected u32 at line 1, column 8"
	);
	assert_eq!(
		soml::from_str::<Config>("count: 2026-09-19T14:00:00Z")
			.unwrap_err()
			.to_string(),
		"invalid type: an instant, expected u32 at line 1, column 8"
	);
	assert_eq!(
		soml::from_str::<Vec<f64>>("[5s]").unwrap_err().to_string(),
		"invalid type: a duration, expected f64 at line 1, column 2"
	);
	assert_eq!(
		soml::from_str::<Vec<f32>>("[2026-09-19T14:00:00Z]")
			.unwrap_err()
			.to_string(),
		"invalid type: an instant, expected f32 at line 1, column 2"
	);
}

#[test]
fn a_surrogate_escape_gets_a_hint_that_works() {
	let message = |text: &str| {
		soml::from_str::<Value>(text)
			.expect_err("an invalid escape")
			.message()
			.to_owned()
	};

	// A JSON surrogate pair is one escape for the character it encodes.
	assert_eq!(
		message(r#"a: "\uD83D\uDE00""#),
		r"The four-digit \uD83D\uDE00 form is not an escape. Write \u{1f600}"
	);
	assert_eq!(
		message(r#"a: "\ud83d\ude00""#),
		r"The four-digit \ud83d\ude00 form is not an escape. Write \u{1f600}"
	);
	assert_eq!(
		message(r#"a: "\uDBFF\uDFFF""#),
		r"The four-digit \uDBFF\uDFFF form is not an escape. Write \u{10ffff}"
	);

	// A lone surrogate has no escape to suggest. A low surrogate first, or a high surrogate before something that is not a low one, is not a pair.
	for (text, form) in [
		(r#"a: "\uD83D""#, r"\uD83D"),
		(r#"a: "\uDE00""#, r"\uDE00"),
		(r#"a: "\uDE00\uD83D""#, r"\uDE00"),
		(r#"a: "\uD83D\uD83D""#, r"\uD83D"),
		(r#"a: "\uD83D\u0041""#, r"\uD83D"),
		(r#"a: "\uD83D\u{de00}""#, r"\uD83D"),
		(r#"a: "\uD83Dx""#, r"\uD83D"),
	] {
		assert_eq!(
			message(text),
			format!(
				r"The four-digit {form} form is not an escape, and a lone surrogate is not a Unicode scalar value. Write the character it is half of as one \u{{…}} escape"
			)
		);
	}

	assert_eq!(
		message(r#"a: "\u000d""#),
		r"The four-digit \u000d form is not an escape, and a carriage return (U+000D) cannot be represented"
	);

	// A braced escape with leading zeros is reported for its value first.
	assert_eq!(
		message(r#"a: "\u{000d}""#),
		r"A carriage return (U+000D) cannot be represented, so \u{000d} is not allowed"
	);
	assert_eq!(
		message(r#"a: "\u{00d800}""#),
		r"\u{00d800} is a surrogate, which is not a Unicode scalar value"
	);
	assert_eq!(
		message(r#"a: "\u{011000}""#),
		r"A Unicode escape may not have leading zeros; write \u{11000}"
	);
}

#[test]
fn every_suggested_escape_reads_as_the_character_json_reads() {
	for code in 0..=0xFFFF_u32 {
		let escape = format!("\\u{code:04X}");
		let error =
			soml::from_str::<Value>(&format!("a: \"{escape}\"")).expect_err("a JSON escape");
		let suggestion = error
			.message()
			.split_once("Write \\u{")
			.and_then(|(_, rest)| rest.split_once('}'))
			.map(|(hex, _)| hex);
		let is_representable = code != 0x0D && !(0xD800..=0xDFFF).contains(&code);
		assert_eq!(suggestion.is_some(), is_representable, "{escape}");

		if let Some(hex) = suggestion {
			let value =
				soml::from_str::<Value>(&format!("a: \"\\u{{{hex}}}\"")).expect("a valid escape");
			assert_eq!(
				value.get("a"),
				Some(&Value::String(
					char::from_u32(code).expect("a character").into()
				)),
				"{escape}"
			);
		}
	}
}

#[test]
fn a_code_frame_shows_up_to_two_lines_before_the_error() {
	let text = "a: 1\nb: 2\nc: 3\nd: x\ne: 5";
	let error = soml::from_str::<Value>(text).unwrap_err();
	assert_eq!(
		error.code_frame(text).expect("a frame"),
		"  2 | b: 2\n  3 | c: 3\n> 4 | d: x\n    |    ^"
	);

	let text = "a: x";
	let error = soml::from_str::<Value>(text).unwrap_err();
	assert_eq!(
		error.code_frame(text).expect("a frame"),
		"> 1 | a: x\n    |    ^"
	);
}

#[test]
fn a_code_frame_shows_blank_lines_before_the_error() {
	for (text, frame) in [
		("a: 1\n\n\nd: x", "  2 |\n  3 |\n> 4 | d: x\n    |    ^"),
		("\nd: x", "  1 |\n> 2 | d: x\n    |    ^"),
		("\n\nd: x", "  1 |\n  2 |\n> 3 | d: x\n    |    ^"),
	] {
		let error = soml::from_str::<Value>(text).unwrap_err();
		assert_eq!(error.code_frame(text).expect("a frame"), frame, "{text:?}");
	}
}

#[test]
fn a_code_frame_widens_its_gutter_for_large_line_numbers() {
	let text = format!(
		"{}z: x",
		(1..=9)
			.map(|index| format!("a{index}: 1\n"))
			.collect::<String>()
	);
	let error = soml::from_str::<Value>(&text).unwrap_err();
	assert_eq!(
		error.code_frame(&text).expect("a frame"),
		"   8 | a8: 1\n   9 | a9: 1\n> 10 | z: x\n     |    ^"
	);
}

#[test]
fn a_message_hides_control_characters_from_the_document() {
	// A bidirectional control or a C1 control could change how a terminal shows the rest of the message.
	let message = soml::from_str::<Value>("a: \"\\\u{202E}\"")
		.unwrap_err()
		.message()
		.to_owned();
	assert!(
		message.starts_with("Unknown escape “\\\u{FFFD}”."),
		"{message}"
	);

	let message = soml::from_str::<Value>("'\u{9b}2J': 1\n'\u{9b}2J': 2")
		.unwrap_err()
		.message()
		.to_owned();
	assert_eq!(message, "Duplicate key \"\u{FFFD}2J\"");
}

#[test]
fn a_long_bare_key_stays_bare_in_a_message() {
	let key = "k".repeat(41);
	let message = soml::from_str::<Value>(&format!("{key}: 1\n{key}: 2"))
		.unwrap_err()
		.message()
		.to_owned();
	assert_eq!(message, format!("Duplicate key {}…", "k".repeat(40)));
}

#[test]
fn a_code_frame_keeps_tabs_and_hides_control_characters() {
	let text = "{\n\ta: \u{202E}x\n}";
	let error = soml::from_str::<Value>(text).unwrap_err();
	assert_eq!(
		error.code_frame(text).expect("a frame"),
		"  1 | {\n> 2 | \ta: \u{FFFD}x\n    | \t   ^"
	);
}

#[test]
fn a_code_frame_clips_a_long_line_around_the_error() {
	let text = format!("a: [{}x]", "1, ".repeat(100));
	let error = soml::from_str::<Value>(&text).unwrap_err();
	let frame = error.code_frame(&text).expect("a frame");
	let error_line = frame.lines().next().expect("a line");
	assert!(error_line.starts_with("> 1 | …"), "{frame}");
	assert!(
		error_line.ends_with('…') || error_line.ends_with(']'),
		"{frame}"
	);
	assert_eq!(
		error_line.chars().count(),
		"> 1 | ".len() + 1 + 100 + usize::from(error_line.ends_with('…'))
	);

	// The caret is under the `x`.
	let caret = frame.lines().nth(1).expect("a caret line").chars().count() - 1;
	assert_eq!(error_line.chars().nth(caret), Some('x'), "{frame}");
}

#[test]
fn a_code_frame_needs_a_position_and_survives_other_text() {
	let error = soml::to_string(&Value::Int(1)).unwrap_err();
	assert_eq!(error.code_frame("a: 1"), None);

	let error = soml::from_str::<Value>("aaaaaaaaaa: x").unwrap_err();
	assert_eq!(error.code_frame("é"), Some("> 1 | é\n    |  ^".to_owned()));
	assert_eq!(error.code_frame(""), Some("> 1 |\n    | ^".to_owned()));
}

#[test]
fn every_error_has_a_kind() {
	assert_eq!(
		soml::from_str::<Value>("a: ").unwrap_err().kind(),
		soml::ErrorKind::Syntax
	);
	assert_eq!(
		soml::from_str::<std::collections::BTreeMap<String, u8>>("a: 300")
			.unwrap_err()
			.kind(),
		soml::ErrorKind::Data
	);
	assert_eq!(
		soml::to_string(&f64::NAN).unwrap_err().kind(),
		soml::ErrorKind::Write
	);
	assert_eq!(
		"1d".parse::<soml::Duration>().unwrap_err().kind(),
		soml::ErrorKind::Syntax
	);
	assert_eq!(
		std::time::Duration::try_from(soml::Duration::from_nanoseconds(-1))
			.unwrap_err()
			.kind(),
		soml::ErrorKind::Data
	);

	let mut document: soml::Document = "a: 1".parse().expect("valid");
	assert_eq!(
		document.set(["a", "b"], 1).unwrap_err().kind(),
		soml::ErrorKind::Data
	);
	assert_eq!(
		document.set(["x\ry"], 1).unwrap_err().kind(),
		soml::ErrorKind::Write
	);

	struct Failing;

	impl std::io::Read for Failing {
		fn read(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
			Err(std::io::Error::other("broken"))
		}
	}

	let error = soml::from_reader::<_>(Failing)
		.map(|_: Value| ())
		.unwrap_err();
	assert_eq!(error.kind(), soml::ErrorKind::Io);
	assert_eq!(
		std::error::Error::source(&error)
			.expect("a source")
			.to_string(),
		"broken"
	);
}

#[test]
fn a_failing_writer_gives_an_io_error() {
	struct Failing;

	impl std::io::Write for Failing {
		fn write(&mut self, _buffer: &[u8]) -> std::io::Result<usize> {
			Err(std::io::Error::other("full"))
		}

		fn flush(&mut self) -> std::io::Result<()> {
			Ok(())
		}
	}

	let error = soml::to_writer(Failing, &[1]).unwrap_err();
	assert_eq!(error.kind(), soml::ErrorKind::Io);
	assert_eq!(
		std::error::Error::source(&error)
			.expect("a source")
			.to_string(),
		"full"
	);
}

#[test]
fn an_error_of_your_own_is_a_data_error_with_its_offset_kept_inside_the_text_and_on_a_character() {
	let error = soml::Error::with_position("Too far", "ab", 99);
	assert_eq!(error.kind(), soml::ErrorKind::Data);
	assert_eq!(
		(error.position(), error.offset()),
		(Some(LineColumn { line: 1, column: 3 }), Some(2))
	);

	// An offset inside a character points at that character.
	let error = soml::Error::with_position("Here", "é", 1);
	assert_eq!(
		(error.position(), error.offset()),
		(Some(LineColumn { line: 1, column: 1 }), Some(0))
	);
}

#[test]
fn reader_and_writer_round_trip() {
	let value: Value = soml::from_reader("a: [1, 2]".as_bytes()).expect("valid");
	let mut output = Vec::new();
	soml::to_writer(&mut output, &value).expect("written");
	assert_eq!(output, b"a: [\n\t1\n\t2\n]\n");
}

#[test]
fn a_code_frame_clips_a_long_line_as_the_javascript_reference_does() {
	// The expected frames are the JavaScript reference's `codeFrame` for the same text.
	let caret_line = |column: usize| format!("\n    | {}^", " ".repeat(column));
	let cases = [
		// At the start of a long line, only its end is cut.
		(
			format!("a: x{}", "y".repeat(150)),
			format!("> 1 | a: x{}…{}", "y".repeat(96), caret_line(3)),
		),
		// At the end of a long line, only its start is cut.
		(
			format!("a: [{}z]", "1, ".repeat(50)),
			format!("> 1 | …{}z]{}", ", 1".repeat(32) + ", ", caret_line(99)),
		),
		// In the middle, both ends are cut, and the caret is in the middle of what is left.
		(
			format!("a: [{}z{}]", "1, ".repeat(30), ", 1".repeat(30)),
			format!(
				"> 1 | …{}z{},…{}",
				", 1".repeat(16) + ", ",
				", 1".repeat(16),
				caret_line(51)
			),
		),
		// A line of 100 characters is not cut, and one of 101 is.
		(
			format!("a: {}", "b".repeat(97)),
			format!("> 1 | a: {}{}", "b".repeat(97), caret_line(3)),
		),
		(
			format!("a: {}", "b".repeat(98)),
			format!("> 1 | a: {}…{}", "b".repeat(97), caret_line(3)),
		),
	];

	for (text, expected) in cases {
		let error = soml::from_str::<Value>(&text).unwrap_err();
		assert_eq!(
			error.code_frame(&text).expect("a frame"),
			expected,
			"{text:?}"
		);
	}
}

/**
Reasons for common mistakes, from the error tests of the JS reference. It gives the same reasons, with straight quotes and key paths in JSON syntax.
*/
#[test]
fn the_reasons_for_common_mistakes_match_the_reference() {
	let cases = [
		(
			"",
			"A document must contain an object or an array, but this one is empty",
		),
		(
			"5",
			"A bare value is not a document. A document is an object or an array, so write it as `key: value` or `[value]`",
		),
		(
			"2026-09-19T14:00:00Z",
			"A bare value is not a document. A document is an object or an array, so write it as `key: value` or `[value]`",
		),
		(
			"'hello'",
			"A bare value is not a document. A document is an object or an array, so write it as `key: value` or `[value]`",
		),
		(
			"a: 512 MiB",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 10s, and anything else, such as a size, as a string, as in '512 MiB'",
		),
		(
			"a: 10 seconds",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 10s, and anything else, such as a size, as a string, as in '10 seconds'",
		),
		(
			"a: 100 ms",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 100ms, and anything else, such as a size, as a string, as in '100 ms'",
		),
		(
			"a: 10 m",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 10m, and anything else, such as a size, as a string, as in '10 m'",
		),
		(
			"a: 1.5 h # note",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 1.5h, and anything else, such as a size, as a string, as in '1.5 h'",
		),
		(
			"a: [100 ms]",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 100ms, and anything else, such as a size, as a string, as in '100 ms'",
		),
		(
			"a: {b: 100 ms}",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 100ms, and anything else, such as a size, as a string, as in '100 ms'",
		),
		(
			"a: [1 true]",
			"Expected “,”, a line break, or “]” after an array item, but found “t”",
		),
		(
			"a: 5 and more",
			"Expected a line break before the next entry, but found “a”",
		),
		(
			"a: 'x' MiB",
			"Expected a line break before the next entry, but found “M”",
		),
		(
			"// note\na: 1",
			"Expected a key, but found “/”. A comment starts with “#”",
		),
		(
			"a: 1 // note",
			"Expected a line break before the next entry, but found “/”. A comment starts with “#”",
		),
		(
			"a: [1 // note\n]",
			"Expected “,”, a line break, or “]” after an array item, but found “/”. A comment starts with “#”",
		),
		(
			"a: [1, // note\n2]",
			"Expected a value, but found “/”. A comment starts with “#”",
		),
		(
			"a: 1 / 2",
			"Expected a line break before the next entry, but found “/”",
		),
		(
			"key: |\n  text",
			"Expected a value, but found “|”. Write a multiline string as a block string, between ''' lines",
		),
		("key: >-\n  text", "Expected a value, but found “>”"),
		("key: | x", "Expected a value, but found “|”"),
		(
			"[server]\nport: 1",
			"There are no table headers. Write the table as an object, as in server: {…}",
		),
		(
			"[[servers.http]]\nport: 1",
			"Unexpected “servers”. A string value must be quoted, as in 'servers.http'",
		),
		(
			"a: [server]",
			"Unexpected “server”. A string value must be quoted, as in 'server'",
		),
		(
			"name: 'x'\n\n[server]\nport: 1",
			"There are no table headers. Write the table as an object, as in server: {…}",
		),
		("a: 1\n[", "Expected a key, but found “[”"),
		(
			"{2026-09-19T14:00:00Z: 1}",
			"A key that contains “:” must be quoted, as in '2026-09-19T14:00:00Z'",
		),
		(
			"12:30: 'lunch'",
			"A key that contains “:” must be quoted, as in '12:30'",
		),
		(
			"a:b: 1",
			"A key that contains “:” must be quoted, as in 'a:b'",
		),
		(
			"a:b",
			"Unexpected “b”. A string value must be quoted, as in 'b'",
		),
		(
			"a:{b: nope}",
			"Unexpected “nope”. A string value must be quoted, as in 'nope'",
		),
		(
			"the name: 1",
			"A bare key cannot contain spaces. Quote it, as in 'the name'",
		),
		("a = 1", "Expected “:” after the key, but found “=”"),
		(
			"a$: 1",
			"Expected “:” after the key, but found “$”. A key that contains characters other than letters, digits, “_”, and “-” must be quoted",
		),
		("a:\nb: 1", "Expected a value, but found the key b"),
		(
			"a:\n\t12:30",
			"A time of day is a string, so it must be quoted",
		),
		(
			"a: 07:32:00",
			"A time of day is a string, so it must be quoted",
		),
		(
			"a: 12:00",
			"A time of day is a string, so it must be quoted",
		),
		(
			"a: 10:30 PM",
			"A time of day is a string, so it must be quoted",
		),
		(
			"name: John Smith",
			"Unexpected “John”. A string value must be quoted, as in 'John Smith'",
		),
		(
			"name: John Smith # The owner",
			"Unexpected “John”. A string value must be quoted, as in 'John Smith'",
		),
		(
			"name: John Smith \t\nisAdmin: true",
			"Unexpected “John”. A string value must be quoted, as in 'John Smith'",
		),
		(
			"a: foo.bar",
			"Unexpected “foo”. A string value must be quoted, as in 'foo.bar'",
		),
		(
			"a: foo/bar",
			"Unexpected “foo”. A string value must be quoted, as in 'foo/bar'",
		),
		(
			"a: v1.2.3",
			"Unexpected “v1”. A string value must be quoted, as in 'v1.2.3'",
		),
		(
			"a: C# and F#",
			"Unexpected “C”. A string value must be quoted, as in 'C# and F#'",
		),
		(
			r#"a: say "hi""#,
			r#"Unexpected “say”. A string value must be quoted, as in 'say "hi"'"#,
		),
		(
			"a: [foo bar, true]",
			"Unexpected “foo”. A string value must be quoted, as in 'foo bar'",
		),
		(
			"a: [foo bar]",
			"Unexpected “foo”. A string value must be quoted, as in 'foo bar'",
		),
		(
			"a: {b: foo bar}",
			"Unexpected “foo”. A string value must be quoted, as in 'foo bar'",
		),
		(
			"a: {b: foo bar, c: true}",
			"Unexpected “foo”. A string value must be quoted, as in 'foo bar'",
		),
		(
			"a: [\n\tfoo bar\n]",
			"Unexpected “foo”. A string value must be quoted, as in 'foo bar'",
		),
		(
			"a: it's here",
			"Unexpected “it”. A string value must be quoted",
		),
		(
			"url: https://example.com",
			"Unexpected “https”. A string value must be quoted, as in 'https://example.com'",
		),
		(
			"url: https://example.com/a?b=c # The API",
			"Unexpected “https”. A string value must be quoted, as in 'https://example.com/a?b=c'",
		),
		(
			"host: localhost:8080",
			"Unexpected “localhost”. A string value must be quoted, as in 'localhost:8080'",
		),
		(
			"a: {url: https://x, b: true}",
			"Unexpected “https”. A string value must be quoted, as in 'https://x'",
		),
		(
			"a: x:y",
			"Unexpected “x”. A string value must be quoted, as in 'x:y'",
		),
		(
			"msg: Error: file not found",
			"Unexpected “Error”. A string value must be quoted, as in 'Error: file not found'",
		),
		(
			"a: [Note: x, true]",
			"Expected a value, but found the key Note",
		),
		(
			"a: {msg: Error: x}",
			"Unexpected “Error”. A string value must be quoted, as in 'Error: x'",
		),
		(
			"a: b:",
			"Unexpected “b”. A string value must be quoted, as in 'b:'",
		),
		("a:\nb:\t1", "Expected a value, but found the key b"),
		("a:\nb:", "Expected a value, but found the key b"),
		("a:\nb:1\nc: 2", "Expected a value, but found the key b"),
		("{a:\nb:1}", "Expected a value, but found the key b"),
		(
			"a: [\n\t3\n\tc: 5\n]",
			"Expected a value, but found the key c",
		),
		("[a:1]", "Expected a value, but found the key a"),
		("x: [a:b, c]", "Expected a value, but found the key a"),
		(
			"a: [https://x, true]",
			"Expected a value, but found the key https",
		),
		("a: 0xff_ff", "Hexadecimal digits are uppercase: 0xFF_FF"),
		("a: 0xFf", "Hexadecimal digits are uppercase: 0xFF"),
		("a: 0x00ff", "Hexadecimal digits are uppercase: 0x00FF"),
		(
			"a: 0x_ff",
			"An underscore in a number must be between two digits",
		),
		(
			"a: 0xff_",
			"An underscore in a number must be between two digits",
		),
		(
			"a: 0xf__f",
			"An underscore in a number must be between two digits",
		),
		(
			"a: 0x_FF",
			"An underscore in a number must be between two digits",
		),
		("a: 0xa", "Hexadecimal digits are uppercase: 0xA"),
		("a: 0xf", "Hexadecimal digits are uppercase: 0xF"),
		("a: 0x0a", "Hexadecimal digits are uppercase: 0x0A"),
		("a: 0xa0", "Hexadecimal digits are uppercase: 0xA0"),
		("a: 0xab_cd", "Hexadecimal digits are uppercase: 0xAB_CD"),
		("a: 0xa_b_c", "Hexadecimal digits are uppercase: 0xA_B_C"),
		(
			"a: 0x7fffffffffffffff",
			"Hexadecimal digits are uppercase: 0x7FFFFFFFFFFFFFFF",
		),
		(
			"a: 0xdead_beef",
			"Hexadecimal digits are uppercase: 0xDEAD_BEEF",
		),
		(
			"a: 0x_a",
			"An underscore in a number must be between two digits",
		),
		(
			"a: 0xa_",
			"An underscore in a number must be between two digits",
		),
		(
			"a: 0xa__b",
			"An underscore in a number must be between two digits",
		),
		(
			"a: 0xa_b_",
			"An underscore in a number must be between two digits",
		),
		(
			"a: 0x_",
			"An underscore in a number must be between two digits",
		),
		(
			"a: 2024-02-29",
			"2024-02-29 is a date, not an instant. Write a date as a string, as in '2024-02-29'. An instant needs a time and an offset, as in 2024-02-29T00:00:00Z",
		),
		(
			"a: 0001-01-01",
			"0001-01-01 is a date, not an instant. Write a date as a string, as in '0001-01-01'. An instant needs a time and an offset, as in 0001-01-01T00:00:00Z",
		),
		(
			"a: 9999-12-31",
			"9999-12-31 is a date, not an instant. Write a date as a string, as in '9999-12-31'. An instant needs a time and an offset, as in 9999-12-31T00:00:00Z",
		),
		(
			"a: 2026-02-30",
			"2026-02-30 is a date, not an instant. Write a date as a string, as in '2026-02-30'",
		),
		(
			"a: 2025-02-29",
			"2025-02-29 is a date, not an instant. Write a date as a string, as in '2025-02-29'",
		),
		(
			"a: 1900-02-29",
			"1900-02-29 is a date, not an instant. Write a date as a string, as in '1900-02-29'",
		),
		(
			"a: 2026-04-31",
			"2026-04-31 is a date, not an instant. Write a date as a string, as in '2026-04-31'",
		),
		(
			"a: 2026-13-01",
			"2026-13-01 is a date, not an instant. Write a date as a string, as in '2026-13-01'",
		),
		(
			"a: 2026-00-10",
			"2026-00-10 is a date, not an instant. Write a date as a string, as in '2026-00-10'",
		),
		(
			"a: 2026-01-00",
			"2026-01-00 is a date, not an instant. Write a date as a string, as in '2026-01-00'",
		),
		(
			"a: 0000-01-01",
			"0000-01-01 is a date, not an instant. Write a date as a string, as in '0000-01-01'",
		),
		(
			"a: 1.7976931348623159e308",
			"1.7976931348623159e308 is too large to be a finite float. Use infinity if you mean it",
		),
		(
			"a: -1e999",
			"-1e999 is too large to be a finite float. Use -infinity if you mean it",
		),
		(
			"a: -1.7976931348623159e308",
			"-1.7976931348623159e308 is too large to be a finite float. Use -infinity if you mean it",
		),
		(
			"a: -1_000e1_000",
			"-1_000e1_000 is too large to be a finite float. Use -infinity if you mean it",
		),
		(
			"server localhost:8080",
			"Expected “:” after the key, but found “l”",
		),
		("time 12:30", "Expected “:” after the key, but found “1”"),
		(
			"homepage https://example.com",
			"Expected “:” after the key, but found “h”",
		),
		(
			"the name:\t1",
			"A bare key cannot contain spaces. Quote it, as in 'the name'",
		),
		(
			"the name:\n\t1",
			"A bare key cannot contain spaces. Quote it, as in 'the name'",
		),
		(
			"the name:",
			"A bare key cannot contain spaces. Quote it, as in 'the name'",
		),
		("the name:1", "Expected “:” after the key, but found “n”"),
		(
			"a: .5e3",
			"A number cannot begin with “.”; write a digit before it, as in 0.5",
		),
		(
			"a: .env",
			"Unexpected “.”. A string value must be quoted, as in '.env'",
		),
		(
			"a: ./foo # The path",
			"Unexpected “.”. A string value must be quoted, as in './foo'",
		),
		(
			"files: [.env, .git]",
			"Unexpected “.”. A string value must be quoted, as in '.env'",
		),
		(
			"a: ...",
			"Unexpected “.”. A string value must be quoted, as in '...'",
		),
		(
			"a: .",
			"Unexpected “.”. A string value must be quoted, as in '.'",
		),
		(
			"a: Yes",
			"“Yes” is not a value. Booleans are written true and false, in lowercase",
		),
		(
			"a: None # Unset",
			"“None” is not a value. Null is written null, in lowercase",
		),
		(
			"a: [NaN, 1]",
			"NaN is not representable. Use null for a missing value",
		),
		(
			"a: Inf",
			"“Inf” is not a value. Infinity is written infinity, in lowercase",
		),
		(
			"a: Yes please",
			"Unexpected “Yes”. A string value must be quoted, as in 'Yes please'",
		),
		(
			"status: On hold",
			"Unexpected “On”. A string value must be quoted, as in 'On hold'",
		),
		(
			"a: None of the above",
			"Unexpected “None”. A string value must be quoted, as in 'None of the above'",
		),
		(
			"name: Nan Goldin",
			"Unexpected “Nan”. A string value must be quoted, as in 'Nan Goldin'",
		),
		(
			"a: [Inf loop, 1]",
			"Unexpected “Inf”. A string value must be quoted, as in 'Inf loop'",
		),
		(
			"border: 1px solid black",
			"Invalid number “1px”. A string value must be quoted, as in '1px solid black'",
		),
		(
			"font: 12pt Arial # Body",
			"Invalid number “12pt”. A string value must be quoted, as in '12pt Arial'",
		),
		(
			"a: [1px solid, 2]",
			"Invalid number “1px”. A string value must be quoted, as in '1px solid'",
		),
		(
			"a: 1px",
			"Invalid number “1px”. A string value must be quoted, as in '1px'",
		),
		(
			"a: 3x it's",
			"Invalid number “3x”. A string value must be quoted",
		),
		(
			"phone: 0412 345 678",
			"Leading zeros are not allowed in a decimal number. Write an identifier, such as a ZIP code, as a string: '0412 345 678'",
		),
		(
			"date: 01/02/2026",
			"Leading zeros are not allowed in a decimal number. Write an identifier, such as a ZIP code, as a string: '01/02/2026'",
		),
		(
			"a: hello world /* note */",
			"Unexpected “hello”. A string value must be quoted, as in 'hello world'",
		),
		(
			"a: [hello world\t/* note */]",
			"Unexpected “hello”. A string value must be quoted, as in 'hello world'",
		),
		(
			"a: 1px solid /* note */",
			"Invalid number “1px”. A string value must be quoted, as in '1px solid'",
		),
		(
			"a: src/*.js",
			"Unexpected “src”. A string value must be quoted, as in 'src/*.js'",
		),
		(
			"a: 0x0000_7fff_ffff_ffff_ffff",
			"Hexadecimal digits are uppercase: 0x0000_7FFF_FFFF_FFFF_FFFF",
		),
		("a: 0x8000000000000000a", "Hexadecimal digits are uppercase"),
		("a: 0xffffffffffffffff", "Hexadecimal digits are uppercase"),
		("a: 0xffg", "Invalid hexadecimal digit “g”"),
		("a: 0xFfz", "Invalid hexadecimal digit “z”"),
		("a: 0xFG", "Invalid hexadecimal digit “G”"),
		("a: 0o78", "Invalid octal digit “8”"),
		("a: 0b102", "Invalid binary digit “2”"),
		("name=foo", "Expected “:” after the key, but found “=”"),
		("port=8080", "Expected “:” after the key, but found “=”"),
		("{a=1}", "Expected “:” after the key, but found “=”"),
		(
			"a: 01234 O'Brien",
			"Leading zeros are not allowed in a decimal number. Write an identifier, such as a ZIP code, as a string",
		),
		(
			"a: 0899 x'y",
			"Leading zeros are not allowed in a decimal number. Write an identifier, such as a ZIP code, as a string",
		),
		(
			"a: #don't",
			"Expected a value, but reached the end of the document. “#” starts a comment, so a value that starts with “#” must be quoted",
		),
		(
			"a: #'x'",
			"Expected a value, but reached the end of the document. “#” starts a comment, so a value that starts with “#” must be quoted",
		),
		(
			"a: #it's\nb: 1",
			"Expected a value, but found the key b. “#” starts a comment, so a value that starts with “#” must be quoted",
		),
		(
			"{a: #it's\n}",
			"Expected a value, but found “}”. “#” starts a comment, so a value that starts with “#” must be quoted",
		),
		(
			"a: #dont",
			"Expected a value, but reached the end of the document. “#” starts a comment, so a value that starts with “#” must be quoted, as in '#dont'",
		),
		(
			r#"a: #say"hi""#,
			r#"Expected a value, but reached the end of the document. “#” starts a comment, so a value that starts with “#” must be quoted, as in '#say"hi"'"#,
		),
		(
			"a: #FFF",
			"Expected a value, but reached the end of the document. “#” starts a comment, so a value that starts with “#” must be quoted, as in '#FFF'",
		),
		(
			r#"a: #a\b"#,
			r#"Expected a value, but reached the end of the document. “#” starts a comment, so a value that starts with “#” must be quoted, as in '#a\b'"#,
		),
		(
			"a: #1.5",
			"Expected a value, but reached the end of the document. “#” starts a comment, so a value that starts with “#” must be quoted, as in '#1.5'",
		),
		(
			"a: #é😀",
			"Expected a value, but reached the end of the document. “#” starts a comment, so a value that starts with “#” must be quoted, as in '#é😀'",
		),
		(
			"a: 0777777777777777777777",
			"Leading zeros are not allowed in a decimal number. Write an octal number, such as a file mode, as 0o777777777777777777777, and an identifier, such as a ZIP code, as a string: '0777777777777777777777'",
		),
		(
			"a: 0000777777777777777777777",
			"Leading zeros are not allowed in a decimal number. Write an octal number, such as a file mode, as 0o777777777777777777777, and an identifier, such as a ZIP code, as a string: '0000777777777777777777777'",
		),
		(
			"a: 01000000000000000000000",
			"Leading zeros are not allowed in a decimal number. Write an identifier, such as a ZIP code, as a string: '01000000000000000000000'",
		),
		(
			"a: 01777777777777777777777",
			"Leading zeros are not allowed in a decimal number. Write an identifier, such as a ZIP code, as a string: '01777777777777777777777'",
		),
		(
			"a: 07777777777777777777777777",
			"Leading zeros are not allowed in a decimal number. Write an identifier, such as a ZIP code, as a string: '07777777777777777777777777'",
		),
		(
			"a: 00",
			"Leading zeros are not allowed in a decimal number. Write an octal number, such as a file mode, as 0o0, and an identifier, such as a ZIP code, as a string: '00'",
		),
		(
			"a: 07",
			"Leading zeros are not allowed in a decimal number. Write an octal number, such as a file mode, as 0o7, and an identifier, such as a ZIP code, as a string: '07'",
		),
		(
			"a: 0644",
			"Leading zeros are not allowed in a decimal number. Write an octal number, such as a file mode, as 0o644, and an identifier, such as a ZIP code, as a string: '0644'",
		),
		(
			"a: 0755",
			"Leading zeros are not allowed in a decimal number. Write an octal number, such as a file mode, as 0o755, and an identifier, such as a ZIP code, as a string: '0755'",
		),
		(
			"a: 00007",
			"Leading zeros are not allowed in a decimal number. Write an octal number, such as a file mode, as 0o7, and an identifier, such as a ZIP code, as a string: '00007'",
		),
		(
			"a: 000000000000000000000000000000",
			"Leading zeros are not allowed in a decimal number. Write an octal number, such as a file mode, as 0o0, and an identifier, such as a ZIP code, as a string: '000000000000000000000000000000'",
		),
		(
			"a: [9,223,372,036,854,775,807]",
			"Leading zeros are not allowed in a decimal number. A comma separates items, so 9,223,372,036,854,775,807 is not one number. Write it as 9_223_372_036_854_775_807 or 9223372036854775807",
		),
		(
			"a: [-9,223,372,036,854,775,808]",
			"Leading zeros are not allowed in a decimal number. A comma separates items, so -9,223,372,036,854,775,808 is not one number. Write it as -9_223_372_036_854_775_808 or -9223372036854775808",
		),
		(
			"a: [9,223,372,036,854,775,808]",
			"Leading zeros are not allowed in a decimal number. A comma separates items, so 9,223,372,036,854,775,808 is not one number",
		),
		(
			"a: [-9,223,372,036,854,775,809]",
			"Leading zeros are not allowed in a decimal number. A comma separates items, so -9,223,372,036,854,775,809 is not one number",
		),
		(
			"a: [10,000,000,000,000,000,000]",
			"Leading zeros are not allowed in a decimal number. A comma separates items, so 10,000,000,000,000,000,000 is not one number",
		),
		(
			"a: [999,000]",
			"Leading zeros are not allowed in a decimal number. A comma separates items, so 999,000 is not one number. Write it as 999_000 or 999000",
		),
		(
			"a: 2024-02-29 14:00:00Z",
			"The date and time separator in an instant is an uppercase “T”, not a space, as in 2024-02-29T14:00:00Z",
		),
		(
			"a: 2026-01-01 14:00:00.123456789Z",
			"The date and time separator in an instant is an uppercase “T”, not a space, as in 2026-01-01T14:00:00.123456789Z",
		),
		(
			"a: 2026-02-30 14:00:00Z",
			"The date and time separator in an instant is an uppercase “T”, not a space. An instant is written as 2026-09-19T14:00:00Z, with an optional fraction of up to nine digits and an offset of Z or ±HH:MM",
		),
		(
			"a: 2025-02-29 14:00:00Z",
			"The date and time separator in an instant is an uppercase “T”, not a space. An instant is written as 2026-09-19T14:00:00Z, with an optional fraction of up to nine digits and an offset of Z or ±HH:MM",
		),
		(
			"a: 2026-01-01 24:00:00Z",
			"The date and time separator in an instant is an uppercase “T”, not a space. An instant is written as 2026-09-19T14:00:00Z, with an optional fraction of up to nine digits and an offset of Z or ±HH:MM",
		),
		(
			"a: 2026-01-01 14:60:00Z",
			"The date and time separator in an instant is an uppercase “T”, not a space. An instant is written as 2026-09-19T14:00:00Z, with an optional fraction of up to nine digits and an offset of Z or ±HH:MM",
		),
		(
			"a: 2026-01-01 14:00:60Z",
			"The date and time separator in an instant is an uppercase “T”, not a space. An instant is written as 2026-09-19T14:00:00Z, with an optional fraction of up to nine digits and an offset of Z or ±HH:MM",
		),
		(
			"a: 2026-01-01 14:00:00.1234567890Z",
			"The date and time separator in an instant is an uppercase “T”, not a space. An instant is written as 2026-09-19T14:00:00Z, with an optional fraction of up to nine digits and an offset of Z or ±HH:MM",
		),
		(
			"a: 2026-01-01 14:00:00-00:00",
			"The date and time separator in an instant is an uppercase “T”, not a space. An instant is written as 2026-09-19T14:00:00Z, with an optional fraction of up to nine digits and an offset of Z or ±HH:MM",
		),
		(
			"a: 2026-01-01 14:00:00+24:00",
			"The date and time separator in an instant is an uppercase “T”, not a space. An instant is written as 2026-09-19T14:00:00Z, with an optional fraction of up to nine digits and an offset of Z or ±HH:MM",
		),
		(
			"a: 0001-01-01 00:00:00+01:00",
			"The date and time separator in an instant is an uppercase “T”, not a space. An instant is written as 2026-09-19T14:00:00Z, with an optional fraction of up to nine digits and an offset of Z or ±HH:MM",
		),
		(
			"a: 9999-12-31 23:59:59-01:00",
			"The date and time separator in an instant is an uppercase “T”, not a space. An instant is written as 2026-09-19T14:00:00Z, with an optional fraction of up to nine digits and an offset of Z or ±HH:MM",
		),
		(
			"a: 2024-02-29 14:00:00",
			"The date and time separator in an instant is an uppercase “T”, not a space, and an instant needs the offset it was meant in, as in 2024-02-29T14:00:00Z for UTC. A date and time without an offset is a local time, which is not an instant, so write it as a string, as in '2024-02-29 14:00:00'",
		),
		(
			"a: 2026-02-30 14:00:00",
			"The date and time separator in an instant is an uppercase “T”, not a space. An instant is written as 2026-09-19T14:00:00Z, with an optional fraction of up to nine digits and an offset of Z or ±HH:MM",
		),
		(
			"a: 2026-01-01 25:00:00",
			"The date and time separator in an instant is an uppercase “T”, not a space. An instant is written as 2026-09-19T14:00:00Z, with an optional fraction of up to nine digits and an offset of Z or ±HH:MM",
		),
		(
			"a: 0000-01-01 00:00:00",
			"The date and time separator in an instant is an uppercase “T”, not a space. An instant is written as 2026-09-19T14:00:00Z, with an optional fraction of up to nine digits and an offset of Z or ±HH:MM",
		),
		(
			"a: 1.5 us",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 1.5us, and anything else, such as a size, as a string, as in '1.5 us'",
		),
		(
			"a: -5 m",
			"A unit cannot follow a number after a space. Write a duration without the space, as in -5m, and anything else, such as a size, as a string, as in '-5 m'",
		),
		(
			"a: 1_000 ms",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 1_000ms, and anything else, such as a size, as a string, as in '1_000 ms'",
		),
		(
			"a: 2562047 h",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 2562047h, and anything else, such as a size, as a string, as in '2562047 h'",
		),
		(
			"a: 1.5 ns",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 10s, and anything else, such as a size, as a string, as in '1.5 ns'",
		),
		(
			"a: 0.0000000001 s",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 10s, and anything else, such as a size, as a string, as in '0.0000000001 s'",
		),
		(
			"a: -0.0 s",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 10s, and anything else, such as a size, as a string, as in '-0.0 s'",
		),
		(
			"a: 2562048 h",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 10s, and anything else, such as a size, as a string, as in '2562048 h'",
		),
		(
			"a: 3000000 h",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 10s, and anything else, such as a size, as a string, as in '3000000 h'",
		),
		(
			"a: 1e3 ms",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 10s, and anything else, such as a size, as a string, as in '1e3 ms'",
		),
		(
			"a: 0x10 s",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 10s, and anything else, such as a size, as a string, as in '0x10 s'",
		),
		(
			"a: 1 h",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 1h, and anything else, such as a size, as a string, as in '1 h'",
		),
		(
			"a: 1 m",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 1m, and anything else, such as a size, as a string, as in '1 m'",
		),
		(
			"a: 1 s",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 1s, and anything else, such as a size, as a string, as in '1 s'",
		),
		(
			"a: 1 ms",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 1ms, and anything else, such as a size, as a string, as in '1 ms'",
		),
		(
			"a: 1 us",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 1us, and anything else, such as a size, as a string, as in '1 us'",
		),
		(
			"a: 1 ns",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 1ns, and anything else, such as a size, as a string, as in '1 ns'",
		),
		(
			"a: -1 h",
			"A unit cannot follow a number after a space. Write a duration without the space, as in -1h, and anything else, such as a size, as a string, as in '-1 h'",
		),
		(
			"a: -1 m",
			"A unit cannot follow a number after a space. Write a duration without the space, as in -1m, and anything else, such as a size, as a string, as in '-1 m'",
		),
		(
			"a: -1 s",
			"A unit cannot follow a number after a space. Write a duration without the space, as in -1s, and anything else, such as a size, as a string, as in '-1 s'",
		),
		(
			"a: -1 ms",
			"A unit cannot follow a number after a space. Write a duration without the space, as in -1ms, and anything else, such as a size, as a string, as in '-1 ms'",
		),
		(
			"a: -1 us",
			"A unit cannot follow a number after a space. Write a duration without the space, as in -1us, and anything else, such as a size, as a string, as in '-1 us'",
		),
		(
			"a: -1 ns",
			"A unit cannot follow a number after a space. Write a duration without the space, as in -1ns, and anything else, such as a size, as a string, as in '-1 ns'",
		),
		(
			"a: 0 h",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 0h, and anything else, such as a size, as a string, as in '0 h'",
		),
		(
			"a: 0 m",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 0m, and anything else, such as a size, as a string, as in '0 m'",
		),
		(
			"a: 0 s",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 0s, and anything else, such as a size, as a string, as in '0 s'",
		),
		(
			"a: 0 ms",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 0ms, and anything else, such as a size, as a string, as in '0 ms'",
		),
		(
			"a: 0 us",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 0us, and anything else, such as a size, as a string, as in '0 us'",
		),
		(
			"a: 0 ns",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 0ns, and anything else, such as a size, as a string, as in '0 ns'",
		),
		(
			"a: 0.5 h",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 0.5h, and anything else, such as a size, as a string, as in '0.5 h'",
		),
		(
			"a: 0.5 m",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 0.5m, and anything else, such as a size, as a string, as in '0.5 m'",
		),
		(
			"a: 0.5 s",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 0.5s, and anything else, such as a size, as a string, as in '0.5 s'",
		),
		(
			"a: 0.5 ms",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 0.5ms, and anything else, such as a size, as a string, as in '0.5 ms'",
		),
		(
			"a: 0.5 us",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 0.5us, and anything else, such as a size, as a string, as in '0.5 us'",
		),
		(
			"a: 0.5 ns",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 10s, and anything else, such as a size, as a string, as in '0.5 ns'",
		),
		(
			"a: 1.5 h",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 1.5h, and anything else, such as a size, as a string, as in '1.5 h'",
		),
		(
			"a: 1.5 m",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 1.5m, and anything else, such as a size, as a string, as in '1.5 m'",
		),
		(
			"a: 1.5 s",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 1.5s, and anything else, such as a size, as a string, as in '1.5 s'",
		),
		(
			"a: 1.5 ms",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 1.5ms, and anything else, such as a size, as a string, as in '1.5 ms'",
		),
		(
			"a: -1.5 h",
			"A unit cannot follow a number after a space. Write a duration without the space, as in -1.5h, and anything else, such as a size, as a string, as in '-1.5 h'",
		),
		(
			"a: 1µs",
			"The unit for microseconds is written us, as in 1us",
		),
		(
			"a: -1_000μs",
			"The unit for microseconds is written us, as in -1_000us",
		),
		(
			"a: 0.001µs",
			"The unit for microseconds is written us, as in 0.001us",
		),
		("a: 1.0005µs", "The unit for microseconds is written us"),
		("a: -0.0µs", "The unit for microseconds is written us"),
		("a: 1e3µs", "The unit for microseconds is written us"),
		("a: 0x10µs", "The unit for microseconds is written us"),
		(
			"a: 9223372036854775µs",
			"The unit for microseconds is written us, as in 9223372036854775us",
		),
		(
			"a: 9223372036854776µs",
			"The unit for microseconds is written us",
		),
		(
			"# The server.\n[server]\nport: 1",
			"There are no table headers. Write the table as an object, as in server: {…}",
		),
		(
			"\t[server]\nport: 1",
			"There are no table headers. Write the table as an object, as in server: {…}",
		),
		(
			"[[servers]]\nport: 1",
			"There are no table headers. Write the table as an object, as in servers: {…}",
		),
		(
			"a: 1\n[server]\nport: 1",
			"There are no table headers. Write the table as an object, as in server: {…}",
		),
		(
			"a: {\n\t[server]\n}",
			"There are no table headers. Write the table as an object, as in server: {…}",
		),
		(
			"roles: [\n\t[admin]\n]",
			"Unexpected “admin”. A string value must be quoted, as in 'admin'",
		),
		(
			"a: [\n\t1,\n\t[x]\n]",
			"Unexpected “x”. A string value must be quoted, as in 'x'",
		),
		(
			"[\n\t1,\n\t[x]\n]",
			"Unexpected “x”. A string value must be quoted, as in 'x'",
		),
		(
			"[\n\t[[x]]\n]",
			"Unexpected “x”. A string value must be quoted, as in 'x'",
		),
		(
			"a:\n[server]\nport: 1",
			"Unexpected “server”. A string value must be quoted, as in 'server'",
		),
		(
			"a:\n  [server]",
			"Unexpected “server”. A string value must be quoted, as in 'server'",
		),
		(
			"{a:\n[server]\n}",
			"Unexpected “server”. A string value must be quoted, as in 'server'",
		),
		// A bare key cannot contain a dot, so a name with one is not a table header.
		(
			"[a.b]\nport: 1",
			"Unexpected “a”. A string value must be quoted, as in 'a.b'",
		),
		(
			"[a.b-c.d_e]\nport: 1",
			"Unexpected “a”. A string value must be quoted, as in 'a.b-c.d_e'",
		),
		(
			"[[a.b]]\nport: 1",
			"Unexpected “a”. A string value must be quoted, as in 'a.b'",
		),
		("x: 1\n[a.b]\nport: 1", "Expected a key, but found “[”"),
		(
			"[a..b]\nport: 1",
			"Unexpected “a”. A string value must be quoted, as in 'a..b'",
		),
		(
			"[a.]\nport: 1",
			"Unexpected “a”. A string value must be quoted, as in 'a.'",
		),
		(
			"[[a..b]]\nport: 1",
			"Unexpected “a”. A string value must be quoted, as in 'a..b'",
		),
		("x: 1\n[a..b]\nport: 1", "Expected a key, but found “[”"),
		("x: 1\n[a.]\nport: 1", "Expected a key, but found “[”"),
		(
			"[a]\nport: 1",
			"There are no table headers. Write the table as an object, as in a: {…}",
		),
		(
			"[a.b.c]\nport: 1",
			"Unexpected “a”. A string value must be quoted, as in 'a.b.c'",
		),
		(
			"[a-b]\nport: 1",
			"There are no table headers. Write the table as an object, as in a-b: {…}",
		),
		(
			"[_a]\nport: 1",
			"There are no table headers. Write the table as an object, as in _a: {…}",
		),
		(
			"[a.b.]\nport: 1",
			"Unexpected “a”. A string value must be quoted, as in 'a.b.'",
		),
		(
			"[a...b]\nport: 1",
			"Unexpected “a”. A string value must be quoted, as in 'a...b'",
		),
		(
			"'a.b':\n  'c.d': 1",
			"Expected a value, but found the key \"c.d\". Indentation does not nest objects, so write 'a.b': {'c.d': …}",
		),
		(
			"'the server':\n  'the port': 80",
			"Expected a value, but found the key \"the port\". Indentation does not nest objects, so write 'the server': {'the port': …}",
		),
		(
			"\"it's\":\n  b: 1",
			r#"Expected a value, but found the key b. Indentation does not nest objects, so write "it's": {b: …}"#,
		),
		(
			"\"a\\u{1}\":\n  b: 1",
			r#"Expected a value, but found the key b. Indentation does not nest objects, so write "a\u{1}": {b: …}"#,
		),
		(
			"a:\n  \"\\u{8}\\u{c}\\u{7f}\": 1",
			concat!(
				r#"Expected a value, but found the key "\b\f"#,
				"\u{FFFD}",
				r#"". Indentation does not nest objects, so write a: {"\u{8}\u{c}\u{7f}": …}"#
			),
		),
		(
			"a:\n  \"b\\\\c\": 1",
			r#"Expected a value, but found the key "b\\c". Indentation does not nest objects, so write a: {'b\c': …}"#,
		),
		(
			"a:\n  b: 1",
			"Expected a value, but found the key b. Indentation does not nest objects, so write a: {b: …}",
		),
		(
			"'a b':\n  \"c\\u{1}\": 1",
			r#"Expected a value, but found the key "c\u0001". Indentation does not nest objects, so write 'a b': {"c\u{1}": …}"#,
		),
		(
			"\"\\u{7f}\":\n  \"\\t\": 1",
			r#"Expected a value, but found the key "\t". Indentation does not nest objects, so write "\u{7f}": {"\t": …}"#,
		),
		(
			"'x.y':\n  'é😀': 1",
			"Expected a value, but found the key \"é😀\". Indentation does not nest objects, so write 'x.y': {'é😀': …}",
		),
	];
	let mut failures = Vec::new();

	for (text, expected) in cases {
		match soml::from_str::<Value>(text) {
			Ok(_) => failures.push(format!("{text:?} was accepted")),
			Err(error) if error.message() == expected => {}
			Err(error) => failures.push(format!(
				"{text:?}: {}, expected {expected}",
				error.message()
			)),
		}
	}

	assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn a_long_hexadecimal_number_is_diagnosed_whole() {
	let message = |digits: &str| {
		soml::from_str::<Value>(&format!("a: 0x{}{digits}", "1".repeat(1100)))
			.expect_err("an invalid number")
			.message()
			.to_owned()
	};

	// The lowercase digits come after the length that is diagnosed, so the whole number decides the reason.
	assert_eq!(message("ff"), "Hexadecimal digits are uppercase");
	assert_eq!(message("f_f"), "Hexadecimal digits are uppercase");
	assert_eq!(
		message("f__f"),
		"An underscore in a number must be between two digits"
	);
}

#[test]
fn a_long_number_with_leading_zeros_is_diagnosed_whole() {
	let message = |last_digit: &str| {
		soml::from_str::<Value>(&format!("a: 0{}{last_digit}", "0".repeat(1100)))
			.expect_err("an invalid number")
			.message()
			.to_owned()
	};
	let expected = format!(
		"Leading zeros are not allowed in a decimal number. Write an identifier, such as a ZIP code, as a string: '{}…'",
		"0".repeat(40)
	);

	// The digit that is not octal comes after the length that is diagnosed, so the whole number decides that there is no octal suggestion.
	assert_eq!(message("8"), expected);
	assert_eq!(message("7"), expected);

	// Whether the identifier is suggested is decided on the length that is diagnosed, as in the reference.
	assert_eq!(message("x"), expected);
}

#[test]
fn a_hint_gives_no_example_for_a_token_longer_than_the_diagnosed_length() {
	let message = |text: &str| {
		soml::from_str::<Value>(text)
			.expect_err("an error")
			.message()
			.to_owned()
	};

	assert_eq!(
		message(&format!("${}: 1", "a".repeat(998))),
		format!(
			"Expected a key, but found “$”. A key that contains characters other than letters, digits, “_”, and “-” must be quoted, as in '${}…'",
			"a".repeat(39)
		)
	);
	assert_eq!(
		message(&format!("${}: 1", "a".repeat(999))),
		"Expected a key, but found “$”. A key that contains characters other than letters, digits, “_”, and “-” must be quoted"
	);
	assert_eq!(
		message(&format!("a: {}M", "1".repeat(999))),
		format!(
			"Invalid duration {}…: there is no month unit, because a month is not a fixed length. A size, such as 512M, is a string: '{}…'",
			"1".repeat(40),
			"1".repeat(40)
		)
	);
	assert_eq!(
		message(&format!("a: {}M", "1".repeat(1000))),
		format!(
			"Invalid duration {}…: there is no month unit, because a month is not a fixed length",
			"1".repeat(40)
		)
	);

	// The length counts UTF-16 code units, not bytes, so the `'` that rules out an example is seen as in the reference.
	assert_eq!(
		message(&format!("a: 01 {}'", "é".repeat(996))),
		"Leading zeros are not allowed in a decimal number. Write an identifier, such as a ZIP code, as a string"
	);
	assert_eq!(
		message(&format!("a: 01 {}'", "é".repeat(997))),
		format!(
			"Leading zeros are not allowed in a decimal number. Write an identifier, such as a ZIP code, as a string: '01 {}…'",
			"é".repeat(37)
		)
	);
	assert_eq!(
		message(&format!("a: #{}'", "é".repeat(998))),
		"Expected a value, but reached the end of the document. “#” starts a comment, so a value that starts with “#” must be quoted"
	);
	assert_eq!(
		message(&format!("a: #{}'", "é".repeat(999))),
		format!(
			"Expected a value, but reached the end of the document. “#” starts a comment, so a value that starts with “#” must be quoted, as in '#{}…'",
			"é".repeat(39)
		)
	);
}

/**
Checks the reason, without the position, of each document that must be invalid, and reports every mismatch at once.
*/
fn assert_reasons(cases: &[(&str, &str)]) {
	let mut failures = Vec::new();

	for (text, expected) in cases {
		match soml::from_str::<Value>(text) {
			Ok(_) => failures.push(format!("{text:?} was accepted")),
			Err(error) if error.message() == *expected => {}
			Err(error) => failures.push(format!(
				"{text:?}: {}, expected {expected}",
				error.message()
			)),
		}
	}

	assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/**
The value of a Unicode escape with uppercase digits is checked before its spelling, so that lowercasing the digits never gives an escape that is not allowed either.
*/
#[test]
fn the_value_of_a_unicode_escape_with_uppercase_digits_is_checked_before_its_spelling() {
	assert_reasons(&[
		(
			r#"a: "\u{D}""#,
			r"A carriage return (U+000D) cannot be represented, so \u{D} is not allowed",
		),
		(
			r#"a: "\u{00D}""#,
			r"A carriage return (U+000D) cannot be represented, so \u{00D} is not allowed",
		),
		(
			r#"a: "\u{D800}""#,
			r"\u{D800} is a surrogate, which is not a Unicode scalar value",
		),
		(
			r#"a: "\u{dFfF}""#,
			r"\u{dFfF} is a surrogate, which is not a Unicode scalar value",
		),
		(
			r#"a: "\u{FFFFFF}""#,
			r"\u{FFFFFF} is above U+10FFFF, the largest Unicode scalar value",
		),
		(
			r#"a: "\u{11000F}""#,
			r"\u{11000F} is above U+10FFFF, the largest Unicode scalar value",
		),
		(
			"a: \"\"\"\n\t\\u{D}\n\t\"\"\"",
			r"A carriage return (U+000D) cannot be represented, so \u{D} is not allowed",
		),
		(
			r#""\u{D}": 1"#,
			r"A carriage return (U+000D) cannot be represented, so \u{D} is not allowed",
		),
		(
			r#"a: "\u{E9}""#,
			"A Unicode escape uses lowercase hexadecimal digits",
		),
		(
			r#"a: "\u{1F600}""#,
			"A Unicode escape uses lowercase hexadecimal digits",
		),
		(
			r#"a: "\u{10FFFF}""#,
			"A Unicode escape uses lowercase hexadecimal digits",
		),
		(
			r#"a: "\u{00E9}""#,
			"A Unicode escape uses lowercase hexadecimal digits",
		),
		(
			r#"a: "\u{D7FF}""#,
			"A Unicode escape uses lowercase hexadecimal digits",
		),
		(
			r#"a: "\u{E000}""#,
			"A Unicode escape uses lowercase hexadecimal digits",
		),
		(
			r#"a: "\u{C}""#,
			"A Unicode escape uses lowercase hexadecimal digits",
		),
		(
			r#"a: "\u{FFFFFFF}""#,
			"A Unicode escape uses lowercase hexadecimal digits",
		),
		(
			r#"a: "\u{1234567}""#,
			"A Unicode escape has at most six hexadecimal digits",
		),
	]);
}

/**
An offset after a space is the offset an instant was meant in, so the instant is not a local time to quote as a string.
*/
#[test]
fn an_offset_after_a_space_is_the_offset_an_instant_was_meant_in() {
	assert_reasons(&[
		(
			"a: 2026-09-19T14:00:00 Z",
			"An instant's offset follows its time directly, without a space, as in 2026-09-19T14:00:00Z",
		),
		(
			"a: 2026-09-19T14:00:00 +02:00",
			"An instant's offset follows its time directly, without a space, as in 2026-09-19T14:00:00+02:00",
		),
		(
			"a: 2026-09-19T14:00:00.5 -07:30",
			"An instant's offset follows its time directly, without a space, as in 2026-09-19T14:00:00.5-07:30",
		),
		(
			"a: 2026-09-19T14:00:00\t\tZ # note",
			"An instant's offset follows its time directly, without a space, as in 2026-09-19T14:00:00Z",
		),
		(
			"a: [2026-09-19T14:00:00 Z, 1]",
			"An instant's offset follows its time directly, without a space, as in 2026-09-19T14:00:00Z",
		),
		(
			"a: {b: 2026-09-19T14:00:00 Z}",
			"An instant's offset follows its time directly, without a space, as in 2026-09-19T14:00:00Z",
		),
		(
			"a: [\n\t2026-09-19T14:00:00 Z\n]",
			"An instant's offset follows its time directly, without a space, as in 2026-09-19T14:00:00Z",
		),
		(
			"a: 2026-02-30T14:00:00 Z",
			"An instant's offset follows its time directly, without a space",
		),
		(
			"a: 0001-01-01T00:00:00 +00:01",
			"An instant's offset follows its time directly, without a space",
		),
		(
			"a: 2026-09-19T14:00:00 +24:00",
			"An instant's offset follows its time directly, without a space",
		),
		(
			"a: 2026-09-19T14:00:00 +02:00 # é",
			"An instant's offset follows its time directly, without a space, as in 2026-09-19T14:00:00+02:00",
		),
		(
			"a: 2026-09-19T14:00:00",
			"An instant needs an offset: Z or ±HH:MM. A date and time without an offset is a local time, which is not an instant, so write it as a string, as in '2026-09-19T14:00:00', or add the offset it was meant in",
		),
		(
			"a: 2026-09-19T14:00:00 # Z",
			"An instant needs an offset: Z or ±HH:MM. A date and time without an offset is a local time, which is not an instant, so write it as a string, as in '2026-09-19T14:00:00', or add the offset it was meant in",
		),
		(
			"a: 2026-09-19T14:00:00 Zulu",
			"An instant needs an offset: Z or ±HH:MM. A date and time without an offset is a local time, which is not an instant, so write it as a string, as in '2026-09-19T14:00:00', or add the offset it was meant in",
		),
		(
			"a: 2026-09-19T14:00:00 +02:00:00",
			"An instant needs an offset: Z or ±HH:MM. A date and time without an offset is a local time, which is not an instant, so write it as a string, as in '2026-09-19T14:00:00', or add the offset it was meant in",
		),
		(
			"a: 2026-09-19T14:00:00 +0200",
			"An instant needs an offset: Z or ±HH:MM. A date and time without an offset is a local time, which is not an instant, so write it as a string, as in '2026-09-19T14:00:00', or add the offset it was meant in",
		),
		(
			"a: 2026-09-19T14:00:00\nZ: 1",
			"An instant needs an offset: Z or ±HH:MM. A date and time without an offset is a local time, which is not an instant, so write it as a string, as in '2026-09-19T14:00:00', or add the offset it was meant in",
		),
		(
			"a: 2026-09-19T14:00 Z",
			"Invalid instant “2026-09-19T14:00”. An instant is written as 2026-09-19T14:00:00Z, with an optional fraction of up to nine digits and an offset of Z or ±HH:MM",
		),
		(
			"a: 2026-09-19T14:00:00Z Z",
			"Expected a line break before the next entry, but found “Z”",
		),
		(
			"a: 2026-09-19T14:00:00 Montréal",
			"An instant needs an offset: Z or ±HH:MM. A date and time without an offset is a local time, which is not an instant, so write it as a string, as in '2026-09-19T14:00:00', or add the offset it was meant in",
		),
		(
			"a: 2026-09-19T14:00:00 é",
			"An instant needs an offset: Z or ±HH:MM. A date and time without an offset is a local time, which is not an instant, so write it as a string, as in '2026-09-19T14:00:00', or add the offset it was meant in",
		),
		(
			"a: [2026-09-19 😀😀]",
			"2026-09-19 is a date, not an instant. Write a date as a string, as in '2026-09-19'. An instant needs a time and an offset, as in 2026-09-19T00:00:00Z",
		),
		(
			"a: 2026-09-19 Montréal",
			"2026-09-19 is a date, not an instant. Write a date as a string, as in '2026-09-19'. An instant needs a time and an offset, as in 2026-09-19T00:00:00Z",
		),
		(
			"a: 2026-09-19T14:00 x日本",
			"Invalid instant “2026-09-19T14:00”. An instant is written as 2026-09-19T14:00:00Z, with an optional fraction of up to nine digits and an offset of Z or ±HH:MM",
		),
		(
			"a: 2026-09-19T14:00:00 Zé",
			"An instant needs an offset: Z or ±HH:MM. A date and time without an offset is a local time, which is not an instant, so write it as a string, as in '2026-09-19T14:00:00', or add the offset it was meant in",
		),
		(
			"a: 2026-09-19T14:00:00 +02:0é",
			"An instant needs an offset: Z or ±HH:MM. A date and time without an offset is a local time, which is not an instant, so write it as a string, as in '2026-09-19T14:00:00', or add the offset it was meant in",
		),
		(
			"a: 1979-05-27 07:32:00 Z",
			"The date and time separator in an instant is an uppercase “T”, not a space, and an instant needs the offset it was meant in, as in 1979-05-27T07:32:00Z for UTC. A date and time without an offset is a local time, which is not an instant, so write it as a string, as in '1979-05-27 07:32:00'",
		),
	]);
}

/**
A value with a “:” that starts with a zero, such as a MAC address, is not a number with a leading zero.
*/
#[test]
fn a_value_with_a_colon_that_starts_with_a_zero_is_not_a_number_with_a_leading_zero() {
	assert_reasons(&[
		(
			"mac: 00:1A:2B:3C:4D:5E",
			"Invalid number “00:1A:2B:3C:4D:5E”. A string value must be quoted, as in '00:1A:2B:3C:4D:5E'",
		),
		(
			"mac: 01:23:45:67:89:AB",
			"Invalid number “01:23:45:67:89:AB”. A string value must be quoted, as in '01:23:45:67:89:AB'",
		),
		(
			"a: 0123:abcd::1",
			"Invalid number “0123:abcd::1”. A string value must be quoted, as in '0123:abcd::1'",
		),
		(
			"a: [00:1A:2B, 1]",
			"Invalid number “00:1A:2B”. A string value must be quoted, as in '00:1A:2B'",
		),
		("a: -01:30", "Invalid number “-01:30”"),
		(
			"a: 01",
			"Leading zeros are not allowed in a decimal number. Write an octal number, such as a file mode, as 0o1, and an identifier, such as a ZIP code, as a string: '01'",
		),
		(
			"a: 01A",
			"Leading zeros are not allowed in a decimal number",
		),
		(
			"a: 00.5",
			"Leading zeros are not allowed in a decimal number",
		),
		(
			"a: 00:11:22:33:44:55",
			"Invalid number “00:11:22:33:44:55”. A value that contains “:” must be quoted, as in '00:11:22:33:44:55'",
		),
		(
			"a: 08:30",
			"A time of day is a string, so it must be quoted",
		),
		("a: 1:", "Invalid number “1:”"),
		(
			"a: 2001:db8::1",
			"Invalid number “2001:db8::1”. A string value must be quoted, as in '2001:db8::1'",
		),
	]);
}

/**
A duration in years is reported as one, as one in days or weeks is, and a word that only starts with a year unit is a string to quote.
*/
#[test]
fn a_duration_in_years_is_reported_as_one() {
	assert_reasons(&[
		(
			"a: 1y",
			"Invalid duration 1y: there is no year unit, because a year is not a fixed length. Write 8760h for a fixed 365 days",
		),
		(
			"a: [1y, 1]",
			"Invalid duration 1y: there is no year unit, because a year is not a fixed length. Write 8760h for a fixed 365 days",
		),
		(
			"a: 2years",
			"Invalid duration 2years: there is no year unit, because a year is not a fixed length. Write 8760h for a fixed 365 days",
		),
		(
			"a: [2years, 1]",
			"Invalid duration 2years: there is no year unit, because a year is not a fixed length. Write 8760h for a fixed 365 days",
		),
		(
			"a: 1year",
			"Invalid duration 1year: there is no year unit, because a year is not a fixed length. Write 8760h for a fixed 365 days",
		),
		(
			"a: [1year, 1]",
			"Invalid duration 1year: there is no year unit, because a year is not a fixed length. Write 8760h for a fixed 365 days",
		),
		(
			"a: 3yr",
			"Invalid duration 3yr: there is no year unit, because a year is not a fixed length. Write 8760h for a fixed 365 days",
		),
		(
			"a: [3yr, 1]",
			"Invalid duration 3yr: there is no year unit, because a year is not a fixed length. Write 8760h for a fixed 365 days",
		),
		(
			"a: 3yrs",
			"Invalid duration 3yrs: there is no year unit, because a year is not a fixed length. Write 8760h for a fixed 365 days",
		),
		(
			"a: [3yrs, 1]",
			"Invalid duration 3yrs: there is no year unit, because a year is not a fixed length. Write 8760h for a fixed 365 days",
		),
		(
			"a: 1Y",
			"Invalid duration 1Y: there is no year unit, because a year is not a fixed length. Write 8760h for a fixed 365 days",
		),
		(
			"a: [1Y, 1]",
			"Invalid duration 1Y: there is no year unit, because a year is not a fixed length. Write 8760h for a fixed 365 days",
		),
		(
			"a: 1YEAR",
			"Invalid duration 1YEAR: there is no year unit, because a year is not a fixed length. Write 8760h for a fixed 365 days",
		),
		(
			"a: [1YEAR, 1]",
			"Invalid duration 1YEAR: there is no year unit, because a year is not a fixed length. Write 8760h for a fixed 365 days",
		),
		(
			"a: 1.5y",
			"Invalid duration 1.5y: there is no year unit, because a year is not a fixed length. Write 8760h for a fixed 365 days",
		),
		(
			"a: [1.5y, 1]",
			"Invalid duration 1.5y: there is no year unit, because a year is not a fixed length. Write 8760h for a fixed 365 days",
		),
		(
			"a: 1_000y",
			"Invalid duration 1_000y: there is no year unit, because a year is not a fixed length. Write 8760h for a fixed 365 days",
		),
		(
			"a: [1_000y, 1]",
			"Invalid duration 1_000y: there is no year unit, because a year is not a fixed length. Write 8760h for a fixed 365 days",
		),
		(
			"a: -1y",
			"Invalid duration -1y: there is no year unit, because a year is not a fixed length. Write 8760h for a fixed 365 days",
		),
		(
			"a: [-1y, 1]",
			"Invalid duration -1y: there is no year unit, because a year is not a fixed length. Write 8760h for a fixed 365 days",
		),
		(
			"a: 1y6m",
			"Invalid duration 1y6m: there is no year unit, because a year is not a fixed length. Write 8760h for a fixed 365 days",
		),
		(
			"a: [1y6m, 1]",
			"Invalid duration 1y6m: there is no year unit, because a year is not a fixed length. Write 8760h for a fixed 365 days",
		),
		(
			"a: 1h1y",
			"Invalid duration 1h1y: there is no year unit, because a year is not a fixed length. Write 8760h for a fixed 365 days",
		),
		(
			"a: [1h1y, 1]",
			"Invalid duration 1h1y: there is no year unit, because a year is not a fixed length. Write 8760h for a fixed 365 days",
		),
		(
			"a: 1y1y",
			"Invalid duration 1y1y: there is no year unit, because a year is not a fixed length. Write 8760h for a fixed 365 days",
		),
		(
			"a: [1y1y, 1]",
			"Invalid duration 1y1y: there is no year unit, because a year is not a fixed length. Write 8760h for a fixed 365 days",
		),
		(
			"a: 100yen",
			"Invalid number “100yen”. A string value must be quoted, as in '100yen'",
		),
		(
			"a: 1yard",
			"Invalid number “1yard”. A string value must be quoted, as in '1yard'",
		),
		(
			"a: 5yo",
			"Invalid number “5yo”. A string value must be quoted, as in '5yo'",
		),
		(
			"a: 1yrsx",
			"Invalid number “1yrsx”. A string value must be quoted, as in '1yrsx'",
		),
		(
			"a: 1yearsx",
			"Invalid number “1yearsx”. A string value must be quoted, as in '1yearsx'",
		),
		(
			"a: 1y.5",
			"Invalid number “1y.5”. A string value must be quoted, as in '1y.5'",
		),
		(
			"a: 1 y",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 10s, and anything else, such as a size, as a string, as in '1 y'",
		),
	]);
}

/**
Only a number ends with its exponent marker, so a word that ends with an “e” has no exponent.
*/
#[test]
fn only_a_number_ends_with_its_exponent_marker() {
	assert_reasons(&[
		(
			"a: 7zip-bin-name",
			"Invalid number “7zip-bin-name”. A string value must be quoted, as in '7zip-bin-name'",
		),
		(
			"a: 1byte",
			"Invalid number “1byte”. A string value must be quoted, as in '1byte'",
		),
		(
			"a: 3-phase",
			"Invalid number “3-phase”. A string value must be quoted, as in '3-phase'",
		),
		(
			"a: 1-time",
			"Invalid number “1-time”. A string value must be quoted, as in '1-time'",
		),
		(
			"a: [100-pre, 1]",
			"Invalid number “100-pre”. A string value must be quoted, as in '100-pre'",
		),
		(
			"a: 2e-e",
			"Invalid number “2e-e”. A string value must be quoted, as in '2e-e'",
		),
		("a: -verbose", "Expected a digit or “infinity” after “-”"),
		(
			"args: [-recursive]",
			"Expected a digit or “infinity” after “-”",
		),
		("a: 1e", "Expected digits after the exponent marker “e”"),
		("a: 1e-", "Expected digits after the exponent marker “e”"),
		("a: -1e", "Expected digits after the exponent marker “e”"),
		("a: 1.5e", "Expected digits after the exponent marker “e”"),
		("a: 1.5e-", "Expected digits after the exponent marker “e”"),
		(
			"a: -1_0.5_0e",
			"Expected digits after the exponent marker “e”",
		),
		("a: 0e", "Expected digits after the exponent marker “e”"),
		("a: 0.0e-", "Expected digits after the exponent marker “e”"),
	]);
}

/**
A word after “-” is only NaN or infinity when it is the whole word, as for one without the “-”.
*/
#[test]
fn a_word_after_a_minus_is_only_nan_or_infinity_when_it_is_the_whole_word() {
	assert_reasons(&[
		("a: -nano", "Expected a digit or “infinity” after “-”"),
		("a: -nanny", "Expected a digit or “infinity” after “-”"),
		("a: -info", "Expected a digit or “infinity” after “-”"),
		("a: -INFO", "Expected a digit or “infinity” after “-”"),
		(
			"args: [-inform, PEM]",
			"Expected a digit or “infinity” after “-”",
		),
		("a: -infinite", "Expected a digit or “infinity” after “-”"),
		("a: -infinityx", "Expected a digit or “infinity” after “-”"),
		(
			"a: nano",
			"Unexpected “nano”. A string value must be quoted, as in 'nano'",
		),
		(
			"a: info",
			"Unexpected “info”. A string value must be quoted, as in 'info'",
		),
		(
			"a: -nan",
			"NaN is not representable. Use null for a missing value",
		),
		(
			"a: -NaN",
			"NaN is not representable. Use null for a missing value",
		),
		(
			"a: -NAN",
			"NaN is not representable. Use null for a missing value",
		),
		(
			"a: -inf",
			"“-inf” is not a value. Negative infinity is written -infinity",
		),
		(
			"a: -Inf",
			"“-Inf” is not a value. Negative infinity is written -infinity",
		),
		(
			"a: -INF",
			"“-INF” is not a value. Negative infinity is written -infinity",
		),
		(
			"a: -Infinity",
			"“-Infinity” is not a value. Negative infinity is written -infinity",
		),
		(
			"a: -INFINITY",
			"“-INFINITY” is not a value. Negative infinity is written -infinity",
		),
		(
			"a: [-inf, 1]",
			"“-inf” is not a value. Negative infinity is written -infinity",
		),
		(
			"a: [-Infinity, 1]",
			"“-Infinity” is not a value. Negative infinity is written -infinity",
		),
	]);
}

/**
A “/” only ends a value for a suggestion when it starts a comment, so a suggestion never leaves out the text after it.
*/
#[test]
fn a_slash_only_ends_a_value_for_a_suggestion_when_it_starts_a_comment() {
	assert_reasons(&[
		(
			"speed: 100 km/h",
			"Expected a line break before the next entry, but found “k”",
		),
		(
			"a: 5 m/s",
			"Expected a line break before the next entry, but found “m”",
		),
		(
			"a: [5 m/s]",
			"Expected “,”, a line break, or “]” after an array item, but found “m”",
		),
		(
			"a: 2026-09-19T14:00:00/2026-09-20T15:00:00",
			"An instant needs an offset: Z or ±HH:MM",
		),
		(
			"a: [2026-09-19T14:00:00/P1D]",
			"An instant needs an offset: Z or ±HH:MM",
		),
		(
			"a: 2026-09-19T14:00:00 Z/x",
			"An instant needs an offset: Z or ±HH:MM. A date and time without an offset is a local time, which is not an instant, so write it as a string, as in '2026-09-19T14:00:00', or add the offset it was meant in",
		),
		(
			"a: 5 m /* c */",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 5m, and anything else, such as a size, as a string, as in '5 m'",
		),
		(
			"a: 5 m/* c */",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 5m, and anything else, such as a size, as a string, as in '5 m'",
		),
		(
			"a: 2026-09-19T14:00:00/* c */",
			"An instant needs an offset: Z or ±HH:MM. A date and time without an offset is a local time, which is not an instant, so write it as a string, as in '2026-09-19T14:00:00', or add the offset it was meant in",
		),
		(
			"a: 2026-09-19T14:00:00 Z/* c */",
			"An instant's offset follows its time directly, without a space, as in 2026-09-19T14:00:00Z",
		),
		(
			"a: 2026-09-19T14:00:00 Z /* c */",
			"An instant's offset follows its time directly, without a space, as in 2026-09-19T14:00:00Z",
		),
		(
			"timeout: 5 s // seconds",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 5s, and anything else, such as a size, as a string, as in '5 s'",
		),
		(
			"a: 5 m//c",
			"A unit cannot follow a number after a space. Write a duration without the space, as in 5m, and anything else, such as a size, as a string, as in '5 m'",
		),
		(
			"a: 2026-09-19T14:00:00 Z//c",
			"An instant's offset follows its time directly, without a space, as in 2026-09-19T14:00:00Z",
		),
	]);
}

/**
An underscore next to a letter is part of a word, not a misplaced separator in a number.
*/
#[test]
fn an_underscore_next_to_a_letter_is_part_of_a_word() {
	assert_reasons(&[
		(
			"a: 4k_video",
			"Invalid number “4k_video”. A string value must be quoted, as in '4k_video'",
		),
		(
			"a: 2x_speed",
			"Invalid number “2x_speed”. A string value must be quoted, as in '2x_speed'",
		),
		(
			"a: [5k_run, 2]",
			"Invalid number “5k_run”. A string value must be quoted, as in '5k_run'",
		),
		(
			"a: 1_",
			"An underscore in a number must be between two digits",
		),
		(
			"a: 1__0",
			"An underscore in a number must be between two digits",
		),
		(
			"a: 1_.5",
			"An underscore in a number must be between two digits",
		),
		(
			"a: 1._5",
			"An underscore in a number must be between two digits",
		),
		(
			"a: 1_e5",
			"An underscore in a number must be between two digits",
		),
		(
			"a: 1e_5",
			"An underscore in a number must be between two digits",
		),
		(
			"a: 1e5_",
			"An underscore in a number must be between two digits",
		),
		(
			"a: 1.5_",
			"An underscore in a number must be between two digits",
		),
		(
			"a: -1_",
			"An underscore in a number must be between two digits",
		),
		(
			"a: 1_000_",
			"An underscore in a number must be between two digits",
		),
		(
			"a: 1e-_5",
			"An underscore in a number must be between two digits",
		),
	]);
}

/**
An uppercase “E” is only an exponent marker where one goes, so in a word such as `5EUR` it is not.
*/
#[test]
fn an_uppercase_e_is_only_an_exponent_marker_where_one_goes() {
	assert_reasons(&[
		(
			"price: 5EUR",
			"Invalid number “5EUR”. A string value must be quoted, as in '5EUR'",
		),
		(
			"disk: 10EB",
			"Invalid number “10EB”. A string value must be quoted, as in '10EB'",
		),
		(
			"a: 1.5Ex",
			"Invalid number “1.5Ex”. A string value must be quoted, as in '1.5Ex'",
		),
		(
			"a: [3ED, 1]",
			"Invalid number “3ED”. A string value must be quoted, as in '3ED'",
		),
		("a: 1E5", "An exponent marker is a lowercase “e”"),
		("a: 1E10", "An exponent marker is a lowercase “e”"),
		("a: 1.5E-3", "An exponent marker is a lowercase “e”"),
		("a: -2E7", "An exponent marker is a lowercase “e”"),
		("a: 1_0E1_0", "An exponent marker is a lowercase “e”"),
		("a: 1E", "An exponent marker is a lowercase “e”"),
		("a: 2E-", "An exponent marker is a lowercase “e”"),
		("a: 1.5E", "An exponent marker is a lowercase “e”"),
	]);
}
