/*!
Property tests: random values read back from their canonical form, canonical form is a fixed point, random floats read back exactly, and random respellings of a document have the same canonical form.
*/

#![allow(clippy::tabs_in_doc_comments)]

use proptest::prelude::*;
use proptest::test_runner::TestCaseError;
use soml::{Duration, Instant, Object, Value};

/**
Characters that exercise the quoting and escaping rules, mixed into random strings and keys.
*/
const CHARACTERS: [char; 33] = [
	'a', 'Z', '0', '9', '_', '-', '.', ' ', '\'', '"', '\\', '\t', '\n', '\u{0}', '\u{1}',
	'\u{1F}', '\u{7F}', '\u{85}', '\u{A0}', '\u{2028}', '\u{FEFF}', '\u{FFFE}', 'é', '😀', '#',
	'/', '*', ':', '{', '[', ',', 'e', '\u{301}',
];

const NANOSECONDS_PER_SECOND: u64 = 1_000_000_000;
const NANOSECONDS_PER_HOUR: u64 = 3600 * NANOSECONDS_PER_SECOND;

/**
A string without a carriage return, which SOML cannot represent.
*/
fn text() -> impl Strategy<Value = String> {
	prop_oneof![
		prop::collection::vec(prop::sample::select(CHARACTERS.to_vec()), 0..6)
			.prop_map(|characters| characters.into_iter().collect()),
		any::<String>().prop_map(|text| text.replace('\r', "")),
		"[a-z0-9_-]{1,6}",
	]
}

/**
A float that is not NaN and not negative zero, with every exponent likely.
*/
fn float() -> impl Strategy<Value = f64> {
	prop_oneof![
		any::<u64>().prop_map(f64::from_bits),
		any::<i32>().prop_map(|value| f64::from(value) / 64.0),
		Just(f64::INFINITY),
		Just(f64::NEG_INFINITY),
		Just(0.0),
		Just(f64::MAX),
		Just(f64::from_bits(1)),
	]
	.prop_filter_map("NaN is not a SOML value", |value| {
		if value.is_nan() {
			return None;
		}

		// Canonical form writes negative zero as zero, so it would not read back with the same bits.
		Some(if value == 0.0 { 0.0 } else { value })
	})
}

fn instant() -> impl Strategy<Value = Instant> {
	let seconds = prop_oneof![
		Just(Instant::MIN.unix_seconds()),
		Just(Instant::MAX.unix_seconds()),
		Instant::MIN.unix_seconds()..=Instant::MAX.unix_seconds(),
	];
	let nanoseconds = prop_oneof![Just(0u32), Just(999_999_999), 0u32..1_000_000_000];

	(seconds, nanoseconds).prop_map(|(seconds, nanoseconds)| {
		Instant::from_unix(seconds, nanoseconds).expect("the instant is in range")
	})
}

fn duration() -> impl Strategy<Value = Duration> {
	prop_oneof![
		Just(i64::MIN),
		Just(i64::MAX),
		Just(0),
		any::<i64>(),
		(-1_000_000i64..1_000_000).prop_map(|seconds| seconds * 1_000_000_000),
	]
	.prop_map(Duration::from_nanoseconds)
}

fn scalar() -> BoxedStrategy<Value> {
	prop_oneof![
		Just(Value::Null),
		any::<bool>().prop_map(Value::Bool),
		any::<i64>().prop_map(Value::Int),
		float().prop_map(Value::Float),
		text().prop_map(Value::String),
		instant().prop_map(Value::Instant),
		duration().prop_map(Value::Duration),
	]
	.boxed()
}

fn value() -> impl Strategy<Value = Value> {
	scalar().prop_recursive(4, 48, 5, |inner| {
		prop_oneof![
			prop::collection::vec(inner.clone(), 0..5).prop_map(Value::Array),
			prop::collection::btree_map(text(), inner, 0..5).prop_map(Value::Object),
		]
	})
}

/**
A value that is a document: an array or an object.
*/
fn document() -> impl Strategy<Value = Value> {
	prop_oneof![
		prop::collection::vec(value(), 0..5).prop_map(Value::Array),
		prop::collection::btree_map(text(), value(), 0..5).prop_map(Value::Object),
	]
}

/**
Whether two values are the same, comparing floats by their bits.
*/
fn same(left: &Value, right: &Value) -> bool {
	match (left, right) {
		(Value::Float(left), Value::Float(right)) => left.to_bits() == right.to_bits(),
		(Value::Array(left), Value::Array(right)) => {
			left.len() == right.len()
				&& left
					.iter()
					.zip(right)
					.all(|(left, right)| same(left, right))
		}
		(Value::Object(left), Value::Object(right)) => {
			left.len() == right.len()
				&& left.iter().zip(right).all(
					|((left_key, left_value), (right_key, right_value))| {
						left_key == right_key && same(left_value, right_value)
					},
				)
		}
		_ => left == right,
	}
}

fn read(text: &str) -> Result<Value, TestCaseError> {
	text.parse()
		.map_err(|error| TestCaseError::fail(format!("{text:?} does not read: {error}")))
}

fn write(value: &Value) -> Result<String, TestCaseError> {
	soml::to_string_canonical(value)
		.map_err(|error| TestCaseError::fail(format!("{value:?} cannot be written: {error}")))
}

/**
The significant digits of a decimal float text, without the sign, the point, the exponent, and leading or trailing zeros.
*/
fn significant_digits(text: &str) -> String {
	let significand = text.split('e').next().unwrap_or_default();
	let digits: String = significand.chars().filter(char::is_ascii_digit).collect();
	digits
		.trim_start_matches('0')
		.trim_end_matches('0')
		.to_owned()
}

proptest! {
	#![proptest_config(ProptestConfig::with_cases(512))]

	#[test]
	fn a_value_reads_back_from_its_canonical_form(value in document()) {
		let text = write(&value)?;
		let read_back = read(&text)?;
		prop_assert!(same(&read_back, &value), "{:?} read back as {:?} from {:?}", value, read_back, text);

		let from_slice: Value = soml::from_slice(text.as_bytes()).map_err(|error| TestCaseError::fail(error.to_string()))?;
		prop_assert!(same(&from_slice, &value));
	}

	#[test]
	fn canonical_form_is_a_fixed_point(value in document()) {
		let text = write(&value)?;
		prop_assert_eq!(write(&read(&text)?)?, text);
	}

	#[test]
	fn a_value_round_trips_through_serde(value in document()) {
		let through_serde: Value = soml::from_str(&write(&value)?).map_err(|error| TestCaseError::fail(error.to_string()))?;
		prop_assert!(same(&through_serde, &value));

		let through_value = soml::to_value(&value).map_err(|error| TestCaseError::fail(error.to_string()))?;
		prop_assert!(same(&through_value, &value));

		let from_value: Value = soml::from_value(value.clone()).map_err(|error| TestCaseError::fail(error.to_string()))?;
		prop_assert!(same(&from_value, &value));
	}

	#[test]
	fn a_finite_float_reads_back_exactly_with_the_shortest_digits(bits in any::<u64>()) {
		let value = f64::from_bits(bits);
		prop_assume!(value.is_finite());

		let document: Value = [("a", Value::Float(value))].into_iter().collect();
		let text = write(&document)?;
		let read_back = read(&text)?.get("a").and_then(Value::as_f64);
		let expected = if value == 0.0 { 0.0 } else { value };
		prop_assert_eq!(read_back.map(f64::to_bits), Some(expected.to_bits()), "{}", text);

		// std's `{:e}` also writes the shortest digits, so the counts agree, even where the two choose different digits for a tie.
		let written = text.trim_start_matches("a: ").trim_end_matches('\n');
		prop_assert_eq!(significant_digits(written).len(), significant_digits(&format!("{value:e}")).len(), "{}", written);
	}

	#[test]
	fn a_float_reads_into_f32_as_its_text_rounded_once(bits in 1..0x7F7F_FFFFu32, is_negative in any::<bool>(), digits in 0..70usize) {
		// The halfway point between two f32 values is exact in an f64, and its digits, cut at some length, lie on either side of it, where rounding through an f64 can go the wrong way.
		let low = f32::from_bits(bits);
		let high = f32::from_bits(bits + 1);
		let middle = f64::midpoint(f64::from(low), f64::from(high));
		let middle = if is_negative { -middle } else { middle };
		let text = format!("{middle:.digits$e}");
		let expected: f32 = text.parse().expect("a float");

		let read_back: Vec<f32> = soml::from_str(&format!("[{text}]")).map_err(|error| TestCaseError::fail(format!("{text}: {error}")))?;
		prop_assert_eq!(read_back[0].to_bits(), expected.to_bits(), "{}", text);
	}

	#[test]
	fn a_respelled_document_has_the_same_canonical_form(value in document(), seed in any::<u64>()) {
		let canonical = write(&value)?;
		let respelled = Respeller { state: seed }.document(&value);
		prop_assert_eq!(write(&read(&respelled)?)?, canonical, "respelled as {:?}", respelled);
	}

	#[test]
	fn formatting_keeps_the_value_and_the_comments_and_is_idempotent(value in document(), seed in any::<u64>()) {
		let respelled = Respeller { state: seed }.document(&value);
		let formatted = soml::format(&respelled).map_err(|error| TestCaseError::fail(error.to_string()))?;
		prop_assert!(same(&read(&formatted)?, &value), "{:?} formatted as {:?}", respelled, formatted);
		prop_assert_eq!(soml::format(&formatted).expect("valid"), formatted.clone(), "{:?}", respelled);

		let comment_count = |text: &str| text.parse::<soml::Document>().expect("valid").comments().len();
		prop_assert_eq!(comment_count(&formatted), comment_count(&respelled), "{:?} formatted as {:?}", respelled, formatted);
	}
}

/**
Writes a value in random legal spellings: other quotes, escapes, block strings, radix ints, underscores, other float layouts, offsets on instants, other duration units, any member order, commas or line breaks between items, comments, and whitespace. The choices come from a seed, so a failure can be reproduced.
*/
struct Respeller {
	state: u64,
}

impl Respeller {
	/**
	SplitMix64.
	*/
	fn next(&mut self) -> u64 {
		self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
		let mut value = self.state;
		value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
		value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
		value ^ (value >> 31)
	}

	fn below(&mut self, count: usize) -> usize {
		(self.next() % count as u64) as usize
	}

	fn chance(&mut self, count: usize) -> bool {
		self.below(count) == 0
	}

	fn document(&mut self, value: &Value) -> String {
		let mut output = self.space(true);

		match value {
			Value::Object(object) if !object.is_empty() && !self.chance(4) => {
				for (index, member) in self.members(object).into_iter().enumerate() {
					if index > 0 {
						output.push_str(&self.entry_separator());
					}

					output.push_str(&member);
				}
			}
			_ => output.push_str(&self.value(value, 0)),
		}

		output.push_str(&self.space(true));
		output
	}

	/**
	Whitespace and comments. A `#` comment always ends its line, so it is only used where a line break is allowed.
	*/
	fn space(&mut self, allows_line_break: bool) -> String {
		let options: &[&str] = if allows_line_break {
			&[
				"",
				" ",
				"\n",
				"\n\t\t",
				" /* c */ ",
				" # é /* x\n",
				"\n\n  # c\n",
				"/* a\nb */",
			]
		} else {
			&["", " ", "\t", " /* c */ ", "/**/"]
		};

		options[self.below(options.len())].to_owned()
	}

	/**
	What separates two top-level entries: at least one line break outside a block comment.
	*/
	fn entry_separator(&mut self) -> String {
		let options = [
			"\n",
			"\n\n",
			" # c\n",
			" /* c */\n# d\n\t",
			"\n/* x\n y */\n",
		];
		options[self.below(options.len())].to_owned()
	}

	/**
	What follows an item inside brackets: a comma on the line of the item, or line breaks without one, or nothing after the last item, where a comma is optional. Whitespace and comments come before either.
	*/
	fn item_separator(&mut self, is_last: bool) -> String {
		// A comma goes on the line of the item before it, so no line break outside a comment comes before it.
		if self.chance(2) {
			let options = ["", " ", " /* c */ ", " /* a\nb */"];
			return format!("{},", options[self.below(options.len())]);
		}

		let space = self.space(true);

		if is_last {
			return space;
		}

		format!("{space}{}", self.entry_separator())
	}

	/**
	The members of an object in a random order, as `key: value` texts.
	*/
	fn members(&mut self, object: &Object) -> Vec<String> {
		let mut entries: Vec<(&String, &Value)> = object.iter().collect();
		let mut members = Vec::new();

		while !entries.is_empty() {
			let index = self.below(entries.len());
			let (key, value) = entries.swap_remove(index);
			let key = self.key(key);
			let after_colon = self.space(true);
			let rendered = self.value(value, 1);
			members.push(format!("{key}:{after_colon}{rendered}"));
		}

		members
	}

	/**
	A value. `indentation` is the number of tabs a block string is indented with.
	*/
	fn value(&mut self, value: &Value, indentation: usize) -> String {
		match value {
			Value::Null => String::from("null"),
			Value::Bool(value) => value.to_string(),
			Value::Int(value) => self.int(*value),
			Value::Float(value) => self.float(*value),
			Value::String(text) => self.string(text, indentation),
			Value::Instant(instant) => self.instant(*instant),
			Value::Duration(duration) => self.duration(*duration),
			Value::Array(items) => {
				let mut output = String::from("[");

				for (index, item) in items.iter().enumerate() {
					output.push_str(&self.space(true));
					output.push_str(&self.value(item, indentation + 1));
					output.push_str(&self.item_separator(index + 1 == items.len()));
				}

				output.push_str(&self.space(true));
				output.push(']');
				output
			}
			Value::Object(object) => {
				let mut output = String::from("{");
				let members = self.members(object);
				let count = members.len();

				for (index, member) in members.into_iter().enumerate() {
					output.push_str(&self.space(true));
					output.push_str(&member);
					output.push_str(&self.item_separator(index + 1 == count));
				}

				output.push_str(&self.space(true));
				output.push('}');
				output
			}
		}
	}

	fn key(&mut self, key: &str) -> String {
		let is_bare = !key.is_empty()
			&& key
				.bytes()
				.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');

		if is_bare && !self.chance(3) {
			return key.to_owned();
		}

		if can_be_literal(key) && self.chance(2) {
			return format!("'{key}'");
		}

		self.escaped(key)
	}

	fn string(&mut self, text: &str, indentation: usize) -> String {
		match self.below(4) {
			0 if can_be_literal(text) => format!("'{text}'"),
			1 if can_be_literal_block(text) => {
				let indentation = "\t".repeat(indentation + self.below(2));
				let lines: Vec<String> = text
					.split('\n')
					.map(|line| format!("{indentation}{line}"))
					.collect();
				format!("'''\n{}\n{indentation}'''", lines.join("\n"))
			}
			2 => {
				// One content line, with every line feed as `\n`, and leading spaces and tabs escaped so that the line is never blank.
				let indentation = " ".repeat(self.below(3));
				let mut line = String::new();
				let mut is_leading = true;

				for character in text.chars() {
					let escaped = match character {
						' ' if is_leading => String::from("\\u{20}"),
						'\t' if is_leading => String::from("\\t"),
						'\n' => String::from("\\n"),
						'"' => String::from("\\\""),
						_ => self.escaped_character(character),
					};

					is_leading = false;
					line.push_str(&escaped);
				}

				format!("\"\"\"\n{indentation}{line}\n{indentation}\"\"\"")
			}
			_ => self.escaped(text),
		}
	}

	fn escaped(&mut self, text: &str) -> String {
		let mut output = String::from("\"");

		for character in text.chars() {
			output.push_str(&self.escaped_character(character));
		}

		output.push('"');
		output
	}

	/**
	A character inside `"..."`: as itself when it may be, or one of its escapes.
	*/
	fn escaped_character(&mut self, character: char) -> String {
		let code = u32::from(character);
		let must_escape = matches!(character, '"' | '\\' | '\n')
			|| (code < 0x20 && character != '\t')
			|| code == 0x7F;

		if !must_escape && !self.chance(4) {
			return character.to_string();
		}

		let short = match character {
			'"' => Some("\\\""),
			'\\' => Some("\\\\"),
			'\n' => Some("\\n"),
			'\t' => Some("\\t"),
			_ => None,
		};

		match short {
			Some(short) if self.chance(2) => short.to_owned(),
			_ => format!("\\u{{{code:x}}}"),
		}
	}

	fn int(&mut self, value: i64) -> String {
		let digits = value.unsigned_abs();
		let sign = if value < 0 { "-" } else { "" };

		match self.below(5) {
			0 if value >= 0 => format!(
				"0x{}{}",
				"0".repeat(self.below(3)),
				self.underscores(&format!("{digits:X}"))
			),
			1 if value >= 0 => format!("0o{}", self.underscores(&format!("{digits:o}"))),
			2 if value >= 0 => format!("0b{}", self.underscores(&format!("{digits:b}"))),
			3 => format!("{sign}{}", self.underscores(&digits.to_string())),
			_ => value.to_string(),
		}
	}

	/**
	Digits with a `_` between some of them.
	*/
	fn underscores(&mut self, digits: &str) -> String {
		let mut output = String::new();

		for (index, digit) in digits.chars().enumerate() {
			if index > 0 && self.chance(3) {
				output.push('_');
			}

			output.push(digit);
		}

		output
	}

	fn float(&mut self, value: f64) -> String {
		if value.is_infinite() {
			return String::from(if value > 0.0 { "infinity" } else { "-infinity" });
		}

		match self.below(4) {
			0 if value == 0.0 => String::from("-0.0"),
			1 => format!("{value:e}"),
			2 => format!("{value:?}"),
			_ => {
				let document: Value = [("a", Value::Float(value))].into_iter().collect();
				let text = soml::to_string_canonical(&document)
					.expect("a float that is not NaN can be written");
				text.trim_start_matches("a: ")
					.trim_end_matches('\n')
					.to_owned()
			}
		}
	}

	/**
	An instant at a random offset, with a fraction that may have trailing zeros. The local time is the canonical text of the instant moved by the offset, when that is in range too.
	*/
	fn instant(&mut self, instant: Instant) -> String {
		let offset_minutes = self.below(2 * 1439 + 1) as i64 - 1439;
		let Some(local) = Instant::from_unix(
			instant.unix_seconds() + offset_minutes * 60,
			instant.nanoseconds(),
		) else {
			return instant.to_string();
		};

		let date_and_time = &local.to_string()[..19];
		let fraction = match instant.nanoseconds() {
			0 => [
				String::new(),
				String::from(".0"),
				String::from(".000000000"),
			][self.below(3)]
			.clone(),
			nanoseconds => {
				let digits = format!("{nanoseconds:09}");
				if self.chance(2) {
					format!(".{digits}")
				} else {
					format!(".{}", digits.trim_end_matches('0'))
				}
			}
		};

		let offset = if offset_minutes == 0 && self.chance(2) {
			String::from("Z")
		} else {
			let sign = if offset_minutes < 0 { '-' } else { '+' };
			let magnitude = offset_minutes.unsigned_abs();
			format!("{sign}{:02}:{:02}", magnitude / 60, magnitude % 60)
		};

		format!("{date_and_time}{fraction}{offset}")
	}

	fn duration(&mut self, duration: Duration) -> String {
		let sign = if duration.is_negative() { "-" } else { "" };
		let magnitude = duration.nanoseconds().unsigned_abs();

		match self.below(4) {
			0 => format!("{sign}{}ns", self.underscores(&magnitude.to_string())),
			1 => format!(
				"{sign}{}.{:09}s",
				magnitude / NANOSECONDS_PER_SECOND,
				magnitude % NANOSECONDS_PER_SECOND
			),
			2 => format!(
				"{sign}{}h{}ns",
				magnitude / NANOSECONDS_PER_HOUR,
				magnitude % NANOSECONDS_PER_HOUR
			),
			_ => duration.to_string(),
		}
	}
}

/**
Whether a string can be written as `'...'`: no `'`, no line feed, and no control character but tab.
*/
fn can_be_literal(text: &str) -> bool {
	!text
		.chars()
		.any(|character| character == '\'' || (character.is_ascii_control() && character != '\t'))
}

/**
Whether a string can be written as a `'''` block with one source line per line: no `'`, no control character but tab and line feed, and no line that is empty or only spaces and tabs, since such a line is blank.
*/
fn can_be_literal_block(text: &str) -> bool {
	!text.chars().any(|character| {
		character == '\''
			|| (character.is_ascii_control() && character != '\t' && character != '\n')
	}) && text.split('\n').all(|line| {
		!line
			.chars()
			.all(|character| character == ' ' || character == '\t')
	})
}

#[test]
fn the_respeller_writes_valid_documents_for_a_fixed_example() {
	let value: Value = "a: {b: [1, -2, 1.5, 'x y', 2026-09-19T14:00:00.5Z, -90m, {}, []], 'c.d': \"it's\\n\"}\ne: infinity\nf: 0.0"
		.parse()
		.expect("a valid document");
	let canonical = soml::to_string_canonical(&value).expect("a document");

	for seed in 0..500 {
		let respelled = Respeller { state: seed }.document(&value);
		let read_back: Value = respelled
			.parse()
			.unwrap_or_else(|error| panic!("{respelled:?}: {error}"));
		assert_eq!(
			soml::to_string_canonical(&read_back).expect("a document"),
			canonical,
			"{respelled:?}"
		);
	}
}
