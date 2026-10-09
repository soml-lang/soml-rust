/*!
Reading documents: numbers, strings, block strings, keys, comments, whitespace, and encoding. Every rejection is checked with its exact message and position.
*/

#![allow(clippy::tabs_in_doc_comments)]

use soml::{LineColumn, Value};

/**
Reads a document that must be valid.
*/
fn read(text: &str) -> Value {
	text.parse()
		.unwrap_or_else(|error| panic!("{text:?} should read, but: {error}"))
}

/**
The value of the member `a` of a valid document.
*/
fn member(text: &str) -> Value {
	read(text)
		.get("a")
		.cloned()
		.unwrap_or_else(|| panic!("{text:?} has no member `a`"))
}

/**
The error of a document that must be invalid, with its position, as `Display` writes it.
*/
fn rejection(text: &str) -> String {
	match text.parse::<Value>() {
		Ok(value) => panic!("{text:?} should be rejected, but read as {value:?}"),
		Err(error) => error.to_string(),
	}
}

/**
The canonical form of a valid document.
*/
fn canonical(text: &str) -> String {
	soml::to_string_canonical(&read(text)).expect("a document that was read can be written")
}

fn object(members: &[(&str, Value)]) -> Value {
	members
		.iter()
		.map(|(key, value)| (*key, value.clone()))
		.collect()
}

// Ints

#[test]
fn reads_decimal_ints() {
	assert_eq!(member("a: 0"), Value::Int(0));
	assert_eq!(member("a: 42"), Value::Int(42));
	assert_eq!(member("a: -30"), Value::Int(-30));
	assert_eq!(member("a: 1_000_000"), Value::Int(1_000_000));
}

#[test]
fn reads_radix_ints() {
	assert_eq!(member("a: 0xFF"), Value::Int(255));
	assert_eq!(member("a: 0x00FF"), Value::Int(255));
	assert_eq!(member("a: 0xDEAD_BEEF"), Value::Int(0xDEAD_BEEF));
	assert_eq!(member("a: 0o644"), Value::Int(0o644));
	assert_eq!(member("a: 0b1010"), Value::Int(10));
	assert_eq!(member("a: 0b1_0"), Value::Int(2));
	assert_eq!(member("a: 0x7FFFFFFFFFFFFFFF"), Value::Int(i64::MAX));
}

#[test]
fn reads_the_int64_bounds() {
	assert_eq!(member("a: 9223372036854775807"), Value::Int(i64::MAX));
	assert_eq!(member("a: -9223372036854775808"), Value::Int(i64::MIN));
}

#[test]
fn reads_an_int_that_a_double_cannot_hold_exactly() {
	assert_eq!(
		member("a: 9007199254740993"),
		Value::Int(9_007_199_254_740_993)
	);
}

#[test]
fn rejects_an_int_outside_int64() {
	assert_eq!(
		rejection("a: 9223372036854775808"),
		"The integer 9223372036854775808 is outside the 64-bit range (-9223372036854775808 to 9223372036854775807) at line 1, column 4"
	);
	assert_eq!(
		rejection("a: -9223372036854775809"),
		"The integer -9223372036854775809 is outside the 64-bit range (-9223372036854775808 to 9223372036854775807) at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 0xFFFFFFFFFFFFFFFF"),
		"The integer 0xFFFFFFFFFFFFFFFF is outside the 64-bit range (-9223372036854775808 to 9223372036854775807) at line 1, column 4"
	);
}

#[test]
fn rejects_negative_zero_int() {
	assert_eq!(
		rejection("a: -0"),
		"“-0” is not allowed, because zero has one spelling: 0 at line 1, column 4"
	);
}

#[test]
fn rejects_leading_zeros() {
	// The hint says what the zero most likely meant: an octal number or an identifier.
	assert_eq!(
		rejection("a: 00"),
		"Leading zeros are not allowed in a decimal number. Write an octal number, such as a file mode, as 0o0, and an identifier, such as a ZIP code, as a string: '00' at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 0644"),
		"Leading zeros are not allowed in a decimal number. Write an octal number, such as a file mode, as 0o644, and an identifier, such as a ZIP code, as a string: '0644' at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 089"),
		"Leading zeros are not allowed in a decimal number. Write an identifier, such as a ZIP code, as a string: '089' at line 1, column 4"
	);

	for text in ["a: -07", "a: 0_7", "a: 01.5", "a: 01e5"] {
		assert_eq!(
			rejection(text),
			"Leading zeros are not allowed in a decimal number at line 1, column 4",
			"{text}"
		);
	}
}

#[test]
fn rejects_bad_radix_ints() {
	assert_eq!(
		rejection("a: 0xff"),
		"Hexadecimal digits are uppercase: 0xFF at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 0XFF"),
		"A number prefix is lowercase: 0x, 0o, or 0b at line 1, column 4"
	);
	assert_eq!(
		rejection("a: -0xFF"),
		"A hexadecimal integer cannot have a sign, because it states a bit pattern rather than a quantity at line 1, column 4"
	);
	assert_eq!(
		rejection("a: -0o644"),
		"An octal integer cannot have a sign, because it states a bit pattern rather than a quantity at line 1, column 4"
	);
	assert_eq!(
		rejection("a: -0b1"),
		"A binary integer cannot have a sign, because it states a bit pattern rather than a quantity at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 0x"),
		"Expected hexadecimal digits after “0x” at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 0x_FF"),
		"An underscore in a number must be between two digits at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 0b2"),
		"Invalid binary digit “2” at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 0o8"),
		"Invalid octal digit “8” at line 1, column 4"
	);
}

#[test]
fn rejects_misplaced_underscores() {
	assert_eq!(
		rejection("a: 1_"),
		"An underscore in a number must be between two digits at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 1__0"),
		"An underscore in a number must be between two digits at line 1, column 4"
	);
	// A leading underscore makes a word, not a number.
	assert_eq!(
		rejection("a: _1"),
		"Unexpected “_1”. A string value must be quoted, as in '_1' at line 1, column 4"
	);
}

#[test]
fn rejects_a_plus_sign() {
	assert_eq!(
		rejection("a: +1"),
		"A “+” sign is not allowed. A number without a sign is positive at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 1e+5"),
		"A “+” sign is not allowed in a number, including in an exponent at line 1, column 4"
	);
}

#[test]
fn an_int_and_a_float_are_different_values() {
	assert_eq!(member("a: 3"), Value::Int(3));
	assert_eq!(member("a: 3.0"), Value::Float(3.0));
	assert_ne!(member("a: 3"), member("a: 3.0"));
}

// Floats

#[test]
fn reads_floats() {
	assert_eq!(member("a: 0.5"), Value::Float(0.5));
	assert_eq!(member("a: -1.25"), Value::Float(-1.25));
	assert_eq!(member("a: 1e5"), Value::Float(100_000.0));
	assert_eq!(member("a: 1e-5"), Value::Float(0.000_01));
	assert_eq!(member("a: -1e-10"), Value::Float(-1e-10));
	assert_eq!(member("a: 1e0"), Value::Float(1.0));
	assert_eq!(member("a: 1.5e300"), Value::Float(1.5e300));
	assert_eq!(member("a: 1e1_0"), Value::Float(1e10));
	assert_eq!(member("a: 1_0.0_1e1_0"), Value::Float(10.01e10));
	assert_eq!(member("a: 0e5"), Value::Float(0.0));
}

#[test]
fn reads_infinity() {
	assert_eq!(member("a: infinity"), Value::Float(f64::INFINITY));
	assert_eq!(member("a: -infinity"), Value::Float(f64::NEG_INFINITY));
}

#[test]
fn reads_negative_zero_as_zero() {
	let Some(value) = member("a: -0.0").as_f64() else {
		panic!("-0.0 is a float");
	};

	assert_eq!(value.to_bits(), 0.0f64.to_bits());
}

#[test]
fn reads_the_float_extremes() {
	assert_eq!(member("a: 1.7976931348623157e308"), Value::Float(f64::MAX));
	assert_eq!(member("a: 5e-324"), Value::Float(f64::from_bits(1)));
	// Rounds to the smallest subnormal, which is not zero.
	assert_eq!(member("a: 3e-324"), Value::Float(f64::from_bits(1)));
}

#[test]
fn rejects_a_float_that_overflows() {
	assert_eq!(
		rejection("a: 1e999"),
		"1e999 is too large to be a finite float. Use infinity if you mean it at line 1, column 4"
	);
	assert_eq!(
		rejection("a: -1e999"),
		"-1e999 is too large to be a finite float. Use -infinity if you mean it at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 1.7976931348623159e308"),
		"1.7976931348623159e308 is too large to be a finite float. Use infinity if you mean it at line 1, column 4"
	);
}

#[test]
fn rejects_a_float_that_underflows_to_zero() {
	assert_eq!(
		rejection("a: 1e-400"),
		"1e-400 is too small to be told apart from zero. Write 0.0 if you mean zero at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 2e-324"),
		"2e-324 is too small to be told apart from zero. Write 0.0 if you mean zero at line 1, column 4"
	);
}

#[test]
fn rejects_bad_float_spellings() {
	assert_eq!(
		rejection("a: 1e05"),
		"Leading zeros are not allowed in an exponent at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 1e-0"),
		"“e-0” is not allowed, because an exponent of zero has one spelling: e0 at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 1E10"),
		"An exponent marker is a lowercase “e” at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 00.5"),
		"Leading zeros are not allowed in a decimal number at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 01e2"),
		"Leading zeros are not allowed in a decimal number at line 1, column 4"
	);
	assert_eq!(
		rejection("a: .5"),
		"A number cannot begin with “.”; write a digit before it, as in 0.5 at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 5."),
		"A decimal point must be followed by a digit at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 1.2.3"),
		"Invalid number “1.2.3”. A value with several dots, such as a version number, must be quoted at line 1, column 4"
	);
}

#[test]
fn rejects_other_spellings_of_infinity_and_nan() {
	assert_eq!(
		rejection("a: inf"),
		"“inf” is not a value. Infinity is written infinity, in lowercase at line 1, column 4"
	);
	assert_eq!(
		rejection("a: Infinity"),
		"“Infinity” is not a value. Infinity is written infinity, in lowercase at line 1, column 4"
	);
	assert_eq!(
		rejection("a: nan"),
		"NaN is not representable. Use null for a missing value at line 1, column 4"
	);
}

#[test]
fn rounds_a_long_halfway_literal_to_even() {
	// 2^53 + 1 is halfway between two doubles, and the 800 zeros keep it exactly halfway, so it rounds to the even one, 2^53.
	let text = format!("a: 9007199254740993.{}", "0".repeat(800));
	let Some(value) = member(&text).as_f64() else {
		panic!("the literal is a float");
	};

	assert_eq!(value.to_bits(), 9_007_199_254_740_992.0f64.to_bits());
}

#[test]
fn rounds_a_long_literal_just_above_halfway_up() {
	// A nonzero digit after 800 zeros puts the literal above halfway, so it rounds up.
	let text = format!("a: 9007199254740993.{}1", "0".repeat(800));
	let Some(value) = member(&text).as_f64() else {
		panic!("the literal is a float");
	};

	assert_eq!(value.to_bits(), 9_007_199_254_740_994.0f64.to_bits());
}

// Strings

#[test]
fn a_literal_string_has_no_escapes() {
	assert_eq!(member(r"a: 'C:\Users\n'"), Value::from(r"C:\Users\n"));
	assert_eq!(member(r#"a: 'say "hi"'"#), Value::from(r#"say "hi""#));
	assert_eq!(member("a: ''"), Value::from(""));
	assert_eq!(member("a: 'a\tb'"), Value::from("a\tb"));
}

#[test]
fn an_escaped_string_decodes_every_escape() {
	assert_eq!(
		member(r#"a: "\\ \" \n \t \u{0} \u{e9} \u{1f600} \u{10ffff}""#),
		Value::from("\\ \" \n \t \u{0} \u{e9} \u{1f600} \u{10ffff}")
	);
	assert_eq!(member(r#"a: "it's""#), Value::from("it's"));
}

#[test]
fn a_string_can_hold_invisible_characters_and_noncharacters() {
	assert_eq!(
		member("a: '\u{2028}\u{200B}\u{A0}'"),
		Value::from("\u{2028}\u{200B}\u{A0}")
	);
	assert_eq!(
		member("a: '\u{FFFF}\u{FDD0}'"),
		Value::from("\u{FFFF}\u{FDD0}")
	);
	assert_eq!(member(r#"a: "\u{fffe}""#), Value::from("\u{FFFE}"));
}

#[test]
fn rejects_bad_escapes() {
	assert_eq!(
		rejection(r#"a: "\q""#),
		r#"Unknown escape “\q”. The escapes are \\, \", \n, \t, and \u{…}; use a '...' string for literal backslashes at line 1, column 5"#
	);
	assert_eq!(
		rejection(r#"a: "C:\Users""#),
		r#"Unknown escape “\U”. The escapes are \\, \", \n, \t, and \u{…}; use a '...' string for literal backslashes at line 1, column 7"#
	);
	assert_eq!(
		rejection(r#"a: "\r""#),
		r"There is no \r escape, because a carriage return cannot be represented at line 1, column 5"
	);
	assert_eq!(
		rejection(r#"a: "\'""#),
		r#"A ' needs no escape inside "..." at line 1, column 5"#
	);
}

#[test]
fn rejects_bad_unicode_escapes() {
	assert_eq!(
		rejection(r#"a: "\u{E9}""#),
		"A Unicode escape uses lowercase hexadecimal digits at line 1, column 5"
	);
	assert_eq!(
		rejection(r#"a: "\u{1F600}""#),
		"A Unicode escape uses lowercase hexadecimal digits at line 1, column 5"
	);
	assert_eq!(
		rejection(r#"a: "\u{0041}""#),
		r"A Unicode escape may not have leading zeros; write \u{41} at line 1, column 5"
	);
	assert_eq!(
		rejection(r#"a: "\u0041""#),
		r"The four-digit \u0041 form is not an escape. Write \u{41} at line 1, column 5"
	);
	assert_eq!(
		rejection(r#"a: "\u{d800}""#),
		r"\u{d800} is a surrogate, which is not a Unicode scalar value at line 1, column 5"
	);
	assert_eq!(
		rejection(r#"a: "\u{dfff}""#),
		r"\u{dfff} is a surrogate, which is not a Unicode scalar value at line 1, column 5"
	);
	assert_eq!(
		rejection(r#"a: "\u{d}""#),
		r"A carriage return (U+000D) cannot be represented, so \u{d} is not allowed at line 1, column 5"
	);
	assert_eq!(
		rejection(r#"a: "\u{110000}""#),
		r"\u{110000} is above U+10FFFF, the largest Unicode scalar value at line 1, column 5"
	);
	assert_eq!(
		rejection(r#"a: "\u{1234567}""#),
		"A Unicode escape has at most six hexadecimal digits at line 1, column 5"
	);
	assert_eq!(
		rejection(r#"a: "\u{}""#),
		"A Unicode escape needs one to six hexadecimal digits at line 1, column 5"
	);
}

#[test]
fn a_single_line_string_cannot_span_lines() {
	assert_eq!(
		rejection("a: 'a\nb'"),
		"Unterminated string. A '...' string must end on the line it starts on; use a block string (''') for multiple lines at line 1, column 4"
	);
	assert_eq!(
		rejection("a: \"a\nb\""),
		"Unterminated string. A \"...\" string must end on the line it starts on; use a block string (\"\"\") for multiple lines at line 1, column 4"
	);
}

#[test]
fn a_literal_string_ends_at_the_next_quote() {
	assert_eq!(
		rejection("a: 'it''s'"),
		"There is no '' escape in a '...' string. Write a string that contains ' as \"...\", as in \"it's\" at line 1, column 8"
	);
}

// Block strings

#[test]
fn a_block_string_is_dedented_by_its_closing_delimiter() {
	assert_eq!(member("a: '''\n\tx\n\t'''"), Value::from("x"));
	assert_eq!(
		member("a: '''\n\t\tx\n\t\t\ty\n\t\t'''"),
		Value::from("x\n\ty")
	);
	assert_eq!(
		member("a: '''\n    one\n      two\n    '''"),
		Value::from("one\n  two")
	);
	assert_eq!(member("a: '''\nx\n'''"), Value::from("x"));
}

#[test]
fn a_block_string_drops_blank_lines_at_its_edges_and_keeps_inner_ones() {
	// A blank line holds only spaces and tabs, needs no indentation, and becomes an empty line.
	assert_eq!(
		member("a: '''\n\n  x\n  \n\n \t \n  y\n\n  '''"),
		Value::from("x\n\n\n\ny")
	);
}

#[test]
fn an_empty_block_string_is_empty() {
	assert_eq!(member("a: '''\n'''"), Value::from(""));
	assert_eq!(member("a: '''\n\n\t\n'''"), Value::from(""));
}

#[test]
fn a_literal_block_has_no_escapes() {
	assert_eq!(member("a: '''\n\\n\\t\n'''"), Value::from("\\n\\t"));
}

#[test]
fn an_escaped_block_decodes_escapes() {
	assert_eq!(
		member("a: \"\"\"\n\t\\tx\\n\\u{e9}\n\t\"\"\""),
		Value::from("\tx\n\u{e9}")
	);
}

#[test]
fn a_longer_delimiter_holds_a_run_of_the_shorter_one() {
	assert_eq!(member("a: ''''\n'''\n''''"), Value::from("'''"));
	assert_eq!(member("a: '''\n\"\"\"\n'''"), Value::from("\"\"\""));
	assert_eq!(member("a: '''\n''''\n'''"), Value::from("''''"));
}

#[test]
fn a_block_string_can_be_followed_by_a_comma_a_bracket_or_a_comment() {
	assert_eq!(
		member("a: ['''\n  x\n  ''',]"),
		Value::Array(vec![Value::from("x")])
	);
	assert_eq!(
		member("a: ['''\n  x\n  ''']"),
		Value::Array(vec![Value::from("x")])
	);
	assert_eq!(member("a: '''\nx\n''' # c"), Value::from("x"));
	assert_eq!(
		member("a: {b: '''\n  x\n  '''}"),
		object(&[("b", Value::from("x"))])
	);
}

#[test]
fn rejects_content_after_the_opening_delimiter() {
	assert_eq!(
		rejection("a: '''x\n'''"),
		"A block string's opening delimiter must be followed directly by a line break, and its content starts on the next line at line 1, column 7"
	);
	assert_eq!(
		rejection("a: ''' \nx\n'''"),
		"A block string's opening delimiter must be followed directly by a line break, and its content starts on the next line at line 1, column 7"
	);
	assert_eq!(
		rejection("a: ''' # c\nx\n'''"),
		"A block string's opening delimiter must be followed directly by a line break, and its content starts on the next line at line 1, column 7"
	);
}

#[test]
fn rejects_a_line_indented_less_than_the_closing_delimiter() {
	assert_eq!(
		rejection("a: '''\n x\n  '''"),
		"This line does not start with the indentation of its block string's closing delimiter. Every line except a blank one must start with exactly the same spaces and tabs at line 2, column 1"
	);
}

#[test]
fn rejects_tabs_where_the_closing_delimiter_has_spaces() {
	assert_eq!(
		rejection("a: '''\n  x\n\ty\n  '''"),
		"This line does not start with the indentation of its block string's closing delimiter. Every line except a blank one must start with exactly the same spaces and tabs at line 3, column 1"
	);
}

#[test]
fn rejects_an_unterminated_block_string_at_its_opening() {
	assert_eq!(
		rejection("a: '''\nx"),
		"Unterminated block string at line 1, column 4"
	);
}

#[test]
fn reports_an_escape_error_in_a_block_at_the_escape() {
	assert_eq!(
		rejection("a: \"\"\"\n  x\\q\n  \"\"\""),
		r#"Unknown escape “\q”. The escapes are \\, \", \n, \t, and \u{…}; use a '...' string for literal backslashes at line 2, column 4"#
	);
}

#[test]
fn rejects_a_block_string_as_a_key() {
	assert_eq!(
		rejection("'''\nx\n''': 1"),
		"A block string cannot be a key at line 1, column 1"
	);
}

// Keys

#[test]
fn a_bare_key_is_never_coerced() {
	for key in [
		"404",
		"2024-01-01",
		"-foo",
		"_private",
		"7zip-bin",
		"content-type",
		"true",
		"false",
		"null",
		"infinity",
		"nan",
		"0x10",
		"1h30m",
	] {
		let text = format!("{key}: 1");
		assert_eq!(read(&text), object(&[(key, Value::Int(1))]), "{text}");
	}
}

#[test]
fn keywords_are_keys_inside_braces() {
	assert_eq!(
		read("{true: 1, null: 2}"),
		object(&[("true", Value::Int(1)), ("null", Value::Int(2))])
	);
}

#[test]
fn a_quoted_key_holds_anything_a_string_holds() {
	assert_eq!(read("'a.b': 1"), object(&[("a.b", Value::Int(1))]));
	assert_eq!(
		read("'the name': 1"),
		object(&[("the name", Value::Int(1))])
	);
	assert_eq!(read(r#""a\nb": 1"#), object(&[("a\nb", Value::Int(1))]));
	assert_eq!(read("'': 1"), object(&[("", Value::Int(1))]));
	assert_eq!(read("'😀': 1"), object(&[("😀", Value::Int(1))]));
}

#[test]
fn duplicate_keys_are_decided_on_the_decoded_value() {
	assert_eq!(
		rejection("a: 1\na: 2"),
		"Duplicate key a at line 2, column 1"
	);
	assert_eq!(
		rejection("a: 1\n'a': 2"),
		"Duplicate key a at line 2, column 1"
	);
	assert_eq!(
		rejection("a: 1\n\"a\": 2"),
		"Duplicate key a at line 2, column 1"
	);
	assert_eq!(
		rejection("a: 1\n\"\\u{61}\": 2"),
		"Duplicate key a at line 2, column 1"
	);
	assert_eq!(
		rejection("{a: 1, 'a': 2}"),
		"Duplicate key a at line 1, column 8"
	);
}

#[test]
fn keys_are_not_normalized() {
	// A composed é and an e with a combining accent are two keys.
	assert_eq!(
		read("'\u{E9}': 1\n'e\u{301}': 2")
			.as_object()
			.map(|object| object.len()),
		Some(2)
	);
	assert_eq!(
		read("a: 1\nA: 2\na-b: 3\na_b: 4")
			.as_object()
			.map(|object| object.len()),
		Some(4)
	);
}

#[test]
fn rejects_whitespace_between_a_key_and_its_colon() {
	assert_eq!(
		rejection("a :1"),
		"Whitespace is not allowed between a key and its “:” at line 1, column 2"
	);
}

#[test]
fn rejects_a_comment_between_a_key_and_its_colon() {
	assert_eq!(
		rejection("a/*c*/: 1"),
		"A comment is not allowed between a key and its “:” at line 1, column 2"
	);
	assert_eq!(
		rejection("'a' /* c */ : 1"),
		"A comment is not allowed between a key and its “:” at line 1, column 5"
	);
	// Without a “:” after the comment, the comment is not the mistake.
	assert_eq!(
		rejection("a /* c */ /* d */: 1"),
		"Expected “:” after the key, but found “/” at line 1, column 3"
	);
}

#[test]
fn rejects_a_bare_key_with_a_space() {
	assert_eq!(
		rejection("a b: 1"),
		"A bare key cannot contain spaces. Quote it, as in 'a b' at line 1, column 1"
	);
}

#[test]
fn rejects_a_bare_key_with_other_characters() {
	assert_eq!(
		rejection("a@c: 1"),
		"Expected “:” after the key, but found “@”. A key that contains characters other than letters, digits, “_”, and “-” must be quoted at line 1, column 2"
	);
	// No hint for a character that has a meaning of its own, for whitespace, or after a quoted key.
	for (text, found) in [
		("'a'x: 1", "“x”"),
		("a#c: 1", "“#”"),
		("{a}", "“}”"),
		("[{a]", "“]”"),
		("a}", "“}”"),
		("'it''s': 1", "“'”"),
		("a\u{a0}: 1", "U+00A0"),
	] {
		let message = rejection(text);
		assert!(
			message.starts_with("Expected “:” after the key, but found ")
				&& message.contains(found)
				&& !message.contains("must be quoted"),
			"{text:?}: {message}"
		);
	}
}

#[test]
fn a_key_that_starts_with_another_character_gets_a_quoting_hint() {
	let quote_the_key =
		"A key that contains characters other than letters, digits, “_”, and “-” must be quoted";

	assert_eq!(
		rejection("$schema: 1"),
		format!(
			"Expected a key, but found “$”. {quote_the_key}, as in '$schema' at line 1, column 1"
		)
	);
	assert_eq!(
		rejection("{é: 1}"),
		format!("Expected a key, but found “é”. {quote_the_key}, as in 'é' at line 1, column 2")
	);
	assert_eq!(
		rejection("$a.b: 1"),
		format!("Expected a key, but found “$”. {quote_the_key}, as in '$a.b' at line 1, column 1")
	);
	// No hint for an invisible character, or for one with a meaning of its own.
	assert_eq!(
		rejection("\u{200B}a: 1"),
		"Expected a key, but found U+200B (ZERO WIDTH SPACE; only space, tab, and line feed are whitespace) at line 1, column 1"
	);
	assert_eq!(
		rejection("a\u{200B}: 1"),
		"Expected “:” after the key, but found U+200B (ZERO WIDTH SPACE; only space, tab, and line feed are whitespace) at line 1, column 2"
	);
	assert_eq!(
		rejection("{,}"),
		"Expected a key, but found “,” at line 1, column 2"
	);
}

#[test]
fn allows_no_space_after_the_colon_and_a_line_break() {
	assert_eq!(read("a:1"), object(&[("a", Value::Int(1))]));
	assert_eq!(read("a:\n\n1"), object(&[("a", Value::Int(1))]));
}

// Dots in keys

#[test]
fn a_quoted_dot_is_part_of_the_key() {
	assert_eq!(
		read("'a.b': 1\na: {b: 2}"),
		object(&[
			("a.b", Value::Int(1)),
			("a", object(&[("b", Value::Int(2))]))
		])
	);
}

#[test]
fn rejects_a_dot_after_a_key() {
	assert_eq!(
		rejection("a: 1\nb.c: 2"),
		"A bare key cannot contain “.”. Quote it, as in 'b.c', or use braces to nest, as in b: {c: …} at line 2, column 2"
	);
	assert_eq!(
		rejection("a. b: 1"),
		"A key cannot contain “.” unless it is quoted. Quote the whole key, or use braces to nest, as in a: {b: …} at line 1, column 2"
	);
	assert_eq!(
		rejection("a .b: 1"),
		"Expected “:” after the key, but found “.” at line 1, column 3"
	);
}

#[test]
fn a_collision_inside_braces_points_at_the_second_key() {
	assert_eq!(
		rejection("{a: 1,\n  a: 2}"),
		"Duplicate key a at line 2, column 3"
	);
}

// Documents

#[test]
fn reads_the_three_document_forms() {
	assert_eq!(read("a: 1"), object(&[("a", Value::Int(1))]));
	assert_eq!(read("{a: 1}"), object(&[("a", Value::Int(1))]));
	assert_eq!(
		read("[1, 2]"),
		Value::Array(vec![Value::Int(1), Value::Int(2)])
	);
	assert_eq!(read("{}"), Value::Object(soml::Object::new()));
	assert_eq!(read("[]"), Value::Array(Vec::new()));
}

#[test]
fn rejects_an_empty_document() {
	assert_eq!(
		rejection(""),
		"A document must contain an object or an array, but this one is empty at line 1, column 1"
	);
	assert_eq!(
		rejection("  \n\t\n"),
		"A document must contain an object or an array, but this one is empty at line 3, column 1"
	);
	assert_eq!(
		rejection("# only\n/* a comment */"),
		"A document must contain an object or an array, but this one is empty at line 2, column 16"
	);
}

#[test]
fn rejects_a_bare_scalar_document() {
	for text in [
		"5",
		"true",
		"null",
		"'x'",
		"2026-09-19T14:00:00Z",
		"1h",
		"infinity",
	] {
		assert_eq!(
			rejection(text),
			"A bare value is not a document. A document is an object or an array, so write it as `key: value` or `[value]` at line 1, column 1",
			"{text}"
		);
	}
}

#[test]
fn rejects_anything_after_the_top_level_collection() {
	assert_eq!(
		rejection("[1] [2]"),
		"Unexpected “[” after the end of the document at line 1, column 5"
	);
	assert_eq!(
		rejection("{a: 1}\nb: 2"),
		"Unexpected “b” after the end of the document at line 2, column 1"
	);
}

#[test]
fn rejects_commas_between_top_level_entries() {
	assert_eq!(
		rejection("a: 1, b: 2"),
		"Top-level entries are separated by line breaks, not commas. Use braces for a one-line object at line 1, column 5"
	);
	assert_eq!(
		rejection("a: 1,\nb: 2"),
		"Top-level entries are separated by line breaks, not commas. Use braces for a one-line object at line 1, column 5"
	);
}

#[test]
fn rejects_two_top_level_entries_on_one_line() {
	assert_eq!(
		rejection("a: 1 b: 2"),
		"Expected a line break before the next entry, but found “b” at line 1, column 6"
	);
}

#[test]
fn allows_trailing_commas_inside_brackets() {
	assert_eq!(read("[1, 2,]"), read("[1, 2]"));
	assert_eq!(read("{a: 1,}"), read("{a: 1}"));
}

#[test]
fn rejects_bad_commas_inside_brackets() {
	assert_eq!(
		rejection("a: [,]"),
		"Expected a value, but found “,” at line 1, column 5"
	);
	assert_eq!(
		rejection("a: [1,,]"),
		"Expected a value, but found “,” at line 1, column 7"
	);
	assert_eq!(
		rejection("a: [,1]"),
		"Expected a value, but found “,” at line 1, column 5"
	);
	assert_eq!(
		rejection("a: [1,,2]"),
		"Expected a value, but found “,” at line 1, column 7"
	);
	// Two items on one line need a comma.
	assert_eq!(
		rejection("a: [1 2]"),
		"Expected “,”, a line break, or “]” after an array item, but found “2” at line 1, column 7"
	);
	assert_eq!(
		rejection("a: {b: 1 c: 2}"),
		"Expected “,”, a line break, or “}” after an object member, but found “c” at line 1, column 10"
	);
	// A line break inside a block comment does not separate items, and the message says so.
	assert_eq!(
		rejection("a: [1 /*\n*/ 2]"),
		"Expected “,”, a line break, or “]” after an array item, but found “2”. A line break inside a block comment does not separate items at line 2, column 4"
	);
}

#[test]
fn a_line_break_separates_items_inside_brackets() {
	let ports = Value::Array(vec![Value::Int(80), Value::Int(443)]);
	assert_eq!(member("a: [\n80\n443\n]"), ports);
	assert_eq!(
		member("a: {\nb: 1\nc: 2\n}"),
		object(&[("b", Value::Int(1)), ("c", Value::Int(2))])
	);

	// Commas and line breaks mix, and a comma with line breaks after it is one separator.
	let three = Value::Array(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
	assert_eq!(member("a: [1, 2\n3]"), three);
	assert_eq!(member("a: [1,\n2,\n\n3]"), three);

	// A trailing comma is allowed, on the line of the last item.
	for text in ["a: [1,]", "a: [1,\n]", "a: [1 /* a\n*/,]"] {
		assert_eq!(member(text), Value::Array(vec![Value::Int(1)]), "{text:?}");
	}

	// A comma goes on the line of the item before it.
	for text in [
		"a: [1\n, 2]",
		"a: [1\n,]",
		"a: {b: 1\n, c: 2}",
		"a: [1 # c\n, 2]",
	] {
		assert_eq!(
			rejection(text),
			"A comma must be on the same line as the item before it. The line break already separates the items, so remove the comma at line 2, column 1",
			"{text:?}"
		);
	}

	// A `#` comment ends at its line break, which separates the items.
	assert_eq!(member("a: [80 # web\n443]"), ports);

	// A line break after a `:` is still whitespace before the value.
	assert_eq!(member("a: {b:\n1}"), object(&[("b", Value::Int(1))]));
}

#[test]
fn rejects_unterminated_collections_at_their_opening() {
	assert_eq!(
		rejection("a: [1,\n2"),
		"Unterminated array: expected “]” at line 1, column 4"
	);
	assert_eq!(
		rejection("a: {b: 1"),
		"Unterminated object: expected “}” at line 1, column 4"
	);
}

#[test]
fn rejects_a_value_that_is_left_out() {
	assert_eq!(
		rejection("a:\nb: 1"),
		"Expected a value, but found the key b at line 2, column 1"
	);
	assert_eq!(
		rejection("a:\n'b c': 1"),
		"Expected a value, but found the key \"b c\" at line 2, column 1"
	);
	assert_eq!(
		rejection("{a:\ntrue: 1}"),
		"Expected a value, but found the key true at line 2, column 1"
	);
	assert_eq!(
		rejection("a:\n2026-09-19T25:00:00Z"),
		"Invalid instant 2026-09-19T25:00:00Z: the hour must be 00 to 23 at line 2, column 1"
	);
	assert_eq!(
		rejection("a:"),
		"Expected a value, but reached the end of the document at line 1, column 3"
	);
}

#[test]
fn rejects_an_unquoted_string_value() {
	assert_eq!(
		rejection("a: api-gateway"),
		"Unexpected “api-gateway”. A string value must be quoted, as in 'api-gateway' at line 1, column 4"
	);
	assert_eq!(
		rejection("a: yes"),
		"“yes” is not a value. Booleans are written true and false, in lowercase at line 1, column 4"
	);
	assert_eq!(
		rejection("a: NULL"),
		"“NULL” is not a value. Null is written null, in lowercase at line 1, column 4"
	);
}

#[test]
fn a_keyword_must_be_a_whole_word() {
	assert_eq!(
		rejection("a: nullable"),
		"Unexpected “nullable”. A string value must be quoted, as in 'nullable' at line 1, column 4"
	);
}

#[test]
fn reads_mixed_arrays() {
	assert_eq!(
		member("a: [1, 'two', true, null, 1.5, [], {}]"),
		Value::Array(vec![
			Value::Int(1),
			Value::from("two"),
			Value::Bool(true),
			Value::Null,
			Value::Float(1.5),
			Value::Array(Vec::new()),
			Value::Object(soml::Object::new()),
		])
	);
}

// Instants and durations

#[test]
fn reads_an_instant_as_utc() {
	assert_eq!(
		canonical("a: 2026-09-19T21:00:00.5+07:00"),
		"a: 2026-09-19T14:00:00.5Z\n"
	);
	assert_eq!(
		read("a: 2026-09-19T21:00:00+07:00"),
		read("a: 2026-09-19T14:00:00Z")
	);
}

#[test]
fn reads_the_instant_range() {
	assert_eq!(
		member("a: 0001-01-01T00:00:00Z"),
		Value::Instant(soml::Instant::MIN)
	);
	assert_eq!(
		member("a: 9999-12-31T23:59:59.999999999Z"),
		Value::Instant(soml::Instant::MAX)
	);
	assert_eq!(
		member("a: 2024-02-29T00:00:00Z")
			.as_instant()
			.map(soml::Instant::unix_seconds),
		Some(1_709_164_800)
	);
}

#[test]
fn rejects_bad_instants() {
	assert_eq!(
		rejection("a: 2026-09-19"),
		"2026-09-19 is a date, not an instant. Write a date as a string, as in '2026-09-19'. An instant needs a time and an offset, as in 2026-09-19T00:00:00Z at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 2026-09-19t14:00:00Z"),
		"The date and time separator in an instant is an uppercase “T” at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 2026-09-19T14:00:00z"),
		"The UTC offset in an instant is an uppercase “Z” at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 2026-09-19T14:00:00+0700"),
		"An instant's offset is written with a colon, as in +07:00 at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 2026-09-19T14:00:00"),
		"An instant needs an offset: Z or ±HH:MM. A date and time without an offset is a local time, which is not an instant, so write it as a string, as in '2026-09-19T14:00:00', or add the offset it was meant in at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 2026-02-29T00:00:00Z"),
		"Invalid instant 2026-02-29T00:00:00Z: the day must be 01 to 28 in that month at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 2026-09-19T14:00:60Z"),
		"Invalid instant 2026-09-19T14:00:60Z: the second must be 00 to 59, and a leap second is not representable at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 2026-09-19T14:00:00-00:00"),
		"Invalid instant 2026-09-19T14:00:00-00:00: -00:00 means “offset unknown” in RFC 3339, which is not representable; use Z or +00:00 at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 2026-09-19T14:00:00.1234567890Z"),
		"Invalid instant 2026-09-19T14:00:00.1234567890Z: a fractional second has at most nine digits at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 0000-01-01T00:00:00Z"),
		"Invalid instant 0000-01-01T00:00:00Z: the year must be 0001 to 9999 at line 1, column 4"
	);
}

#[test]
fn rejects_an_instant_outside_the_range_in_utc() {
	assert_eq!(
		rejection("a: 0001-01-01T00:00:00+00:01"),
		"Invalid instant 0001-01-01T00:00:00+00:01: in UTC it falls outside the years 0001 to 9999 at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 9999-12-31T23:59:59-00:01"),
		"Invalid instant 9999-12-31T23:59:59-00:01: in UTC it falls outside the years 0001 to 9999 at line 1, column 4"
	);
}

#[test]
fn reads_durations() {
	let nanoseconds = |text: &str| member(text).as_duration().map(soml::Duration::nanoseconds);

	assert_eq!(nanoseconds("a: 30s"), Some(30_000_000_000));
	assert_eq!(nanoseconds("a: 90m"), nanoseconds("a: 1h30m"));
	assert_eq!(nanoseconds("a: 1.5h"), nanoseconds("a: 1h30m"));
	assert_eq!(nanoseconds("a: -5m"), Some(-300_000_000_000));
	assert_eq!(nanoseconds("a: 1h1.5m"), Some(3_690_000_000_000));
	assert_eq!(nanoseconds("a: 0.0000000001h"), Some(360));
	assert_eq!(nanoseconds("a: 1_000s"), Some(1_000_000_000_000));
	assert_eq!(nanoseconds("a: 0s"), Some(0));
	assert_eq!(nanoseconds("a: 9223372036854775807ns"), Some(i64::MAX));
	assert_eq!(nanoseconds("a: -9223372036854775808ns"), Some(i64::MIN));
	assert_eq!(nanoseconds("a: 1h2m3s4ms5us6ns"), Some(3_723_004_005_006));
}

#[test]
fn rejects_bad_durations() {
	assert_eq!(
		rejection("a: -0s"),
		"Invalid duration -0s: “-” is not allowed before zero, because zero has one spelling: 0s at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 30m1h"),
		"Invalid duration 30m1h: the units are in the order h, m, s, ms, us, ns, and each appears at most once at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 1m1m"),
		"Invalid duration 1m1m: the units are in the order h, m, s, ms, us, ns, and each appears at most once at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 1.5h30m"),
		"Invalid duration 1.5h30m: only the last part may have a fraction at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 0.5ns"),
		"Invalid duration 0.5ns: it is not a whole number of nanoseconds at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 1.0000000001s"),
		"Invalid duration 1.0000000001s: it is not a whole number of nanoseconds at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 9223372036854775808ns"),
		"Invalid duration 9223372036854775808ns: it is outside the 64-bit range of nanoseconds, about 292 years either way at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 1d"),
		"Invalid duration 1d: there is no day unit, because a day is not a fixed length. Write 24h for a fixed 24 hours at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 1H"),
		"Invalid duration 1H: the units are lowercase: h at line 1, column 4"
	);
	// `M` is not read as minutes, which `512m` would be, because a number with `M` alone is more often a size.
	assert_eq!(
		rejection("memory: 512M"),
		"Invalid duration 512M: there is no month unit, because a month is not a fixed length. A size, such as 512M, is a string: '512M' at line 1, column 9"
	);
	assert_eq!(
		rejection("a: 1h5M"),
		"Invalid duration 1h5M: there is no month unit, because a month is not a fixed length at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 01s"),
		"Invalid duration 01s: leading zeros are not allowed at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 1h-5m"),
		"Invalid duration 1h-5m: only the whole duration takes a sign, as in -1h30m at line 1, column 4"
	);
	assert_eq!(
		rejection("a: 1.s"),
		"Invalid duration 1.s: a “.” must be followed by a digit at line 1, column 4"
	);
}

// Comments

#[test]
fn comments_are_not_content() {
	let expected = read("a: 1\nb: [2, 3]\nc: {}");

	assert_eq!(
		read("# head\na: 1 # end of line\nb: [2, # item\n3]\nc: {/* empty */}\n# tail"),
		expected
	);
	assert_eq!(
		read(
			"/* a\nregion */\na: /* before */ 1/* after */\nb: [/* first */ 2, 3 /* last */]\nc: {# c\n}"
		),
		expected
	);
	assert_eq!(read("a: 1#c\nb: [2, 3]\nc: {}"), expected);
}

#[test]
fn comments_may_appear_inside_empty_collections() {
	assert_eq!(read("[/* c */]"), Value::Array(Vec::new()));
	assert_eq!(read("{# c\n}"), Value::Object(soml::Object::new()));
}

#[test]
fn a_line_break_inside_a_block_comment_does_not_separate_entries() {
	assert_eq!(
		rejection("a: 1 /* x\n */ b: 2"),
		"Expected a line break before the next entry, but found “b” at line 2, column 5"
	);
	assert_eq!(read("a: 1 /* x */\nb: 2"), read("a: 1\nb: 2"));
}

#[test]
fn rejects_a_nested_block_comment_at_the_inner_opening() {
	assert_eq!(
		rejection("a: 1 /* a /* b */"),
		"Block comments cannot be nested, and their body may not contain “/*” at line 1, column 11"
	);
}

#[test]
fn a_slash_star_that_overlaps_the_closing_is_not_nested() {
	assert_eq!(read("a: 1 /*/ x */"), read("a: 1"));
}

#[test]
fn rejects_an_unterminated_block_comment_at_its_opening() {
	assert_eq!(
		rejection("a: 1 /* unterminated"),
		"Unterminated block comment at line 1, column 6"
	);
}

#[test]
fn comment_markers_inside_strings_are_content() {
	assert_eq!(
		member("a: '# not a comment /* */'"),
		Value::from("# not a comment /* */")
	);
}

#[test]
fn rejects_a_removed_slash_dash() {
	assert_eq!(
		rejection("/-a: 1\nb: 2"),
		"Expected a key, but found “/” at line 1, column 1"
	);
	assert_eq!(
		rejection("a: [1, /-2]"),
		"Expected a value, but found “/” at line 1, column 8"
	);
}

// Whitespace and encoding

#[test]
fn indentation_does_not_change_the_structure() {
	assert_eq!(read("\ta:  1\n    b:\t2"), read("a: 1\nb: 2"));
}

#[test]
fn rejects_a_bom() {
	assert_eq!(
		rejection("\u{FEFF}a: 1"),
		"A byte order mark (BOM) is not allowed at line 1, column 1"
	);
}

#[test]
fn rejects_u_feff_between_tokens() {
	assert_eq!(
		rejection("a: 1\n\u{FEFF}b: 2"),
		"Expected a key, but found U+FEFF (ZERO WIDTH NO-BREAK SPACE; only space, tab, and line feed are whitespace) at line 2, column 1"
	);
	assert_eq!(
		rejection("a: \u{FEFF}1"),
		"Expected a value, but found U+FEFF (ZERO WIDTH NO-BREAK SPACE; only space, tab, and line feed are whitespace) at line 1, column 4"
	);
}

#[test]
fn u_feff_inside_a_string_or_a_comment_is_content() {
	assert_eq!(member("a: '\u{FEFF}'"), Value::from("\u{FEFF}"));
	assert_eq!(read("a: 1 # \u{FEFF}\n/* \u{FEFF} */"), read("a: 1"));
	// A comment may hold any character but the refused control characters, including the Unicode whitespace that is an error between tokens.
	assert_eq!(
		read("a: 1 # \u{A0}\u{2028}\u{3000}\u{85}\n/* \u{2028}\u{A0} */"),
		read("a: 1")
	);
	assert_eq!(
		read("'\u{FEFF}': 1"),
		object(&[("\u{FEFF}", Value::Int(1))])
	);
}

#[test]
fn rejects_a_carriage_return_anywhere() {
	assert_eq!(
		rejection("a: 1\r\nb: 2"),
		"A carriage return (U+000D) is not allowed anywhere. Use LF line endings at line 1, column 5"
	);
	assert_eq!(
		rejection("a: 'x\ry'"),
		"A carriage return (U+000D) is not allowed anywhere. Use LF line endings at line 1, column 6"
	);
	assert_eq!(
		rejection("a: 1 # \r"),
		"A carriage return (U+000D) is not allowed anywhere. Use LF line endings at line 1, column 8"
	);
}

#[test]
fn rejects_raw_control_characters_even_in_strings_and_comments() {
	assert_eq!(
		rejection("a: 'x\u{0}'"),
		"A raw control character (U+0000) is not allowed anywhere, including in strings and comments. In a string, write it as the escape \\u{0} inside \"...\" at line 1, column 6"
	);
	assert_eq!(
		rejection("a: 1 # \u{7}"),
		"A raw control character (U+0007) is not allowed anywhere, including in strings and comments. In a string, write it as the escape \\u{7} inside \"...\" at line 1, column 8"
	);
	assert_eq!(
		rejection("a: '\u{7F}'"),
		"A raw control character (U+007F) is not allowed anywhere, including in strings and comments. In a string, write it as the escape \\u{7f} inside \"...\" at line 1, column 5"
	);
	assert_eq!(
		rejection("a: '''\n\u{1B}\n'''"),
		"A raw control character (U+001B) is not allowed anywhere, including in strings and comments. In a string, write it as the escape \\u{1b} inside \"...\" at line 2, column 1"
	);
}

#[test]
fn every_control_character_but_cr_is_representable_as_an_escape() {
	for code in (0u32..0x20).chain([0x7F]).filter(|code| *code != 0x0D) {
		let character = char::from_u32(code).expect("a control character is a scalar value");
		let text = format!("a: \"\\u{{{code:x}}}\"");
		assert_eq!(
			member(&text),
			Value::String(character.to_string()),
			"{text}"
		);
	}
}

#[test]
fn rejects_other_unicode_whitespace_between_tokens() {
	assert_eq!(
		rejection("a:\u{A0}1"),
		"Expected a value, but found U+00A0 (NO-BREAK SPACE; only space, tab, and line feed are whitespace) at line 1, column 3"
	);
	assert_eq!(
		rejection("a: 1\u{2028}"),
		"Unexpected U+2028 (LINE SEPARATOR; only space, tab, and line feed are whitespace) after a value at line 1, column 5"
	);
}

#[test]
fn reads_invalid_utf8_from_a_slice_with_its_position() {
	let error = soml::from_slice::<Value>(b"a: 'x'\nb: '\xFF'").expect_err("0xFF is not UTF-8");
	assert_eq!(
		error.to_string(),
		"Invalid UTF-8 byte 0xFF at line 2, column 5"
	);
	assert_eq!(error.offset(), Some(11));
}

#[test]
fn reads_a_truncated_utf8_sequence_from_a_slice_with_its_position() {
	// The first two bytes of the three-byte sequence for €, then the end.
	let error = soml::from_slice::<Value>(b"a: '\xE2\x82").expect_err("the sequence is incomplete");
	assert_eq!(
		error.to_string(),
		"Incomplete UTF-8 sequence at the end of the input at line 1, column 5"
	);
}

#[test]
fn rejects_an_encoded_surrogate_from_a_slice() {
	// U+D800 encoded as if it were a scalar value.
	let error =
		soml::from_slice::<Value>(b"a: '\xED\xA0\x80'").expect_err("a surrogate is not UTF-8");
	assert_eq!(
		error.to_string(),
		"Invalid UTF-8 byte 0xED at line 1, column 5"
	);
}

#[test]
fn reads_a_valid_slice() {
	assert_eq!(
		soml::from_slice::<Value>("a: 'é'".as_bytes()).expect("the slice is valid"),
		object(&[("a", Value::from("é"))])
	);
}

#[test]
fn rejects_a_bom_from_a_slice() {
	let error = soml::from_slice::<Value>(b"\xEF\xBB\xBFa: 1").expect_err("a BOM is not allowed");
	assert_eq!(
		error.to_string(),
		"A byte order mark (BOM) is not allowed at line 1, column 1"
	);
}

#[test]
fn a_column_counts_unicode_scalar_values() {
	// The emoji is four bytes and one scalar value, so the column and the byte offset differ.
	let error = "a: '😀' x"
		.parse::<Value>()
		.expect_err("x is after a value");
	assert_eq!(
		error.message(),
		"Expected a line break before the next entry, but found “x”"
	);
	assert_eq!(error.position(), Some(LineColumn { line: 1, column: 8 }));
	assert_eq!(error.offset(), Some(10));
}

#[test]
fn a_column_counts_a_combining_mark_as_its_own_scalar_value() {
	let error = "a: 'e\u{301}' x"
		.parse::<Value>()
		.expect_err("x is after a value");
	assert_eq!(error.position(), Some(LineColumn { line: 1, column: 9 }));
	assert_eq!(error.offset(), Some(9));
}

#[test]
fn reports_the_line_of_an_error_after_many_lines() {
	let text = format!(
		"{}z: oops",
		(0..500)
			.map(|index| format!("k{index}: {index}\n"))
			.collect::<String>()
	);
	assert_eq!(
		rejection(&text),
		"Unexpected “oops”. A string value must be quoted, as in 'oops' at line 501, column 4"
	);
}

#[test]
fn an_error_message_abbreviates_a_huge_token() {
	let text = format!("a: x{}", "y".repeat(10_000));
	let message = rejection(&text);
	assert!(message.len() < 200, "{message}");
	assert!(
		message.starts_with("Unexpected “xyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy…”"),
		"{message}"
	);
}

#[test]
fn a_long_radix_number_is_diagnosed_by_all_of_its_digits() {
	let zeros = "0".repeat(1200);
	assert_eq!(
		rejection(&format!("a: 0x{zeros}g")),
		"Invalid hexadecimal digit “g” at line 1, column 4"
	);
}
