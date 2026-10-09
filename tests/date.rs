use serde::{Deserialize, Serialize};
use soml::{Date, Value};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Schedule {
	date: Date,
	label: String,
	dates: Vec<Date>,
	optional: Option<Date>,
}

#[test]
fn native_dates_preserve_exact_components_and_type() {
	for (year, month, day, text) in [
		(1, 1, 1, "0001-01-01"),
		(9999, 12, 31, "9999-12-31"),
		(1582, 10, 10, "1582-10-10"),
		(2011, 12, 30, "2011-12-30"),
		(2000, 2, 29, "2000-02-29"),
		(2024, 2, 29, "2024-02-29"),
	] {
		let date = Date::new(year, month, day).expect("valid Gregorian components");
		assert_eq!((date.year(), date.month(), date.day()), (year, month, day));
		assert_eq!(date.to_string(), text);
		assert_eq!(text.parse::<Date>().expect("valid"), date);
		let value: Value = format!("[{text}, '{text}']").parse().expect("valid");
		assert_eq!(value.get(0).expect("item"), &Value::Date(date));
		assert_eq!(value.get(0).expect("item").as_date(), Some(date));
		assert_eq!(value.get(1).expect("item").as_str(), Some(text));
		assert_eq!(
			soml::to_string(&vec![date]).expect("written"),
			format!("[\n\t{text}\n]\n")
		);
	}
	for (year, month, day) in [
		(0, 1, 1),
		(10000, 1, 1),
		(2026, 0, 1),
		(2026, 13, 1),
		(1900, 2, 29),
		(2026, 2, 30),
		(2026, 4, 31),
		(2026, 1, 0),
	] {
		assert!(Date::new(year, month, day).is_none());
	}
}

#[test]
fn strict_whole_tokens_reject_impossible_dates_and_suffixes() {
	for text in [
		"0000-01-01",
		"1900-02-29",
		"2026-02-30",
		"2026-1-01",
		"10000-01-01",
		"2026-11-01T",
		"2026-11-01Z",
		"2026-11-01foo",
		"2026-11-01T12:00:00",
		"２０２６-11-01",
	] {
		assert!(text.parse::<Date>().is_err());
		assert!(soml::from_str::<Value>(&format!("date: {text}")).is_err());
		assert!(soml::format(&format!("date: {text}")).is_err());
	}
	assert!(soml::from_str::<Value>("2026-11-01").is_err());
	let error = soml::from_str::<Value>("date: 2026-02-30").expect_err("invalid");
	assert_eq!(
		error.to_string(),
		"Invalid date 2026-02-30: the day must be 01 to 28 in that month at line 1, column 7"
	);
}

#[test]
fn typed_and_value_serde_keep_dates_distinct() {
	let date: Date = "1582-10-10".parse().expect("valid");
	let expected = Schedule {
		date,
		label: "1582-10-10".to_owned(),
		dates: vec![date],
		optional: Some(date),
	};
	let source = "date: 1582-10-10\nlabel: '1582-10-10'\ndates: [1582-10-10]\noptional: 1582-10-10";
	assert_eq!(
		soml::from_str::<Schedule>(source).expect("native dates"),
		expected
	);
	let tree = soml::to_value(&expected).expect("written");
	assert_eq!(*tree.get("date").expect("date"), Value::Date(date));
	assert_eq!(
		soml::from_value::<Schedule>(tree).expect("decoded"),
		expected
	);
	assert_eq!(
		soml::from_str::<Schedule>(&soml::to_string(&expected).expect("written")).expect("decoded"),
		expected
	);
	assert!(soml::from_str::<Vec<Date>>("['1582-10-10']").is_err());
	assert!(soml::from_str::<Vec<Date>>("[1582-10-10T00:00:00Z]").is_err());
	assert!(soml::from_str::<Vec<String>>("[1582-10-10]").is_err());
	assert!(soml::from_str::<Vec<soml::Instant>>("[1582-10-10]").is_err());
	assert_eq!(
		serde_json::to_string(&date).expect("JSON"),
		"\"1582-10-10\""
	);
	assert_eq!(
		serde_json::from_str::<Date>("\"1582-10-10\"").expect("JSON"),
		date
	);
}

#[test]
fn boundaries_comments_formatting_and_editing_keep_native_dates() {
	for source in [
		"date: 1582-10-10",
		"date: 1582-10-10\nnext: 1",
		"date: 1582-10-10# keep\nnext: 1",
		"date: 1582-10-10/* keep */",
		"{date: 1582-10-10, next: 1}",
	] {
		let value: Value = soml::from_str(source).expect("valid");
		assert!(value.get("date").expect("date").as_date().is_some());
		assert_eq!(
			value,
			soml::from_slice::<Value>(source.as_bytes()).expect("UTF-8")
		);
		assert_eq!(
			value,
			soml::from_str::<Value>(&soml::format(source).expect("formatted")).expect("valid")
		);
	}
	let mut document: soml::Document = "date: 1582-10-10 # Calendar\ntext: '1582-10-10'\n"
		.parse()
		.expect("valid");
	document
		.set(["date"], Date::new(2000, 2, 29).expect("valid"))
		.expect("edited");
	assert_eq!(
		document.to_string(),
		"date: 2000-02-29 # Calendar\ntext: '1582-10-10'\n"
	);
	assert!(
		document
			.to_value()
			.expect("tree")
			.get("date")
			.expect("date")
			.as_date()
			.is_some()
	);
}

#[cfg(feature = "chrono")]
#[test]
fn chrono_dates_use_explicit_native_adapters_without_an_instant() {
	#[derive(Debug, PartialEq, Serialize, Deserialize)]
	struct Calendar {
		#[serde(with = "soml::chrono::naive_date")]
		day: chrono::NaiveDate,
	}
	for text in ["0001-01-01", "9999-12-31", "1582-10-10", "2011-12-30"] {
		let date: Date = text.parse().expect("valid");
		let native = chrono::NaiveDate::from(date);
		assert_eq!(Date::try_from(native).expect("in range"), date);
		let calendar = Calendar { day: native };
		assert_eq!(
			soml::to_string(&calendar).expect("written"),
			format!("day: {text}\n")
		);
		assert_eq!(
			soml::from_str::<Calendar>(&format!("day: {text}")).expect("read"),
			calendar
		);
	}
	assert!(
		Date::try_from(chrono::NaiveDate::from_ymd_opt(0, 1, 1).expect("chrono year zero"))
			.is_err()
	);
	assert!(soml::from_str::<Calendar>("day: '1582-10-10'").is_err());
}

#[cfg(feature = "jiff")]
#[test]
fn jiff_dates_use_explicit_native_adapters_without_an_instant() {
	#[derive(Debug, PartialEq, Serialize, Deserialize)]
	struct Calendar {
		#[serde(with = "soml::jiff::date")]
		day: jiff::civil::Date,
	}
	for text in ["0001-01-01", "9999-12-31", "1582-10-10", "2011-12-30"] {
		let date: Date = text.parse().expect("valid");
		let native = jiff::civil::Date::from(date);
		assert_eq!(Date::try_from(native).expect("in range"), date);
		let calendar = Calendar { day: native };
		assert_eq!(
			soml::to_string(&calendar).expect("written"),
			format!("day: {text}\n")
		);
		assert_eq!(
			soml::from_str::<Calendar>(&format!("day: {text}")).expect("read"),
			calendar
		);
	}
	assert!(Date::try_from(jiff::civil::Date::new(0, 1, 1).expect("jiff year zero")).is_err());
	assert!(soml::from_str::<Calendar>("day: '1582-10-10'").is_err());
}

#[cfg(feature = "chrono")]
#[test]
fn all_components_in_a_gregorian_cycle_match_an_independent_calendar() {
	let mut valid_count = 0;
	for year in 1600..2000 {
		for month in 1..=12 {
			for day in 0..=32 {
				let expected = chrono::NaiveDate::from_ymd_opt(year, month, day);
				let date = Date::new(year as u16, month as u8, day as u8);
				assert_eq!(date.is_some(), expected.is_some(), "{year}-{month}-{day}");
				let text = format!("{year:04}-{month:02}-{day:02}");
				assert_eq!(text.parse::<Date>().is_ok(), expected.is_some(), "{text}");
				assert_eq!(
					soml::from_str::<Vec<Date>>(&format!("[{text}]")).is_ok(),
					expected.is_some(),
					"{text}"
				);
				if let Some(date) = date {
					assert_eq!(date.to_string(), text);
					assert_eq!(chrono::NaiveDate::from(date), expected.expect("valid"));
					valid_count += 1;
				}
			}
		}
	}
	assert_eq!(valid_count, 146_097);
}

#[test]
fn component_equality_ordering_hash_and_canonical_format_are_stable() {
	use std::collections::HashSet;
	let early = Date::new(1582, 10, 10).expect("valid");
	let later = Date::new(1582, 10, 11).expect("valid");
	assert!(early < later);
	assert_eq!(HashSet::from([early, early, later]).len(), 2);
	assert_eq!(format!("{early:>20.2}"), "1582-10-10");
	assert_eq!(format!("{early:?}"), "Date(1582-10-10)");
	assert_ne!(Value::Date(early), Value::String(early.to_string()));
	assert_ne!(
		Value::Date(early),
		Value::Instant("1582-10-10T00:00:00Z".parse().expect("instant"))
	);
	assert_eq!(
		soml::to_string_canonical(&vec![early]).expect("canonical"),
		"[\n\t1582-10-10\n]\n"
	);
}

#[test]
fn typed_mismatches_json_and_long_untrusted_tokens_are_strict() {
	for source in [
		"[true]",
		"[1]",
		"[1.0]",
		"[1s]",
		"['2000-02-29']",
		"[2000-02-29T00:00:00Z]",
	] {
		assert!(soml::from_str::<Vec<Date>>(source).is_err(), "{source}");
	}
	for text in [
		"0000-01-01",
		"1900-02-29",
		"2026-01-01T00:00:00Z",
		"2026-01-01[u-ca=gregory]",
		"２０２６-01-01",
	] {
		assert!(
			serde_json::from_value::<Date>(serde_json::json!(text)).is_err(),
			"{text}"
		);
	}
	for suffix in [
		"x".repeat(100_000),
		"é".repeat(50_000),
		"T00:00:00".to_owned(),
		"\0".to_owned(),
	] {
		let text = format!("2026-11-01{suffix}");
		assert!(text.parse::<Date>().is_err());
		assert!(soml::from_str::<Value>(&format!("date: {text}")).is_err());
	}
	let error = soml::from_str::<Vec<bool>>("[1582-10-10]").expect_err("wrong type");
	assert!(error.message().contains("a date"), "{error}");
}

#[cfg(feature = "chrono")]
#[test]
fn chrono_optional_dates_and_out_of_range_years_preserve_type() {
	#[derive(Debug, PartialEq, Serialize, Deserialize)]
	struct Optional {
		#[serde(with = "soml::chrono::naive_date::option", default)]
		day: Option<chrono::NaiveDate>,
	}
	for day in [
		None,
		Some(chrono::NaiveDate::from_ymd_opt(1582, 10, 10).expect("valid")),
	] {
		let expected = Optional { day };
		let source = soml::to_string(&expected).expect("write");
		assert_eq!(soml::from_str::<Optional>(&source).expect("read"), expected);
		assert_eq!(
			serde_json::from_str::<Optional>(&serde_json::to_string(&expected).expect("JSON"))
				.expect("JSON"),
			expected
		);
	}
	for year in [-1, 0, 10000, 65536] {
		let day = chrono::NaiveDate::from_ymd_opt(year, 1, 1).expect("chrono range");
		assert!(Date::try_from(day).is_err());
		assert!(soml::to_string(&Optional { day: Some(day) }).is_err());
	}
}

#[cfg(feature = "jiff")]
#[test]
fn jiff_optional_dates_and_negative_years_preserve_type() {
	#[derive(Debug, PartialEq, Serialize, Deserialize)]
	struct Optional {
		#[serde(with = "soml::jiff::date::option", default)]
		day: Option<jiff::civil::Date>,
	}
	for day in [
		None,
		Some(jiff::civil::Date::new(1582, 10, 10).expect("valid")),
	] {
		let expected = Optional { day };
		assert_eq!(
			soml::from_str::<Optional>(&soml::to_string(&expected).expect("write")).expect("read"),
			expected
		);
		assert_eq!(
			serde_json::from_str::<Optional>(&serde_json::to_string(&expected).expect("JSON"))
				.expect("JSON"),
			expected
		);
	}
	for year in [-9999, -1, 0] {
		let day = jiff::civil::Date::new(year, 1, 1).expect("jiff range");
		assert!(Date::try_from(day).is_err());
		assert!(soml::to_string(&Optional { day: Some(day) }).is_err());
	}
}
