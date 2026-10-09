/*!
`soml::Instant` and `soml::Duration`: construction, text forms, ordering, and conversions with the standard library.
*/

#![allow(clippy::tabs_in_doc_comments)]

use soml::{Duration, Instant};
use std::time::{Duration as StdDuration, SystemTime, UNIX_EPOCH};

fn instant(text: &str) -> Instant {
	text.parse()
		.unwrap_or_else(|error| panic!("{text:?} should be an instant, but: {error}"))
}

fn duration(text: &str) -> Duration {
	text.parse()
		.unwrap_or_else(|error| panic!("{text:?} should be a duration, but: {error}"))
}

// Instant

#[test]
fn an_instant_is_unix_seconds_and_nanoseconds() {
	let value = instant("2026-09-19T21:00:00.5+07:00");
	assert_eq!(value.unix_seconds(), 1_789_826_400);
	assert_eq!(value.nanoseconds(), 500_000_000);
	assert_eq!(value.to_string(), "2026-09-19T14:00:00.5Z");
}

#[test]
fn an_instant_before_1970_has_negative_seconds_and_positive_nanoseconds() {
	let value = instant("1969-12-31T23:59:59.25Z");
	assert_eq!(value.unix_seconds(), -1);
	assert_eq!(value.nanoseconds(), 250_000_000);
	assert_eq!(Instant::from_unix(-1, 250_000_000), Some(value));
}

#[test]
fn from_unix_checks_the_range() {
	assert_eq!(
		Instant::from_unix(0, 0).map(|value| value.to_string()),
		Some(String::from("1970-01-01T00:00:00Z"))
	);
	assert_eq!(Instant::from_unix(-62_135_596_800, 0), Some(Instant::MIN));
	assert_eq!(
		Instant::from_unix(253_402_300_799, 999_999_999),
		Some(Instant::MAX)
	);
	assert_eq!(Instant::from_unix(-62_135_596_801, 999_999_999), None);
	assert_eq!(Instant::from_unix(253_402_300_800, 0), None);
	assert_eq!(Instant::from_unix(0, 1_000_000_000), None);
}

#[test]
fn unix_nanoseconds_round_trip_and_check_the_range() {
	for value in [
		Instant::MIN,
		Instant::MAX,
		instant("1969-12-31T23:59:59.999999999Z"),
		instant("1970-01-01T00:00:00Z"),
	] {
		assert_eq!(
			Instant::from_unix_nanoseconds(value.unix_nanoseconds()),
			Some(value)
		);
	}

	assert_eq!(
		instant("1969-12-31T23:59:59.999999999Z").unix_nanoseconds(),
		-1
	);
	assert_eq!(
		Instant::from_unix_nanoseconds(Instant::MIN.unix_nanoseconds() - 1),
		None
	);
	assert_eq!(
		Instant::from_unix_nanoseconds(Instant::MAX.unix_nanoseconds() + 1),
		None
	);
	assert_eq!(Instant::from_unix_nanoseconds(i128::MIN), None);
	assert_eq!(Instant::from_unix_nanoseconds(i128::MAX), None);
}

#[test]
fn the_instant_range_is_written_canonically() {
	assert_eq!(Instant::MIN.to_string(), "0001-01-01T00:00:00Z");
	assert_eq!(Instant::MAX.to_string(), "9999-12-31T23:59:59.999999999Z");
}

#[test]
fn two_spellings_of_one_instant_are_equal() {
	assert_eq!(
		instant("2026-09-19T21:00:00+07:00"),
		instant("2026-09-19T14:00:00Z")
	);
	assert_eq!(
		instant("2026-09-19T14:00:00.000Z"),
		instant("2026-09-19T14:00:00Z")
	);
	assert_eq!(
		instant("2026-09-19T08:30:00-05:30"),
		instant("2026-09-19T14:00:00Z")
	);
}

#[test]
fn instants_are_ordered_by_time() {
	assert!(instant("2026-09-19T14:00:00Z") < instant("2026-09-19T14:00:00.000000001Z"));
	assert!(instant("2026-09-19T15:00:00+02:00") < instant("2026-09-19T14:00:00Z"));
	assert!(Instant::MIN < Instant::MAX);
}

#[test]
fn every_day_of_a_leap_year_and_a_common_year_round_trips() {
	for year in [1900, 2000, 2024, 2026] {
		let first = instant(&format!("{year:04}-01-01T00:00:00Z")).unix_seconds();
		let next = instant(&format!("{:04}-01-01T00:00:00Z", year + 1)).unix_seconds();
		let days = (next - first) / 86_400;
		let is_leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
		assert_eq!(days, if is_leap { 366 } else { 365 }, "{year}");

		for day in 0..days {
			let value = Instant::from_unix(first + day * 86_400, 0).expect("in range");
			assert_eq!(instant(&value.to_string()), value);
		}
	}
}

#[test]
fn the_february_29_rule() {
	assert!("2000-02-29T00:00:00Z".parse::<Instant>().is_ok());
	assert!("2024-02-29T00:00:00Z".parse::<Instant>().is_ok());
	assert!("1900-02-29T00:00:00Z".parse::<Instant>().is_err());
	assert!("2026-02-29T00:00:00Z".parse::<Instant>().is_err());
}

#[test]
fn instant_from_str_rejects_anything_that_is_not_exactly_an_instant() {
	for text in [
		"",
		"2026-09-19",
		" 2026-09-19T14:00:00Z",
		"2026-09-19T14:00:00Z ",
		"2026-09-19T14:00:00.Z",
		"2026-9-19T14:00:00Z",
		"2026-09-19T14:00:00+7:00",
		"'2026-09-19T14:00:00Z'",
	] {
		assert!(text.parse::<Instant>().is_err(), "{text:?}");
	}

	assert_eq!(
		"2026-09-19T14:00:00"
			.parse::<Instant>()
			.expect_err("no offset")
			.to_string(),
		"An instant needs an offset: Z or ±HH:MM"
	);
}

#[test]
fn an_instant_converts_to_and_from_system_time() {
	let value = instant("2026-09-19T14:00:00.123456789Z");
	let time = SystemTime::try_from(value).expect("a SystemTime holds it");
	assert_eq!(
		time.duration_since(UNIX_EPOCH).expect("after 1970"),
		StdDuration::new(1_789_826_400, 123_456_789)
	);
	assert_eq!(Instant::try_from(time).expect("in range"), value);

	let before = instant("1969-12-31T23:59:59.25Z");
	let time = SystemTime::try_from(before).expect("a SystemTime holds it");
	assert_eq!(
		UNIX_EPOCH.duration_since(time).expect("before 1970"),
		StdDuration::from_millis(750)
	);
	assert_eq!(Instant::try_from(time).expect("in range"), before);

	// A whole number of seconds before 1970, and both ends of the range.
	for value in [instant("1969-12-31T23:59:59Z"), Instant::MIN, Instant::MAX] {
		let time = SystemTime::try_from(value).expect("a SystemTime holds it");
		assert_eq!(Instant::try_from(time).expect("in range"), value);
	}
}

#[test]
fn a_system_time_outside_the_instant_range_is_an_error() {
	let time = UNIX_EPOCH + StdDuration::from_secs(253_402_300_800);
	assert_eq!(
		Instant::try_from(time).expect_err("year 10000").to_string(),
		"The time is outside the years 0001 to 9999"
	);
}

// Duration

#[test]
fn a_duration_is_a_signed_count_of_nanoseconds() {
	assert_eq!(duration("1h30m").nanoseconds(), 5_400_000_000_000);
	assert_eq!(duration("-1.5s").nanoseconds(), -1_500_000_000);
	assert!(duration("-1ns").is_negative());
	assert!(!duration("0s").is_negative());
	assert_eq!(Duration::ZERO, duration("0s"));
	assert_eq!(Duration::default(), Duration::ZERO);
	assert_eq!(Duration::MIN.nanoseconds(), i64::MIN);
	assert_eq!(Duration::MAX.nanoseconds(), i64::MAX);
}

#[test]
fn a_duration_is_written_canonically() {
	assert_eq!(duration("90m").to_string(), "1h30m");
	assert_eq!(duration("1500ms").to_string(), "1.5s");
	assert_eq!(duration("250us").to_string(), "0.00025s");
	assert_eq!(duration("0ns").to_string(), "0s");
	assert_eq!(duration("3600s").to_string(), "1h");
	assert_eq!(duration("61s").to_string(), "1m1s");
	assert_eq!(duration("1h0.5s").to_string(), "1h0.5s");
	assert_eq!(Duration::MIN.to_string(), "-2562047h47m16.854775808s");
}

#[test]
fn durations_are_ordered_by_length() {
	assert!(duration("-1h") < duration("0s"));
	assert!(duration("59m") < duration("1h"));
	assert_eq!(duration("1.5h"), duration("90m"));
}

#[test]
fn duration_from_str_rejects_anything_that_is_not_exactly_a_duration() {
	for text in [
		"1", "1 s", " 1s", "1s ", "-", "+1s", "1h1h", "1.5h30m", "1µs", "'1s'",
	] {
		assert!(text.parse::<Duration>().is_err(), "{text:?}");
	}

	assert_eq!(
		"1d".parse::<Duration>()
			.expect_err("no day unit")
			.to_string(),
		"Invalid duration 1d: there is no day unit, because a day is not a fixed length. Write 24h for a fixed 24 hours"
	);
}

#[test]
fn duration_from_str_rejects_an_empty_string() {
	// The grammar needs at least one part: `duration = [ "-" ] 1*( int-part [ "." digits ] unit )`.
	assert!("".parse::<Duration>().is_err());
	// Other formats read `soml::Duration` from text, where an empty string would otherwise become 0s.
	assert!(serde_json::from_str::<Duration>("\"\"").is_err());
}

#[test]
fn a_duration_converts_to_and_from_std_duration() {
	assert_eq!(
		StdDuration::try_from(duration("1.5s")).expect("not negative"),
		StdDuration::from_millis(1500)
	);
	assert_eq!(
		Duration::try_from(StdDuration::new(90, 1)).expect("in range"),
		duration("1m30.000000001s")
	);
	assert_eq!(
		Duration::try_from(StdDuration::from_nanos(i64::MAX.unsigned_abs())).expect("in range"),
		Duration::MAX
	);
}

#[test]
fn a_negative_duration_does_not_convert_to_std_duration() {
	assert_eq!(
		StdDuration::try_from(duration("-5m"))
			.expect_err("negative")
			.to_string(),
		"The duration -5m is negative, which a std::time::Duration cannot be"
	);
}

#[test]
fn a_std_duration_too_long_for_soml_is_an_error() {
	assert_eq!(
		Duration::try_from(StdDuration::from_nanos(i64::MAX.unsigned_abs() + 1))
			.expect_err("too long")
			.to_string(),
		"The duration 9223372036.854775808s is outside the SOML range of about 292 years"
	);
}
