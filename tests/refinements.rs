use serde::{Deserialize, Serialize};
use soml::{Document, Duration, ErrorKind, Instant, PathSegment, Value};

#[derive(Debug, Deserialize, PartialEq)]
struct Settings {
	first: u32,
	second: bool,
	third: String,
}

#[test]
fn ignored_collections_preserve_validation_and_the_next_field() {
	let nested =
		"[{a: [null, false, -infinity, 1_000.5, 1h, 2026-01-01T00:00:00Z, \"\\u{1f600}\"]}, {}]";
	for text in [
		format!("first: 7\nunused: {nested}\nsecond: true\nthird: 'done'"),
		format!("{{unused: {nested}, first: 7, second: true, third: 'done'}}"),
	] {
		assert_eq!(
			soml::from_str::<Settings>(&text).unwrap(),
			Settings {
				first: 7,
				second: true,
				third: "done".to_owned()
			}
		);
	}
	for invalid in [
		"{a: 1, 'a': 2}",
		"[1 2]",
		"[1,] junk",
		"[1, 1e999]",
		"[1, \"\\r\"]",
	] {
		let text = format!("first: 7\nunused: {invalid}\nsecond: true\nthird: 'done'");
		let error = soml::from_str::<Settings>(&text).unwrap_err();
		let reference = text.parse::<Value>().unwrap_err();
		assert_eq!(error.to_string(), reference.to_string(), "{text}");
		assert_eq!(error.offset(), reference.offset());
	}
}

#[test]
fn ignored_collections_enforce_the_exact_depth_limit() {
	for (depth, accepted) in [(99, true), (100, false)] {
		let text = format!(
			"first: 7\nunused: {}0{}\nsecond: true\nthird: 'done'",
			"[".repeat(depth),
			"]".repeat(depth)
		);
		assert_eq!(soml::from_str::<Settings>(&text).is_ok(), accepted);
		assert_eq!(
			soml::from_str::<serde::de::IgnoredAny>(&text).is_ok(),
			accepted
		);
	}
}

#[test]
fn hostile_quote_runs_have_bounded_diagnostics() {
	let delimiter = "'".repeat(100_000);
	let text = format!("a: {delimiter}\ncontent{delimiter}");
	let error = text.parse::<Value>().unwrap_err();
	assert_eq!(error.kind(), ErrorKind::Syntax);
	assert!(
		error
			.message()
			.contains("closing delimiter must start a line")
	);
	assert!(error.message().len() < 300, "{}", error.message().len());
	assert!(error.message().contains('…'));
}

#[test]
fn huge_duration_parts_still_report_later_syntax_errors_first() {
	let magnitude = "9".repeat(100_000);
	let error = format!("{magnitude}h1X").parse::<Duration>().unwrap_err();
	assert!(error.message().contains("is not a unit"));
	assert!(error.message().len() < 200);
	let error = format!("{magnitude}h").parse::<Duration>().unwrap_err();
	assert!(error.message().contains("outside the 64-bit range"));
}

#[test]
fn huge_unknown_words_and_paths_have_short_safe_errors() {
	let word = "N".repeat(100_000);
	let error = format!("a: {word}").parse::<Value>().unwrap_err();
	assert!(error.message().len() < 200);
	let mut document: Document = "a: 1".parse().unwrap();
	let key = "😀\u{202e}quoted\"name";
	let mut path = vec![PathSegment::Key("a")];
	path.extend(std::iter::repeat_n(PathSegment::Key(key), 10_000));
	let before = document.to_string();
	let error = document.remove(path).unwrap_err();
	assert!(error.message().len() < 1500);
	assert!(error.message().contains("\\u{202e}"));
	assert!(!error.message().contains('\u{202e}'));
	assert_eq!(document.to_string(), before);
}

#[test]
fn quoted_key_boundaries_survive_long_chunks_and_escaped_quotes() {
	let prefix = "é😀".repeat(10_000);
	let key = format!("{prefix}\"colon:slash\\");
	let text = format!("\"{prefix}\\\"colon:slash\\\\\": [1] # owned\nnext: 2");
	let document: Document = text.parse().unwrap();
	assert_eq!(document.to_string(), text);
	let member = &document.root().as_object().unwrap().members()[0];
	assert_eq!(member.key().value(), key);
	let span = member.key().span().unwrap();
	assert_eq!(member.key().raw(), Some(&text[span.clone()]));
	assert_eq!(&text[span.end..span.end + 2], ": ");
	assert_eq!(
		document.to_value().unwrap().get(&key).unwrap().get(0),
		Some(&Value::Int(1))
	);
}

#[test]
fn direct_value_validation_preserves_error_precedence() {
	for value in [
		Value::Array(vec![Value::Float(f64::NAN)]),
		Value::Object(soml::Object::from([(
			"bad\rkey".to_owned(),
			Value::Float(f64::NAN),
		)])),
		Value::Object(soml::Object::from([(
			"bad\rkey".to_owned(),
			Value::String("bad\rvalue".to_owned()),
		)])),
	] {
		assert_eq!(
			soml::tree::Node::new(value.clone()).unwrap_err().message(),
			soml::to_value(&value).unwrap_err().message()
		);
	}
}

#[test]
fn one_line_replacements_and_failed_changes_keep_comments() {
	let mut document: Document = "a: [0 /* item */, {old: 1}] # outer\n".parse().unwrap();
	document
		.set(
			[PathSegment::Key("a"), PathSegment::Index(1)],
			soml::soml!({new: [1, {nested: "é\n'"}]}),
		)
		.unwrap();
	let text = document.to_string();
	assert!(text.contains("/* item */"));
	assert!(text.ends_with("# outer\n"));
	assert_eq!(text.lines().count(), 1);
	let expected = document.to_value().unwrap();
	let deep = (0..100).fold(Value::Null, |value, _| Value::Array(vec![value]));
	assert!(document.set(["a"], deep).is_err());
	assert_eq!(document.to_string(), text);
	assert_eq!(document.to_value().unwrap(), expected);
}

#[test]
fn removing_an_entry_keeps_the_surviving_nodes_and_their_spans() {
	let text = "[/* opening */\n  10, /* middle */\n  20,\n  30 /* closing */]\n";
	let mut document: Document = text.parse().unwrap();
	let span = document.get([2]).unwrap().span();
	assert!(document.remove([1]).unwrap());
	assert_eq!(document.get([1]).unwrap().span(), span);
	assert_eq!(
		document.to_value().unwrap(),
		Value::Array(vec![10.into(), 30.into()])
	);
	assert!(document.to_string().contains("/* opening */"));
	assert!(document.to_string().contains("/* closing */"));
	assert_eq!(
		document.format(),
		soml::format(&document.to_string()).unwrap()
	);
}

#[test]
fn format_preserves_comment_order_across_joined_decor() {
	let text = "{a: 1 /* before comma */ , /* after comma */\n# next\nb: [2, /* inline */ 3], /* closing */\n}";
	let document: Document = text.parse().unwrap();
	let formatted = document.format();
	let reread: Document = formatted.parse().unwrap();
	let comments = |document: &Document| {
		document
			.comments()
			.into_iter()
			.map(|comment| comment.text.trim().to_owned())
			.collect::<Vec<_>>()
	};
	assert_eq!(comments(&document), comments(&reread));
	assert_eq!(reread.to_value().unwrap(), document.to_value().unwrap());
	assert_eq!(reread.format(), formatted);
}

#[test]
fn constant_getters_work_in_constant_expressions() {
	const BEFORE_EPOCH: Instant = match Instant::from_unix_nanoseconds(-1) {
		Some(instant) => instant,
		None => panic!("a nanosecond before the epoch is valid"),
	};
	static EMPTY_STRING: Value = Value::String(String::new());
	const TEXT: Option<&str> = EMPTY_STRING.as_str();
	const TOO_EARLY: Option<Instant> = Instant::from_unix_nanoseconds(i128::MIN);
	assert_eq!(BEFORE_EPOCH.to_string(), "1969-12-31T23:59:59.999999999Z");
	assert_eq!(TEXT, Some(""));
	assert_eq!(TOO_EARLY, None);
}

#[test]
fn system_time_conversion_is_exact_on_both_sides_of_the_epoch() {
	use std::time::{Duration as StdDuration, SystemTime, UNIX_EPOCH};
	for duration in [
		StdDuration::from_nanos(1),
		StdDuration::new(1, 999_999_999),
		StdDuration::new(100_000, 500_000_000),
	] {
		for time in [UNIX_EPOCH + duration, UNIX_EPOCH - duration] {
			let instant = Instant::try_from(time).unwrap();
			assert_eq!(SystemTime::try_from(instant).unwrap(), time);
		}
	}
}

#[test]
fn collect_str_preserves_escaping_and_checks_representability() {
	struct Text(&'static str);
	impl std::fmt::Display for Text {
		fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
			formatter.write_str(self.0)
		}
	}
	impl Serialize for Text {
		fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
			serializer.collect_str(self)
		}
	}
	let value = [Text("é\n'\\\t")];
	let text = soml::to_string_canonical(&value).unwrap();
	assert_eq!(soml::from_str::<Vec<String>>(&text).unwrap(), ["é\n'\\\t"]);
	assert_eq!(soml::to_string(&value).unwrap(), text);
	assert_eq!(
		soml::to_value(&[Text("\r")]).unwrap_err().kind(),
		ErrorKind::Write
	);
}

#[test]
fn type_errors_abbreviate_large_strings_before_formatting_them() {
	#[derive(Debug, Deserialize)]
	#[allow(dead_code)]
	struct Number {
		value: u32,
	}
	let text = format!("value: '{}'", "😀".repeat(100_000));
	let error = soml::from_str::<Number>(&text).unwrap_err();
	assert_eq!(error.kind(), ErrorKind::Data);
	assert_eq!(error.offset(), Some(7));
	assert!(error.message().contains('…'));
	assert!(error.message().len() < 200);
}

#[test]
fn duplicate_serialized_keys_have_bounded_errors() {
	struct Duplicate;
	impl Serialize for Duplicate {
		fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
			use serde::ser::SerializeMap;
			let key = "x".repeat(100_000);
			let mut map = serializer.serialize_map(Some(2))?;
			map.serialize_entry(&key, &1)?;
			map.serialize_entry(&key, &2)?;
			map.end()
		}
	}
	for error in [
		soml::to_string(&Duplicate).unwrap_err(),
		soml::to_value(&Duplicate).unwrap_err(),
	] {
		assert_eq!(error.kind(), ErrorKind::Write);
		assert!(error.message().contains("Duplicate key"));
		assert!(error.message().contains('…'));
		assert!(error.message().len() < 100);
	}
}
