/*!
Canonical form: the exact bytes `to_string_canonical` writes for a value. `to_string` writes the same, except that members keep their order.
*/

#![allow(clippy::tabs_in_doc_comments)]

use soml::{Duration, Instant, Value};

/**
The canonical text of a value, as the member `a` of a document.
*/
fn written(value: impl Into<Value>) -> String {
	let document: Value = [("a", value.into())].into_iter().collect();
	let text = soml::to_string_canonical(&document).expect("the value can be written");

	text.strip_prefix("a: ")
		.and_then(|text| text.strip_suffix('\n'))
		.unwrap_or_else(|| panic!("unexpected document {text:?}"))
		.to_owned()
}

/**
The canonical text of a key, as a document with that key.
*/
fn written_key(key: &str) -> String {
	let document: Value = [(key, Value::Int(1))].into_iter().collect();
	let text = soml::to_string_canonical(&document).expect("the key can be written");

	text.strip_suffix(": 1\n")
		.unwrap_or_else(|| panic!("unexpected document {text:?}"))
		.to_owned()
}

fn canonical(text: &str) -> String {
	let value: Value = text
		.parse()
		.unwrap_or_else(|error| panic!("{text:?} should read, but: {error}"));
	soml::to_string_canonical(&value).expect("a document that was read can be written")
}

// Floats, spec rule 11

#[test]
fn writes_zero_and_infinity() {
	assert_eq!(written(0.0), "0.0");
	assert_eq!(written(-0.0), "0.0");
	assert_eq!(written(f64::INFINITY), "infinity");
	assert_eq!(written(f64::NEG_INFINITY), "-infinity");
}

#[test]
fn writes_an_integral_float_with_zeros_up_to_21_digits() {
	// k ≤ n ≤ 21
	assert_eq!(written(1.0), "1.0");
	assert_eq!(written(100.0), "100.0");
	assert_eq!(written(-3.0), "-3.0");
	assert_eq!(written(9_007_199_254_740_992.0), "9007199254740992.0");
	assert_eq!(written(1e20), "100000000000000000000.0");
	assert_eq!(written(1.2345678901234568e20), "123456789012345680000.0");
}

#[test]
fn writes_a_float_with_a_point_inside_its_digits() {
	// 0 < n < k
	assert_eq!(written(1.5), "1.5");
	assert_eq!(written(314.0 / 100.0), "3.14");
	assert_eq!(written(-1.25), "-1.25");
	assert_eq!(written(123.456), "123.456");
	assert_eq!(written(0.1 + 0.2), "0.30000000000000004");
}

#[test]
fn writes_a_small_float_with_leading_zeros() {
	// −6 < n ≤ 0
	assert_eq!(written(0.5), "0.5");
	assert_eq!(written(0.1), "0.1");
	assert_eq!(written(0.000001), "0.000001");
	assert_eq!(written(0.000_012_34), "0.00001234");
	assert_eq!(written(-0.000001), "-0.000001");
}

#[test]
fn writes_other_floats_with_an_exponent() {
	assert_eq!(written(1e21), "1e21");
	assert_eq!(written(1.2345678901234568e21), "1.2345678901234568e21");
	assert_eq!(written(1.5e300), "1.5e300");
	assert_eq!(written(1e100), "1e100");
	assert_eq!(written(1e-7), "1e-7");
	assert_eq!(written(1.5e-7), "1.5e-7");
	assert_eq!(written(-1e-7), "-1e-7");
	assert_eq!(written(5e-324), "5e-324");
	assert_eq!(written(f64::MIN_POSITIVE), "2.2250738585072014e-308");
	assert_eq!(written(f64::MAX), "1.7976931348623157e308");
	assert_eq!(written(f64::MIN), "-1.7976931348623157e308");
}

#[test]
fn writes_the_even_digit_when_two_shortest_digit_strings_are_equally_close() {
	// 1658206780088562.25 is exactly halfway between the 17-digit candidates …2 and …3, and the spec takes the even one. std writes …3.
	let value: f64 = 1_658_206_780_088_562.0 + 0.25;
	assert_eq!(written(value), "1658206780088562.2");
	assert_eq!(
		"1658206780088562.2".parse::<f64>().map(f64::to_bits),
		Ok(value.to_bits())
	);
	assert_eq!(
		"1658206780088562.3".parse::<f64>().map(f64::to_bits),
		Ok(value.to_bits())
	);
}

#[test]
fn a_float_keeps_no_spelling_from_the_source() {
	assert_eq!(canonical("a: 1_0.0_1e1_0"), "a: 100100000000.0\n");
	assert_eq!(canonical("a: 0.1e1"), "a: 1.0\n");
	assert_eq!(canonical("a: 3e-324"), "a: 5e-324\n");
}

#[test]
fn refuses_to_write_nan() {
	let document: Value = [("a", Value::Float(f64::NAN))].into_iter().collect();
	let error = soml::to_string(&document).expect_err("NaN is not a SOML value");
	assert_eq!(error.to_string(), "NaN is not a SOML value");
	assert_eq!(error.position(), None);
	assert_eq!(error.offset(), None);
}

// Ints

#[test]
fn writes_an_int_in_plain_decimal() {
	assert_eq!(written(0), "0");
	assert_eq!(written(-5), "-5");
	assert_eq!(written(i64::MAX), "9223372036854775807");
	assert_eq!(written(i64::MIN), "-9223372036854775808");
	assert_eq!(
		canonical("a: 0xFF\nb: 0o644\nc: 0b1010\nd: 1_000"),
		"a: 255\nb: 420\nc: 10\nd: 1000\n"
	);
}

// Strings, spec rules 8 and 9

#[test]
fn writes_a_literal_string_unless_it_must_be_escaped() {
	assert_eq!(written("plain"), "'plain'");
	assert_eq!(written(""), "''");
	assert_eq!(written(r"C:\Users"), r"'C:\Users'");
	assert_eq!(written(r#"say "hi""#), r#"'say "hi"'"#);
	assert_eq!(written("é😀"), "'é😀'");
}

#[test]
fn writes_an_escaped_string_for_a_quote_a_tab_a_line_feed_or_a_control() {
	assert_eq!(written("it's"), r#""it's""#);
	assert_eq!(written("a\tb"), r#""a\tb""#);
	assert_eq!(written("a\nb"), r#""a\nb""#);
	assert_eq!(written("\u{0}"), r#""\u{0}""#);
	assert_eq!(written("\u{1B}"), r#""\u{1b}""#);
	assert_eq!(written("\u{1F}"), r#""\u{1f}""#);
	assert_eq!(written("\u{7F}"), r#""\u{7f}""#);
}

#[test]
fn escapes_a_backslash_and_a_double_quote_only_in_an_escaped_string() {
	assert_eq!(written(r#"it's "x" \"#), r#""it's \"x\" \\""#);
}

#[test]
fn writes_invisible_characters_and_noncharacters_literally() {
	assert_eq!(
		written("\u{2028}\u{2029}\u{FEFF}\u{200B}\u{85}"),
		"'\u{2028}\u{2029}\u{FEFF}\u{200B}\u{85}'"
	);
	assert_eq!(written("\u{FFFF}\u{FDD0}"), "'\u{FFFF}\u{FDD0}'");
	assert_eq!(written("\u{2028}\t"), "\"\u{2028}\\t\"");
}

#[test]
fn refuses_to_write_a_carriage_return() {
	let document: Value = [("a", Value::from("x\ry"))].into_iter().collect();
	assert_eq!(
		soml::to_string(&document)
			.expect_err("CR is not representable")
			.to_string(),
		"A string cannot contain a carriage return (U+000D), because SOML cannot represent one"
	);

	let document: Value = [("x\ry", Value::Int(1))].into_iter().collect();
	assert_eq!(
		soml::to_string(&document)
			.expect_err("CR is not representable")
			.to_string(),
		"A key cannot contain a carriage return (U+000D), because SOML cannot represent one"
	);
}

#[test]
fn a_string_keeps_no_spelling_from_the_source() {
	assert_eq!(canonical(r#"a: "\u{61}b""#), "a: 'ab'\n");
	assert_eq!(canonical("a: '''\n\tx\n\ty\n\t'''"), "a: \"x\\ny\"\n");
}

// Keys

#[test]
fn writes_a_key_bare_when_it_can() {
	assert_eq!(written_key("a"), "a");
	assert_eq!(written_key("404"), "404");
	assert_eq!(written_key("-"), "-");
	assert_eq!(written_key("content-type"), "content-type");
	assert_eq!(written_key("true"), "true");
	assert_eq!(written_key("_private"), "_private");
}

#[test]
fn quotes_a_key_by_the_string_rules_otherwise() {
	assert_eq!(written_key(""), "''");
	assert_eq!(written_key("a.b"), "'a.b'");
	assert_eq!(written_key("the name"), "'the name'");
	assert_eq!(written_key("é"), "'é'");
	assert_eq!(written_key("it's"), r#""it's""#);
	assert_eq!(written_key("a\nb"), r#""a\nb""#);
	assert_eq!(written_key(r"a\b"), r"'a\b'");
}

#[test]
fn sorts_members_by_unicode_scalar_values() {
	let document: Value = [
		"ab", "aa", "a_b", "a-b", "a", "Z", "é", "😀", "\u{FFFF}", "10", "9",
	]
	.into_iter()
	.map(|key| (key, Value::Null))
	.collect();

	assert_eq!(
		soml::to_string_canonical(&document).expect("the document can be written"),
		"10: null\n9: null\nZ: null\na: null\na-b: null\na_b: null\naa: null\nab: null\n'é': null\n'\u{FFFF}': null\n'😀': null\n"
	);
}

#[test]
fn sorting_is_by_scalar_values_not_by_canonical_equivalence() {
	// A composed é (U+00E9) sorts after an e with a combining accent (U+0065 U+0301), and they stay two keys.
	assert_eq!(
		canonical("'\u{E9}': 1\n'e\u{301}': 2"),
		"'e\u{301}': 2\n'\u{E9}': 1\n"
	);
}

// Layout

#[test]
fn writes_an_empty_top_level_object_with_braces() {
	assert_eq!(
		soml::to_string(&Value::Object(soml::Object::new())).expect("{}"),
		"{}\n"
	);
	assert_eq!(canonical("{}"), "{}\n");
	assert_eq!(canonical("# nothing\n{ /* still nothing */ }"), "{}\n");
}

#[test]
fn writes_an_empty_top_level_array() {
	assert_eq!(
		soml::to_string(&Value::Array(Vec::new())).expect("[]"),
		"[]\n"
	);
}

#[test]
fn writes_a_top_level_array_with_brackets_at_column_zero() {
	assert_eq!(
		canonical("[1, 'x', [], {}]"),
		"[\n\t1\n\t'x'\n\t[]\n\t{}\n]\n"
	);
	assert_eq!(canonical("[{a: 1}]"), "[\n\t{\n\t\ta: 1\n\t}\n]\n");
}

#[test]
fn writes_a_braced_top_level_object_without_braces() {
	assert_eq!(canonical("{b: 2, a: 1}"), "a: 1\nb: 2\n");
}

#[test]
fn indents_nested_collections_with_tabs_and_no_commas() {
	assert_eq!(
		canonical("a: {b: [1, {c: [], d: {}}], e: 'x'}\nf: [[1, 2], [3]]"),
		"a: {\n\tb: [\n\t\t1\n\t\t{\n\t\t\tc: []\n\t\t\td: {}\n\t\t}\n\t]\n\te: 'x'\n}\nf: [\n\t[\n\t\t1\n\t\t2\n\t]\n\t[\n\t\t3\n\t]\n]\n"
	);
}

#[test]
fn removes_comments_and_block_strings() {
	assert_eq!(
		canonical("# head\nserver: {host: 'x' # host\nport: 80}\nnote: '''\n\tline\n\t'''"),
		"note: 'line'\nserver: {\n\thost: 'x'\n\tport: 80\n}\n"
	);
}

#[test]
fn writes_bools_null_instants_and_durations() {
	assert_eq!(written(true), "true");
	assert_eq!(written(false), "false");
	assert_eq!(written(Value::Null), "null");
	assert_eq!(
		written(Instant::from_unix(1_789_826_400, 0).expect("in range")),
		"2026-09-19T14:00:00Z"
	);
	assert_eq!(
		written(Instant::from_unix(1_789_826_400, 1).expect("in range")),
		"2026-09-19T14:00:00.000000001Z"
	);
	assert_eq!(
		written(Instant::from_unix(1_789_826_400, 120_000_000).expect("in range")),
		"2026-09-19T14:00:00.12Z"
	);
	assert_eq!(written(Instant::MIN), "0001-01-01T00:00:00Z");
	assert_eq!(written(Instant::MAX), "9999-12-31T23:59:59.999999999Z");
	assert_eq!(written(Duration::from_nanoseconds(0)), "0s");
	assert_eq!(
		written(Duration::from_nanoseconds(5_400_000_000_000)),
		"1h30m"
	);
	assert_eq!(written(Duration::from_nanoseconds(1_500_000_000)), "1.5s");
	assert_eq!(written(Duration::from_nanoseconds(250_000)), "0.00025s");
	assert_eq!(written(Duration::from_nanoseconds(-1_500_000_000)), "-1.5s");
	assert_eq!(
		written(Duration::from_nanoseconds(3_600_000_000_001)),
		"1h0.000000001s"
	);
	assert_eq!(written(Duration::MIN), "-2562047h47m16.854775808s");
	assert_eq!(written(Duration::MAX), "2562047h47m16.854775807s");
}

#[test]
fn an_instant_and_a_duration_keep_no_spelling_from_the_source() {
	assert_eq!(
		canonical("a: 2026-09-19T21:00:00.500+07:00"),
		"a: 2026-09-19T14:00:00.5Z\n"
	);
	assert_eq!(
		canonical("a: 90m\nb: 1500ms\nc: 0.5h\nd: 1_000ns"),
		"a: 1h30m\nb: 1.5s\nc: 30m\nd: 0.000001s\n"
	);
}

#[test]
fn refuses_to_write_a_top_level_scalar() {
	assert_eq!(
		soml::to_string(&Value::Int(1))
			.expect_err("a scalar is not a document")
			.to_string(),
		"A document must be an object or an array, not an int"
	);
	assert_eq!(
		soml::to_string(&Value::Null)
			.expect_err("a scalar is not a document")
			.to_string(),
		"A document must be an object or an array, not null"
	);
	assert_eq!(
		soml::to_string("x")
			.expect_err("a scalar is not a document")
			.to_string(),
		"A document must be an object or an array, not a string"
	);
}

#[test]
fn canonical_form_ends_with_one_line_feed_and_has_no_trailing_whitespace() {
	let text = canonical("a: {b: [1, {c: 'x '}]}\nd: '''\n  e\n\n  f\n  '''");

	assert!(text.ends_with("}\nd: \"e\\n\\nf\"\n"), "{text:?}");
	assert!(!text.ends_with("\n\n"));
	assert!(
		text.lines().all(|line| !line.ends_with([' ', '\t'])),
		"{text:?}"
	);
	assert!(!text.contains('\r'));
}

#[test]
fn canonical_form_is_a_fixed_point() {
	let text = canonical(
		"z: [1.5, 0x10, '''\n  a\n  '''] # c\ny: {x: {w: 2026-09-19T21:00:00+07:00, v: 90m}}\n'a b': -infinity",
	);
	assert_eq!(canonical(&text), text);
}

#[test]
fn member_order_does_not_change_the_bytes() {
	assert_eq!(
		canonical("b: 1\na: 2\nc: {y: 1, x: 2}"),
		canonical("c: {x: 2, y: 1}\na: 2\nb: 1")
	);
}
