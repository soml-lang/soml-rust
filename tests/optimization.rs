use serde::{Deserialize, Serialize};
use soml::{Document, Duration, ErrorKind, Instant, Value};

#[test]
fn unknown_fields_and_variants_have_bounded_safe_diagnostics() {
	#[derive(Debug, Deserialize)]
	#[serde(deny_unknown_fields)]
	#[allow(dead_code)]
	struct Strict {
		known: bool,
	}
	#[derive(Debug, Deserialize)]
	enum Choice {
		Known,
	}
	let name = format!("\u{202e}{}", "😀".repeat(100_000));
	for error in [
		soml::from_str::<Strict>(&format!("'{name}': true")).unwrap_err(),
		soml::from_str::<Vec<Choice>>(&format!("['{name}']")).unwrap_err(),
	] {
		assert_eq!(error.kind(), ErrorKind::Data);
		assert!(error.message().contains('…'));
		assert!(!error.message().contains('\u{202e}'));
		assert!(error.message().len() < 250);
		assert!(error.offset().is_some());
	}
}

#[test]
fn bounded_serde_errors_preserve_short_name_messages() {
	use serde::de::Error as _;
	for expected in [
		&[][..],
		&["one"][..],
		&["one", "two"][..],
		&["one", "two", "three"][..],
	] {
		for name in ["plain", "quoted\"", "line\n"] {
			assert_eq!(
				soml::Error::unknown_field(name, expected).message(),
				serde::de::value::Error::unknown_field(name, expected).to_string(),
			);
			assert_eq!(
				soml::Error::unknown_variant(name, expected).message(),
				serde::de::value::Error::unknown_variant(name, expected).to_string(),
			);
		}
	}
}

#[test]
fn duplicate_keys_from_other_formats_are_bounded_and_sanitized() {
	let key = format!("\u{202e}{}", "😀".repeat(100_000));
	let text = format!("{{\"{key}\":1,\"{key}\":2}}");
	let error = serde_json::from_str::<Value>(&text)
		.unwrap_err()
		.to_string();
	assert!(error.contains("Duplicate key"));
	assert!(error.contains('…'));
	assert!(!error.contains('\u{202e}'));
	assert!(error.len() < 250);
}

#[test]
fn hostile_numeric_suggestions_do_not_expand_the_error() {
	for token in [
		format!("0x{}", "a".repeat(100_000)),
		format!("0{}", "7".repeat(100_000)),
		format!("{}000", "1,".repeat(100_000)),
	] {
		let error = format!("[{token}]").parse::<Value>().unwrap_err();
		assert!(error.message().len() < 300, "{}", error.message().len());
		assert_eq!(error.kind(), ErrorKind::Syntax);
	}
}

#[test]
fn float_precision_checks_cover_every_integer_bit_boundary() {
	for exponent in 0..63 {
		let power = 1_i64 << exponent;
		for value in [
			power - 1,
			power,
			power + 1,
			-(power - 1),
			-power,
			-(power + 1),
		] {
			let text = format!("[{value}]");
			assert_eq!(
				soml::from_str::<Vec<f32>>(&text).is_ok(),
				(value as f32) as i128 == i128::from(value),
				"f32 {value}"
			);
			assert_eq!(
				soml::from_str::<Vec<f64>>(&text).is_ok(),
				(value as f64) as i128 == i128::from(value),
				"f64 {value}"
			);
		}
	}
	for value in [i64::MIN, i64::MAX] {
		let source = Value::Array(vec![Value::Int(value)]);
		assert_eq!(
			soml::from_value::<Vec<f64>>(source).is_ok(),
			(value as f64) as i128 == i128::from(value)
		);
	}
}

#[test]
fn streamed_f32_layout_matches_canonical_form_across_bit_patterns() {
	let mut state = 0x1234_5678_u32;
	let values = (0..20_000).map(|_| {
		state ^= state << 13;
		state ^= state >> 17;
		state ^= state << 5;
		f32::from_bits(state)
	});
	for value in [
		0.0,
		-0.0,
		f32::MIN_POSITIVE,
		f32::from_bits(1),
		f32::MAX,
		f32::INFINITY,
		f32::NEG_INFINITY,
	]
	.into_iter()
	.chain(values)
	.filter(|value| !value.is_nan())
	{
		let text = soml::to_string(&[value]).unwrap();
		assert_eq!(
			text,
			soml::to_string_canonical(&[value]).unwrap(),
			"{value:e}"
		);
		let reread: Vec<f32> = soml::from_str(&text).unwrap();
		assert_eq!(reread[0], value);
	}
}

#[test]
fn streamed_map_entries_keep_key_scratch_storage_across_nested_values() {
	let value = soml::soml!({
		"quote'": {"inner\n": ["value'", "é"]},
		"": Duration::MIN,
		"after": Instant::MAX,
	});
	let text = soml::to_string(&value).unwrap();
	assert_eq!(text, soml::to_string_canonical(&value).unwrap());
	assert_eq!(text.parse::<Value>().unwrap(), value);
}

#[test]
fn caught_map_entry_failures_rebuild_without_partial_output() {
	struct Recover;
	impl Serialize for Recover {
		fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
			use serde::ser::SerializeMap;
			let mut map = serializer.serialize_map(Some(3))?;
			let _ = map.serialize_entry("invalid", &f64::NAN);
			map.serialize_entry("after", &42)?;
			map.end()
		}
	}
	assert_eq!(soml::to_string(&Recover).unwrap(), "after: 42\n");
}

#[test]
fn huge_collection_hints_do_not_change_empty_collection_results() {
	struct HugeHint;
	impl Serialize for HugeHint {
		fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
			use serde::ser::SerializeSeq;
			serializer.serialize_seq(Some(usize::MAX))?.end()
		}
	}
	assert_eq!(soml::to_value(&HugeHint).unwrap(), Value::Array(Vec::new()));
	assert_eq!(soml::to_string_canonical(&HugeHint).unwrap(), "[]\n");
}

#[test]
fn edits_and_removal_preserve_multiline_comment_indentation() {
	for text in [
		"a: [1 /* x\ny */, 2]\nb: 3\n",
		"{a: [1, 2], /* x\ny */ b: 3}",
		"/* opening */\n\t{a: [1, 2], b: 3}",
	] {
		let mut document: Document = text.parse().unwrap();
		document.set(["b"], soml::soml!({nested: [4, 5]})).unwrap();
		assert!(
			document
				.remove([soml::PathSegment::Key("a"), 0.into()])
				.unwrap()
		);
		let printed = document.to_string();
		assert_eq!(
			document.to_value().unwrap(),
			printed.parse::<Value>().unwrap()
		);
		assert_eq!(document.format(), soml::format(&printed).unwrap());
	}
}

#[test]
fn duration_unit_matching_keeps_order_and_fraction_precedence() {
	assert_eq!(
		"1h2m3s4ms5us6ns".parse::<Duration>().unwrap().nanoseconds(),
		3_723_004_005_006
	);
	for text in ["1s1m", "1ms1ms", "1.5s1ms", "1US", "1Ms"] {
		assert!(text.parse::<Duration>().is_err(), "{text}");
	}
	assert_eq!(
		"0.0000000000025h"
			.parse::<Duration>()
			.unwrap()
			.nanoseconds(),
		9
	);
}

#[test]
fn root_offsets_keep_opening_comments_and_quoted_keys_lossless() {
	for text in [
		"/* x */\n{'a': 1}",
		" # x\n[1, 2] # tail",
		"/* x */\n\"a\": 1\n'β': 2",
	] {
		let document: Document = text.parse().unwrap();
		assert_eq!(document.to_string(), text);
		assert_eq!(document.to_value().unwrap(), text.parse::<Value>().unwrap());
	}
}

#[test]
fn block_string_indentation_errors_precede_escape_errors() {
	let text = "[\"\"\"\n  \\q\nbad indentation\n  \"\"\"]";
	let error = text.parse::<Value>().unwrap_err();
	assert!(
		error
			.message()
			.starts_with("This line does not start with the indentation")
	);
	assert_eq!(error.position().unwrap().line, 3);
	for quote in ["'''", "\"\"\""] {
		let text = format!("[{quote}\n  a\n\t\n  b\n  {quote}]");
		assert_eq!(
			text.parse::<Value>().unwrap(),
			Value::Array(vec![Value::from("a\n\nb")])
		);
	}
}

#[test]
fn inline_comments_keep_order_across_every_formatter_gap() {
	let text = "/* outer */ { /* first */ a: [\n /* array */ 1, /* next */\n 2 /* last */\n], /* second */\n b: { /* nested */ c: 3 }, /* closing */\n} /* tail */";
	let document: Document = text.parse().unwrap();
	let formatted = document.format();
	let reread: Document = formatted.parse().unwrap();
	let comments = |document: &Document| {
		document
			.comments()
			.into_iter()
			.map(|comment| comment.text)
			.collect::<Vec<_>>()
	};
	assert_eq!(comments(&reread), comments(&document));
	assert_eq!(reread.to_value().unwrap(), document.to_value().unwrap());
	assert_eq!(reread.format(), formatted);
}

#[test]
fn block_comment_normalization_changes_only_layout() {
	for comment in [
		"/* unchanged\n  changed  \n\n\n final */",
		"/* first \t\n\n last */",
		"/* \n\n\n*/",
		"/* unchanged\nsecond\nlast */",
	] {
		let text = format!("{comment}\nvalue: 1");
		let formatted = soml::format(&text).unwrap();
		assert_eq!(soml::format(&formatted).unwrap(), formatted);
		assert_eq!(
			formatted.parse::<Value>().unwrap(),
			text.parse::<Value>().unwrap()
		);
		assert!(!formatted.lines().any(|line| line.ends_with([' ', '\t'])));
		assert!(!formatted.contains("\n\n\n"));
	}
}

#[test]
fn standard_duration_struct_recognition_accepts_either_field_order() {
	#[derive(Serialize)]
	struct Duration {
		nanos: u32,
		secs: u64,
	}
	let value = [Duration {
		nanos: 500_000_000,
		secs: 1,
	}];
	assert_eq!(soml::to_string(&value).unwrap(), "[\n\t1.5s\n]\n");
	assert_eq!(
		soml::to_string_canonical(&value).unwrap(),
		soml::to_string(&value).unwrap()
	);
}

#[test]
fn paths_are_shortened_before_unicode_controls_are_escaped() {
	let mut document: Document = "a: 1".parse().unwrap();
	let path = std::iter::once("a").chain(std::iter::repeat_n("😀\u{202e}", 100));
	let error = document.remove(path).unwrap_err();
	assert!(error.message().contains("\\u{202e}"));
	assert!(error.message().contains('…'));
	assert!(!error.message().contains('\u{202e}'));
	assert!(error.message().len() < 1000);
	assert_eq!(document.to_string(), "a: 1");
}

#[test]
fn decimal_output_stays_exact_around_every_power_of_ten() {
	let values = [i64::MIN, i64::MAX, 0]
		.into_iter()
		.chain((0..=18).flat_map(|exponent| {
			let power = 10_i64.pow(exponent);
			[power - 1, power, power + 1, -power + 1, -power, -power - 1]
		}));
	for value in values {
		let expected = format!("[\n\t{value}\n]\n");
		assert_eq!(soml::to_string(&[value]).unwrap(), expected);
		assert_eq!(soml::to_string_canonical(&[value]).unwrap(), expected);
		let duration = Duration::from_nanoseconds(value);
		assert_eq!(duration.to_string().parse::<Duration>().unwrap(), duration);
	}
}

#[test]
fn all_integer_key_widths_keep_their_decimal_spelling() {
	macro_rules! check {
		($($type:ty),*) => {
			$(
				for value in [<$type>::MIN, 0, 1, <$type>::MAX] {
					let map = std::collections::BTreeMap::from([(value, true)]);
					let expected = format!("{value}: true\n");
					assert_eq!(soml::to_string(&map).unwrap(), expected);
					assert_eq!(soml::to_string_canonical(&map).unwrap(), expected);
					assert_eq!(soml::from_str::<std::collections::BTreeMap<$type, bool>>(&expected).unwrap(), map);
				}
			)*
		};
	}
	check!(i8, i16, i32, i64, i128, u8, u16, u32, u64, u128);
}

#[test]
fn path_diagnostics_keep_unicode_escape_spelling() {
	let mut document: Document = "a: 1".parse().unwrap();
	for (character, escape) in [
		('\u{85}', "\\u{85}"),
		('\u{ad}', "\\u{ad}"),
		('\u{2028}', "\\u{2028}"),
		('\u{2029}', "\\u{2029}"),
		('\u{202e}', "\\u{202e}"),
		('\u{e0001}', "\\u{e0001}"),
	] {
		let key = format!("before{character}after");
		let error = document.remove(["a", &key]).unwrap_err();
		assert_eq!(
			error.message(),
			format!(
				"Cannot edit a.\"before{escape}after\", because a is not an object or an array"
			)
		);
	}
}

#[test]
fn appending_moves_unicode_comments_without_changing_their_layout() {
	let mut document: Document = "[1 /* é😀 */]".parse().unwrap();
	document.set([1], 2).unwrap();
	assert_eq!(document.to_string(), "[1 /* é😀 */, 2]");
	assert_eq!(document.to_value().unwrap(), soml::soml!([1, 2]));

	let mut document: Document = "[1]".parse().unwrap();
	document
		.root_mut()
		.as_array_mut()
		.unwrap()
		.closing_decor_mut()
		.push_str(" /* é😀 */\n # tail\n");
	document.set([1], 2).unwrap();
	assert_eq!(document.to_string(), "[1 /* é😀 */\n2\n # tail\n]");
	assert_eq!(document.to_value().unwrap(), soml::soml!([1, 2]));
}

#[test]
fn calendar_conversion_matches_chrono_across_gregorian_cycles() {
	for year in [
		1, 4, 100, 400, 401, 1600, 1700, 1900, 1970, 2000, 2400, 9999,
	] {
		for month in [1, 2, 3, 12] {
			for day in [1, 28, 29, 31] {
				let Some(date) = chrono::NaiveDate::from_ymd_opt(year, month, day) else {
					continue;
				};
				let expected = date.and_hms_nano_opt(23, 59, 59, 1).unwrap().and_utc();
				let text = format!("{year:04}-{month:02}-{day:02}T23:59:59.000000001Z");
				let instant = Instant::from_unix(expected.timestamp(), 1).unwrap();
				assert_eq!(instant.to_string(), text);
				assert_eq!(text.parse::<Instant>().unwrap(), instant);
			}
		}
	}
}

#[test]
fn duration_digit_accumulation_checks_the_full_unsigned_carry() {
	for magnitude in [
		i64::MAX as u128,
		1_u128 << 63,
		u64::MAX as u128,
		u64::MAX as u128 + 1,
		u128::MAX,
	] {
		assert_eq!(
			format!("{magnitude}ns")
				.parse::<Duration>()
				.ok()
				.map(Duration::nanoseconds),
			i64::try_from(magnitude).ok(),
		);
		assert_eq!(
			format!("-{magnitude}ns")
				.parse::<Duration>()
				.ok()
				.map(Duration::nanoseconds),
			i128::try_from(magnitude)
				.ok()
				.and_then(|value| i64::try_from(-value).ok()),
		);
	}

	let error = "18446744073709551616ns1s".parse::<Duration>().unwrap_err();
	assert!(error.message().contains("the units are in the order"));
}

#[test]
fn underscored_integer_accumulation_checks_the_full_unsigned_carry() {
	for magnitude in [
		i64::MAX as u128,
		1_u128 << 63,
		u64::MAX as u128,
		u64::MAX as u128 + 1,
		u128::MAX,
	] {
		for (prefix, digits) in [
			("", magnitude.to_string()),
			("0b", format!("{magnitude:b}")),
			("0o", format!("{magnitude:o}")),
			("0x", format!("{magnitude:X}")),
		] {
			let text = format!("[{prefix}{}_{}]", &digits[..1], &digits[1..]);
			assert_eq!(
				soml::from_str::<Vec<i64>>(&text).ok(),
				i64::try_from(magnitude).ok().map(|value| vec![value]),
				"{text}",
			);
		}

		let digits = magnitude.to_string();
		let text = format!("[-{}_{}]", &digits[..1], &digits[1..]);
		assert_eq!(
			soml::from_str::<Vec<i64>>(&text).ok(),
			i128::try_from(magnitude)
				.ok()
				.and_then(|value| i64::try_from(-value).ok())
				.map(|value| vec![value]),
			"{text}",
		);
	}
}
