/*!
serde support: every row of the type mapping in the plan, both ways, and the interop with jiff, chrono, and `serde_json`.
*/

#![allow(clippy::tabs_in_doc_comments)]

use serde::{Deserialize, Serialize};
use soml::Value;
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};

/**
Reads the member `a` of a document into `T`.
*/
fn read<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, soml::Error> {
	#[derive(Deserialize)]
	struct Wrapper<T> {
		a: T,
	}

	soml::from_str::<Wrapper<T>>(text).map(|wrapper| wrapper.a)
}

/**
The error of reading the member `a` of a document into `T`, as `Display` writes it.
*/
fn read_error<T: serde::de::DeserializeOwned + std::fmt::Debug>(text: &str) -> String {
	match read::<T>(text) {
		Ok(value) => panic!("{text:?} should not read, but read as {value:?}"),
		Err(error) => error.to_string(),
	}
}

/**
Writes `value` as the member `a` of a document.
*/
fn write<T: Serialize>(value: T) -> Result<String, soml::Error> {
	#[derive(Serialize)]
	struct Wrapper<T> {
		a: T,
	}

	soml::to_string(&Wrapper { a: value })
}

/**
The canonical text of `value` as the member `a`.
*/
fn written<T: Serialize>(value: T) -> String {
	let text = write(value).expect("the value can be written");

	text.strip_prefix("a: ")
		.and_then(|text| text.strip_suffix('\n'))
		.unwrap_or_else(|| panic!("unexpected document {text:?}"))
		.to_owned()
}

fn write_error<T: Serialize>(value: T) -> String {
	match write(value) {
		Ok(text) => panic!("the value should not be written, but was written as {text:?}"),
		Err(error) => error.to_string(),
	}
}

// bool

#[test]
fn a_bool_reads_and_writes() {
	assert!(read::<bool>("a: true").expect("a bool"));
	assert!(!read::<bool>("a: false").expect("a bool"));
	assert_eq!(written(true), "true");
	assert_eq!(
		read_error::<bool>("a: 1"),
		"invalid type: integer `1`, expected a boolean at line 1, column 4"
	);
	assert_eq!(
		read_error::<bool>("a: 'true'"),
		"invalid type: string \"true\", expected a boolean at line 1, column 4"
	);
}

// Integers

#[test]
fn every_integer_type_reads_within_its_range() {
	assert_eq!(read::<i8>("a: -128").expect("i8"), i8::MIN);
	assert_eq!(read::<i16>("a: 32767").expect("i16"), i16::MAX);
	assert_eq!(read::<i32>("a: -2147483648").expect("i32"), i32::MIN);
	assert_eq!(
		read::<i64>("a: -9223372036854775808").expect("i64"),
		i64::MIN
	);
	assert_eq!(read::<u8>("a: 255").expect("u8"), u8::MAX);
	assert_eq!(read::<u16>("a: 0xFFFF").expect("u16"), u16::MAX);
	assert_eq!(read::<u32>("a: 4294967295").expect("u32"), u32::MAX);
	assert_eq!(
		read::<u64>("a: 9223372036854775807").expect("u64"),
		9_223_372_036_854_775_807
	);
	assert_eq!(
		read::<i128>("a: -9223372036854775808").expect("i128"),
		i128::from(i64::MIN)
	);
	assert_eq!(
		read::<u128>("a: 9223372036854775807").expect("u128"),
		9_223_372_036_854_775_807
	);
	assert_eq!(read::<usize>("a: 7").expect("usize"), 7);
}

#[test]
fn an_integer_outside_the_type_range_is_an_error() {
	assert_eq!(
		read_error::<u8>("a: 256"),
		"invalid value: integer `256`, expected u8 at line 1, column 4"
	);
	assert_eq!(
		read_error::<i8>("a: -129"),
		"invalid value: integer `-129`, expected i8 at line 1, column 4"
	);
	assert_eq!(
		read_error::<u32>("a: -1"),
		"invalid value: integer `-1`, expected u32 at line 1, column 4"
	);
	assert_eq!(
		read_error::<u64>("a: -1"),
		"invalid value: integer `-1`, expected u64 at line 1, column 4"
	);
	assert_eq!(
		read_error::<u128>("a: -1"),
		"invalid value: integer `-1`, expected u128 at line 1, column 4"
	);
}

#[test]
fn every_integer_type_writes_as_an_int() {
	assert_eq!(written(i8::MIN), "-128");
	assert_eq!(written(u8::MAX), "255");
	assert_eq!(written(i16::MIN), "-32768");
	assert_eq!(written(u16::MAX), "65535");
	assert_eq!(written(i32::MIN), "-2147483648");
	assert_eq!(written(u32::MAX), "4294967295");
	assert_eq!(written(i64::MIN), "-9223372036854775808");
	assert_eq!(written(9_223_372_036_854_775_807u64), "9223372036854775807");
	assert_eq!(written(i128::from(i64::MIN)), "-9223372036854775808");
	assert_eq!(written(u128::from(u64::MAX >> 1)), "9223372036854775807");
}

#[test]
fn an_integer_outside_int64_cannot_be_written() {
	assert_eq!(
		write_error(9_223_372_036_854_775_808u64),
		"The integer 9223372036854775808 is outside the 64-bit range of a SOML int"
	);
	assert_eq!(
		write_error(u64::MAX),
		"The integer 18446744073709551615 is outside the 64-bit range of a SOML int"
	);
	assert_eq!(
		write_error(i128::from(i64::MIN) - 1),
		"The integer -9223372036854775809 is outside the 64-bit range of a SOML int"
	);
	assert_eq!(
		write_error(u128::MAX),
		"The integer 340282366920938463463374607431768211455 is outside the 64-bit range of a SOML int"
	);
}

#[test]
fn a_float_never_reads_into_an_integer() {
	assert_eq!(
		read_error::<i64>("a: 1.0"),
		"invalid type: floating point `1.0`, expected i64 at line 1, column 4"
	);
	assert_eq!(
		read_error::<u8>("a: 2.0"),
		"invalid type: floating point `2.0`, expected u8 at line 1, column 4"
	);
	assert_eq!(
		read_error::<i128>("a: 0.0"),
		"invalid type: floating point `0.0`, expected i128 at line 1, column 4"
	);
}

#[test]
fn a_string_never_reads_into_an_integer() {
	assert_eq!(
		read_error::<i64>("a: '1'"),
		"invalid type: string \"1\", expected i64 at line 1, column 4"
	);
}

// Floats

#[test]
fn an_int_reads_into_a_float_when_it_converts_exactly() {
	assert_eq!(read::<f64>("a: 1").expect("f64"), 1.0);
	assert_eq!(
		read::<f64>("a: -9223372036854775808").expect("-2^63 is exact"),
		-9_223_372_036_854_775_808.0
	);
	assert_eq!(
		read::<f64>("a: 9007199254740992").expect("2^53 is exact"),
		9_007_199_254_740_992.0
	);
	assert_eq!(
		read::<f32>("a: 16777216").expect("2^24 is exact"),
		16_777_216.0
	);
}

#[test]
fn an_int_that_does_not_convert_exactly_does_not_read_into_f64() {
	assert_eq!(
		read_error::<f64>("a: 9007199254740993"),
		"The int 9007199254740993 cannot be converted to a float exactly at line 1, column 4"
	);
	assert_eq!(
		read_error::<f64>("a: 9223372036854775807"),
		"The int 9223372036854775807 cannot be converted to a float exactly at line 1, column 4"
	);
}

#[test]
fn an_int_that_does_not_convert_exactly_does_not_read_into_f32() {
	// 2^24 + 1 is exact in an f64, but an f32 rounds it to 2^24.
	assert!(read::<f32>("a: 16777217").is_err());
}

#[test]
fn a_float_reads_into_f64_and_f32() {
	assert_eq!(read::<f64>("a: 0.1").expect("f64"), 0.1);
	assert_eq!(read::<f64>("a: infinity").expect("f64"), f64::INFINITY);
	assert_eq!(read::<f32>("a: 0.1").expect("f32"), 0.1f32);
	assert_eq!(read::<f32>("a: -infinity").expect("f32"), f32::NEG_INFINITY);
}

#[test]
fn a_float_too_large_for_f32_reads_as_infinity() {
	// The plan does not decide this. serde's f32 visitor narrows the f64 with `as`, as serde_json does.
	assert_eq!(read::<f32>("a: 1e300").expect("f32"), f32::INFINITY);
}

#[test]
fn f32_is_written_with_its_own_shortest_digits() {
	assert_eq!(written(0.1f32), "0.1");
	assert_eq!(written(1.1f32), "1.1");
	assert_eq!(written(16_777_216.0f32), "16777216.0");
	assert_eq!(written(f32::MAX), "3.4028235e38");
	assert_eq!(written(f32::MIN_POSITIVE), "1.1754944e-38");
	assert_eq!(written(f32::from_bits(1)), "1e-45");
	assert_eq!(written(f32::INFINITY), "infinity");
	assert_eq!(written(-0.0f32), "0.0");
	assert_eq!(Value::from(0.1f32), Value::Float(0.1));
}

#[test]
fn f32_reads_back_exactly_from_its_written_form() {
	for value in [
		0.1f32,
		1.0 / 3.0,
		f32::MAX,
		f32::MIN_POSITIVE,
		f32::from_bits(1),
		123_456.79,
		-2.5e-20,
	] {
		let text = write(value).expect("an f32 can be written");
		let read_back = read::<f32>(&text).expect("an f32 reads back");
		assert_eq!(read_back.to_bits(), value.to_bits(), "{text}");
	}
}

#[test]
fn f64_negative_zero_is_written_as_zero() {
	assert_eq!(written(-0.0f64), "0.0");
	assert_eq!(
		soml::to_value(&-0.0f64)
			.expect("a float")
			.as_f64()
			.map(f64::to_bits),
		Some(0)
	);
}

#[test]
fn nan_cannot_be_written() {
	assert_eq!(write_error(f64::NAN), "NaN is not a SOML value");
	assert_eq!(write_error(f32::NAN), "NaN is not a SOML value");
	assert_eq!(
		soml::to_value(&f64::NAN).expect_err("NaN").to_string(),
		"NaN is not a SOML value"
	);
}

#[test]
fn a_string_never_reads_into_a_float() {
	assert_eq!(
		read_error::<f64>("a: '1.5'"),
		"invalid type: string \"1.5\", expected f64 at line 1, column 4"
	);
}

// Strings

#[test]
fn a_string_reads_into_string_str_cow_and_char() {
	assert_eq!(read::<String>("a: 'x'").expect("String"), "x");
	assert_eq!(
		read::<Cow<'static, str>>("a: \"a\\nb\"").expect("Cow"),
		"a\nb"
	);
	assert_eq!(read::<char>("a: 'é'").expect("char"), 'é');
	assert_eq!(read::<char>("a: \"\\u{1f600}\"").expect("char"), '😀');
}

#[test]
fn a_char_needs_exactly_one_character() {
	assert_eq!(
		read_error::<char>("a: 'ab'"),
		"invalid value: string \"ab\", expected a character at line 1, column 4"
	);
	assert_eq!(
		read_error::<char>("a: ''"),
		"invalid value: string \"\", expected a character at line 1, column 4"
	);
}

#[test]
fn strings_and_chars_write_as_strings() {
	assert_eq!(written("x"), "'x'");
	assert_eq!(written(String::from("it's")), "\"it's\"");
	assert_eq!(written(Cow::Borrowed("x")), "'x'");
	assert_eq!(written('\''), "\"'\"");
	assert_eq!(written('é'), "'é'");
}

#[test]
fn a_str_field_borrows_from_the_input() {
	#[derive(Deserialize)]
	struct Borrowed<'a> {
		literal: &'a str,
		escaped: &'a str,
		key: &'a str,
	}

	let text = String::from("literal: 'a'\nescaped: \"b\"\nkey: 'c'");
	let borrowed: Borrowed<'_> =
		soml::from_str(&text).expect("strings without escapes are borrowed");
	assert_eq!(
		(borrowed.literal, borrowed.escaped, borrowed.key),
		("a", "b", "c")
	);

	let range = text.as_bytes().as_ptr_range();
	assert!(range.contains(&borrowed.literal.as_ptr()));
	assert!(range.contains(&borrowed.escaped.as_ptr()));
}

#[test]
fn a_str_field_cannot_hold_a_string_with_escapes_or_a_block_string() {
	#[derive(Deserialize, Debug)]
	#[allow(dead_code)]
	struct Borrowed<'a> {
		a: &'a str,
	}

	assert_eq!(
		soml::from_str::<Borrowed<'_>>("a: \"x\\ny\"")
			.expect_err("an escape needs an owned string")
			.to_string(),
		"invalid type: string \"x\\ny\", expected a borrowed string at line 1, column 4"
	);
	assert_eq!(
		soml::from_str::<Borrowed<'_>>("a: '''\n  x\n  '''")
			.expect_err("a block string is owned")
			.to_string(),
		"invalid type: string \"x\", expected a borrowed string at line 1, column 4"
	);
}

#[test]
fn a_borrowed_map_key() {
	let text = String::from("first: 1\n'second': 2");
	let map: BTreeMap<&str, i32> =
		soml::from_str(&text).expect("bare and literal keys are borrowed");
	assert_eq!(map, BTreeMap::from([("first", 1), ("second", 2)]));
}

#[test]
fn ints_floats_and_bools_never_read_into_a_string() {
	assert_eq!(
		read_error::<String>("a: 1"),
		"invalid type: integer `1`, expected a string at line 1, column 4"
	);
	assert_eq!(
		read_error::<String>("a: 1.5"),
		"invalid type: floating point `1.5`, expected a string at line 1, column 4"
	);
	assert_eq!(
		read_error::<String>("a: true"),
		"invalid type: boolean `true`, expected a string at line 1, column 4"
	);
	assert_eq!(
		read_error::<String>("a: null"),
		"invalid type: unit value, expected a string at line 1, column 4"
	);
}

#[test]
fn a_string_field_accepts_an_instant_or_a_duration_as_its_canonical_text() {
	// A documented decision: jiff and chrono types ask for a string, so an instant or a duration answers with its canonical text, and a `String` field gets that too.
	assert_eq!(
		read::<String>("a: 2026-09-19T21:00:00.50+07:00").expect("an instant as text"),
		"2026-09-19T14:00:00.5Z"
	);
	assert_eq!(
		read::<String>("a: 90m").expect("a duration as text"),
		"1h30m"
	);
}

// Option and unit

#[test]
fn an_option_is_null_missing_or_the_value() {
	#[derive(Deserialize, Debug, PartialEq)]
	struct Optional {
		a: Option<i32>,
		b: Option<i32>,
		c: Option<i32>,
	}

	assert_eq!(
		soml::from_str::<Optional>("a: null\nb: 1").expect("options"),
		Optional {
			a: None,
			b: Some(1),
			c: None,
		}
	);
}

#[test]
fn an_option_is_written_as_null_or_the_value() {
	assert_eq!(written(None::<i32>), "null");
	assert_eq!(written(Some(1)), "1");
	assert_eq!(written(Some(Some(1))), "1");
}

#[test]
fn an_option_of_the_wrong_type_is_still_an_error() {
	assert_eq!(
		read_error::<Option<i32>>("a: 'x'"),
		"invalid type: string \"x\", expected i32 at line 1, column 4"
	);
}

#[test]
fn unit_and_a_unit_struct_are_null() {
	#[derive(Serialize, Deserialize, Debug, PartialEq)]
	struct Unit;

	assert_eq!(written(()), "null");
	assert_eq!(written(Unit), "null");
	read::<()>("a: null").expect("unit");
	assert_eq!(read::<Unit>("a: null").expect("a unit struct"), Unit);
	assert_eq!(
		read_error::<()>("a: 0"),
		"invalid type: integer `0`, expected unit at line 1, column 4"
	);
}

// Sequences

#[test]
fn sequences_tuples_and_arrays_are_arrays() {
	assert_eq!(
		read::<Vec<i32>>("a: [1, 2, 3]").expect("Vec"),
		vec![1, 2, 3]
	);
	assert_eq!(
		read::<(i32, String, bool)>("a: [1, 'x', true]").expect("tuple"),
		(1, String::from("x"), true)
	);
	assert_eq!(read::<[u8; 2]>("a: [1, 2]").expect("array"), [1, 2]);
	assert_eq!(written(vec![1, 2]), "[\n\t1\n\t2\n]");
	assert_eq!(written((1, "x")), "[\n\t1\n\t'x'\n]");
	assert_eq!(written([0u8; 0]), "[]");
}

#[test]
fn a_tuple_struct_is_an_array_and_a_newtype_struct_is_its_content() {
	#[derive(Serialize, Deserialize, Debug, PartialEq)]
	struct Pair(i32, i32);

	#[derive(Serialize, Deserialize, Debug, PartialEq)]
	struct Meters(f64);

	assert_eq!(written(Pair(1, 2)), "[\n\t1\n\t2\n]");
	assert_eq!(
		read::<Pair>("a: [1, 2]").expect("a tuple struct"),
		Pair(1, 2)
	);
	assert_eq!(written(Meters(1.5)), "1.5");
	assert_eq!(
		read::<Meters>("a: 1.5").expect("a newtype struct"),
		Meters(1.5)
	);
}

#[test]
fn a_tuple_that_is_too_short_is_an_error() {
	assert_eq!(
		read_error::<(i32, i32)>("a: [1]"),
		"invalid length 1, expected a tuple of size 2 at line 1, column 4"
	);
	assert_eq!(
		read_error::<[i32; 2]>("a: [1]"),
		"invalid length 1, expected an array of length 2 at line 1, column 4"
	);
}

#[test]
fn a_tuple_that_is_too_long_is_an_error() {
	// serde_json and serde's own `SeqDeserializer` reject the extra item. Dropping it loses data without a word.
	assert!(read::<(i32, i32)>("a: [1, 2, 3]").is_err());
	assert!(read::<[i32; 2]>("a: [1, 2, 3]").is_err());
	assert!(read::<Shape>("a: {Point: [1, 2, 3]}").is_err());
	assert!(
		soml::from_value::<(i32, i32)>(Value::Array(vec![
			Value::Int(1),
			Value::Int(2),
			Value::Int(3)
		]))
		.is_err()
	);
}

#[test]
fn a_sequence_needs_an_array() {
	assert_eq!(
		read_error::<Vec<i32>>("a: {}"),
		"invalid type: map, expected a sequence at line 1, column 4"
	);
	assert_eq!(
		read_error::<Vec<i32>>("a: 'x'"),
		"invalid type: string \"x\", expected a sequence at line 1, column 4"
	);
}

#[test]
fn a_top_level_array_reads_into_a_vec() {
	assert_eq!(
		soml::from_str::<Vec<i32>>("[1, 2]").expect("a top-level array"),
		vec![1, 2]
	);
	assert_eq!(
		soml::to_string(&vec![1, 2]).expect("a top-level array"),
		"[\n\t1\n\t2\n]\n"
	);
}

// Structs and maps

#[test]
fn a_struct_round_trips() {
	#[derive(Serialize, Deserialize, Debug, PartialEq)]
	#[serde(rename_all = "kebab-case")]
	struct Config {
		name: String,
		replicas: u32,
		ratio: f64,
		labels: Vec<String>,
		limits: BTreeMap<String, i64>,
		owner: Option<String>,
	}

	let config = Config {
		name: String::from("api-gateway"),
		replicas: 3,
		ratio: 0.5,
		labels: vec![String::from("prod")],
		limits: BTreeMap::from([(String::from("cpu"), 2)]),
		owner: None,
	};

	let text = soml::to_string(&config).expect("a struct is a document");
	assert_eq!(
		text,
		"name: 'api-gateway'\nreplicas: 3\nratio: 0.5\nlabels: [\n\t'prod'\n]\nlimits: {\n\tcpu: 2\n}\nowner: null\n"
	);
	assert_eq!(
		soml::from_str::<Config>(&text).expect("it reads back"),
		config
	);
}

#[derive(Serialize)]
struct Package {
	name: &'static str,
	description: &'static str,
	author: Author,
	versions: Vec<Release>,
}

#[derive(Serialize)]
struct Author {
	name: &'static str,
	email: &'static str,
}

#[derive(Serialize)]
struct Release {
	version: &'static str,
	date: &'static str,
}

fn package() -> Package {
	Package {
		name: "soml",
		description: "A config format",
		author: Author {
			name: "Sindre",
			email: "sindre@example.com",
		},
		versions: vec![Release {
			version: "1.0.0",
			date: "2026-10-07",
		}],
	}
}

#[test]
fn a_struct_is_written_in_the_order_of_its_fields_at_every_level() {
	let package = package();

	assert_eq!(
		soml::to_string(&package).expect("a struct is a document"),
		"name: 'soml'\ndescription: 'A config format'\nauthor: {\n\tname: 'Sindre'\n\temail: 'sindre@example.com'\n}\nversions: [\n\t{\n\t\tversion: '1.0.0'\n\t\tdate: '2026-10-07'\n\t}\n]\n"
	);
}

#[test]
fn canonical_form_sorts_the_members_of_a_struct_at_every_level() {
	let package = package();
	let canonical = "author: {\n\temail: 'sindre@example.com'\n\tname: 'Sindre'\n}\ndescription: 'A config format'\nname: 'soml'\nversions: [\n\t{\n\t\tdate: '2026-10-07'\n\t\tversion: '1.0.0'\n\t}\n]\n";

	assert_eq!(
		soml::to_string_canonical(&package).expect("a struct is a document"),
		canonical
	);

	let mut output = Vec::new();
	soml::to_writer_canonical(&mut output, &package).expect("a struct is a document");
	assert_eq!(output, canonical.as_bytes());
}

#[test]
fn a_map_is_written_in_the_order_it_iterates_in() {
	// A `Vec` of pairs, written as a map, stands in for an insertion-ordered map such as `IndexMap`.
	struct Ordered(Vec<(&'static str, i32)>);

	impl Serialize for Ordered {
		fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
			serializer.collect_map(self.0.iter().copied())
		}
	}

	let map = Ordered(vec![("b", 1), ("a", 2), ("c", 3)]);
	assert_eq!(
		soml::to_string(&map).expect("a map is a document"),
		"b: 1\na: 2\nc: 3\n"
	);
	assert_eq!(
		soml::to_string_canonical(&map).expect("a map is a document"),
		"a: 2\nb: 1\nc: 3\n"
	);
	// A `Value` keeps its members sorted, so it is written sorted.
	assert_eq!(
		soml::to_string(&soml::to_value(&map).expect("a map is a value"))
			.expect("a map is a document"),
		"a: 2\nb: 1\nc: 3\n"
	);
}

#[test]
fn a_struct_ignores_unknown_fields_unless_told_not_to() {
	#[derive(Deserialize, Debug, PartialEq)]
	struct Loose {
		a: i32,
	}

	assert_eq!(
		soml::from_str::<Loose>("a: 1\nb: {c: [1, 'x']}").expect("b is ignored"),
		Loose { a: 1 }
	);
}

#[test]
fn a_hash_map_is_written_sorted_in_canonical_form() {
	let map: HashMap<String, i32> = (0..20)
		.map(|index| (format!("k{index:02}"), index))
		.collect();
	let text = soml::to_string_canonical(&map).expect("a map is a document");
	let keys: Vec<&str> = text.lines().map(|line| &line[..3]).collect();
	let mut sorted = keys.clone();
	sorted.sort_unstable();
	assert_eq!(keys, sorted);
}

#[test]
fn int_keys_are_written_in_decimal_and_read_back() {
	let map = BTreeMap::from([(404u16, "not found"), (200, "ok")]);
	assert_eq!(
		soml::to_string(&map).expect("int keys"),
		"200: 'ok'\n404: 'not found'\n"
	);
	assert_eq!(
		soml::from_str::<BTreeMap<u16, String>>("404: 'not found'\n'200': 'ok'").expect("int keys"),
		BTreeMap::from([(404, String::from("not found")), (200, String::from("ok"))])
	);
	assert_eq!(
		soml::from_str::<BTreeMap<i32, i32>>("-1: 1\n0: 2").expect("int keys"),
		BTreeMap::from([(-1, 1), (0, 2)])
	);
	assert_eq!(
		soml::to_string(&BTreeMap::from([(-1i64, 1)])).expect("a negative key"),
		"-1: 1\n"
	);
}

#[test]
fn an_int_key_reads_only_from_canonical_decimal() {
	for key in ["007", "-0", "+1", "0x10", "1_000", "1.0", "", "a", "- 1"] {
		let text = format!("'{key}': 1");
		let error = soml::from_str::<BTreeMap<u16, i32>>(&text).expect_err("not canonical decimal");
		assert_eq!(
			error.to_string(),
			format!(
				"Expected the key “{key}” to be an integer in decimal, like 404 at line 1, column 1"
			),
			"{text}"
		);
	}
}

#[test]
fn an_int_key_outside_the_type_range_is_an_error() {
	assert_eq!(
		soml::from_str::<BTreeMap<u8, i32>>("256: 1")
			.expect_err("too large for u8")
			.to_string(),
		"invalid value: integer `256`, expected u8 at line 1, column 1"
	);
	assert_eq!(
		soml::from_str::<BTreeMap<u64, i32>>("18446744073709551615: 1").expect("u64::MAX as a key")
			[&u64::MAX],
		1
	);
	assert_eq!(
		soml::from_str::<BTreeMap<i128, i32>>("-170141183460469231731687303715884105728: 1")
			.expect("i128::MIN as a key")[&i128::MIN],
		1
	);
}

#[test]
fn integer_keys_of_128_bits_are_written_in_decimal() {
	// A key is a string, so the int64 range of a value does not apply.
	assert_eq!(
		soml::to_string(&BTreeMap::from([(u128::MAX, 1)])).expect("a key is text"),
		"340282366920938463463374607431768211455: 1\n"
	);
}

#[test]
fn bool_keys_are_written_and_read() {
	let map = BTreeMap::from([(true, 1), (false, 0)]);
	assert_eq!(
		soml::to_string(&map).expect("bool keys"),
		"false: 0\ntrue: 1\n"
	);
	assert_eq!(
		soml::from_str::<BTreeMap<bool, i32>>("true: 1\nfalse: 0").expect("bool keys"),
		map
	);
	assert_eq!(
		soml::from_str::<BTreeMap<bool, i32>>("yes: 1")
			.expect_err("not a bool")
			.to_string(),
		"Expected the key “yes” to be true or false at line 1, column 1"
	);
}

#[test]
fn char_keys_are_written_and_read() {
	let map = BTreeMap::from([('a', 1), ('é', 2)]);
	assert_eq!(soml::to_string(&map).expect("char keys"), "a: 1\n'é': 2\n");
	assert_eq!(
		soml::from_str::<BTreeMap<char, i32>>("a: 1\n'é': 2").expect("char keys"),
		map
	);
	assert_eq!(
		soml::from_str::<BTreeMap<char, i32>>("ab: 1")
			.expect_err("two characters")
			.to_string(),
		"invalid value: string \"ab\", expected a character at line 1, column 1"
	);
}

#[test]
fn float_keys_are_an_error_both_ways() {
	struct Map(Vec<(f64, i32)>);

	impl Serialize for Map {
		fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
			serializer.collect_map(self.0.iter().map(|(key, value)| (key, value)))
		}
	}

	assert_eq!(
		soml::to_string(&Map(vec![(1.5, 1)]))
			.expect_err("a float key")
			.to_string(),
		"A map key must be a string, a char, a bool, an integer, or a unit enum variant, not a float"
	);

	#[derive(Deserialize, Debug)]
	#[allow(dead_code)]
	struct Wrapper {
		#[serde(with = "float_map")]
		map: Vec<(f64, i32)>,
	}

	mod float_map {
		use serde::Deserializer;
		use serde::de::{MapAccess, Visitor};

		pub fn deserialize<'de, D: Deserializer<'de>>(
			deserializer: D,
		) -> Result<Vec<(f64, i32)>, D::Error> {
			struct FloatMapVisitor;

			impl<'de> Visitor<'de> for FloatMapVisitor {
				type Value = Vec<(f64, i32)>;

				fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
					formatter.write_str("a map with float keys")
				}

				fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
					let mut entries = Vec::new();

					while let Some(entry) = map.next_entry()? {
						entries.push(entry);
					}

					Ok(entries)
				}
			}

			deserializer.deserialize_map(FloatMapVisitor)
		}
	}

	assert_eq!(
		soml::from_str::<Wrapper>("map: {'1.5': 1}")
			.expect_err("a float key")
			.to_string(),
		"invalid type: string \"1.5\", expected f64 at line 1, column 7"
	);
}

#[test]
fn other_map_keys_are_an_error() {
	assert_eq!(
		soml::to_string(&BTreeMap::from([(None::<i32>, 1)]))
			.expect_err("None is not a key")
			.to_string(),
		"A map key must be a string, a char, a bool, an integer, or a unit enum variant, not None"
	);
	assert_eq!(
		soml::to_string(&BTreeMap::from([((1, 2), 1)]))
			.expect_err("a tuple is not a key")
			.to_string(),
		"A map key must be a string, a char, a bool, an integer, or a unit enum variant, not a tuple"
	);
	assert_eq!(
		soml::to_string(&BTreeMap::from([(soml::Instant::MIN, 1)]))
			.expect_err("an instant is not a key")
			.to_string(),
		"A map key must be a string, a char, a bool, an integer, or a unit enum variant, not an instant"
	);
}

#[test]
fn unit_variant_keys_are_written_and_read() {
	#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, PartialOrd, Ord)]
	#[serde(rename_all = "kebab-case")]
	enum Region {
		EuWest,
		UsEast,
	}

	let map = BTreeMap::from([(Region::UsEast, 2), (Region::EuWest, 1)]);
	let text = soml::to_string(&map).expect("unit variant keys");
	assert_eq!(text, "eu-west: 1\nus-east: 2\n");
	assert_eq!(
		soml::from_str::<BTreeMap<Region, i32>>(&text).expect("unit variant keys"),
		map
	);
}

#[test]
fn a_duplicate_key_on_serialize_is_an_error() {
	struct Duplicates;

	impl Serialize for Duplicates {
		fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
			serializer.collect_map([("a", 1), ("a", 2)])
		}
	}

	assert_eq!(
		soml::to_string(&Duplicates)
			.expect_err("a duplicate")
			.to_string(),
		"Duplicate key “a”"
	);
	assert_eq!(
		soml::to_value(&Duplicates)
			.expect_err("a duplicate")
			.to_string(),
		"Duplicate key “a”"
	);
}

#[test]
fn keys_that_only_become_equal_as_text_are_duplicates() {
	// The int key 1 and the string key "1" are the same SOML key.
	#[derive(Serialize)]
	struct Mixed {
		#[serde(rename = "1")]
		one: i32,
		#[serde(flatten)]
		rest: BTreeMap<i32, i32>,
	}

	let mixed = Mixed {
		one: 1,
		rest: BTreeMap::from([(1, 2)]),
	};

	assert_eq!(
		soml::to_string(&mixed)
			.expect_err("a duplicate")
			.to_string(),
		"Duplicate key “1”"
	);
}

// Enums

#[derive(Serialize, Deserialize, Debug, PartialEq)]
enum Shape {
	Empty,
	Circle(f64),
	Point(i32, i32),
	Rectangle { width: i32, height: i32 },
}

#[test]
fn enum_variants_are_externally_tagged_when_written() {
	assert_eq!(written(Shape::Empty), "'Empty'");
	assert_eq!(written(Shape::Circle(1.5)), "{\n\tCircle: 1.5\n}");
	assert_eq!(
		written(Shape::Point(1, 2)),
		"{\n\tPoint: [\n\t\t1\n\t\t2\n\t]\n}"
	);
	assert_eq!(
		written(Shape::Rectangle {
			width: 2,
			height: 3
		}),
		"{\n\tRectangle: {\n\t\twidth: 2\n\t\theight: 3\n\t}\n}"
	);
}

#[test]
fn enum_variants_are_read_from_their_tagged_form() {
	assert_eq!(
		read::<Shape>("a: 'Empty'").expect("a unit variant"),
		Shape::Empty
	);
	assert_eq!(
		read::<Shape>("a: \"Empty\"").expect("a unit variant"),
		Shape::Empty
	);
	assert_eq!(
		read::<Shape>("a: {Empty: null}").expect("a unit variant as an object"),
		Shape::Empty
	);
	assert_eq!(
		read::<Shape>("a: {Circle: 1.5}").expect("a newtype variant"),
		Shape::Circle(1.5)
	);
	assert_eq!(
		read::<Shape>("a: {Point: [1, 2]}").expect("a tuple variant"),
		Shape::Point(1, 2)
	);
	assert_eq!(
		read::<Shape>("a: {Rectangle: {width: 2, height: 3}}").expect("a struct variant"),
		Shape::Rectangle {
			width: 2,
			height: 3
		}
	);
}

#[test]
fn a_bad_enum_value_is_an_error() {
	assert_eq!(
		read_error::<Shape>("a: 'Square'"),
		"unknown variant `Square`, expected one of `Empty`, `Circle`, `Point`, `Rectangle` at line 1, column 4"
	);
	assert_eq!(
		read_error::<Shape>("a: {Circle: 1.5, Empty: null}"),
		"invalid type: map, expected an enum variant, which is a string, or an object with one member at line 1, column 4"
	);
	assert_eq!(
		read_error::<Shape>("a: 1"),
		"invalid type: integer `1`, expected an enum variant, which is a string, or an object with one member at line 1, column 4"
	);
	assert_eq!(
		read_error::<Shape>("a: {Empty: 1}"),
		"invalid type: integer `1`, expected null for a unit variant at line 1, column 12"
	);
	assert_eq!(
		read_error::<Shape>("a: {Square: 1}"),
		"unknown variant `Square`, expected one of `Empty`, `Circle`, `Point`, `Rectangle` at line 1, column 5"
	);
}

#[test]
fn enums_round_trip_through_a_document() {
	let shapes = vec![
		Shape::Empty,
		Shape::Circle(0.1),
		Shape::Point(-1, 1),
		Shape::Rectangle {
			width: 1,
			height: 2,
		},
	];
	let text = soml::to_string(&shapes).expect("enums");
	assert_eq!(
		soml::from_str::<Vec<Shape>>(&text).expect("enums read back"),
		shapes
	);
}

// Instants, durations, and values

#[test]
fn soml_instant_and_duration_round_trip_as_native_values() {
	#[derive(Serialize, Deserialize, Debug, PartialEq)]
	struct Event {
		at: soml::Instant,
		took: soml::Duration,
	}

	let event: Event =
		soml::from_str("at: 2026-09-19T21:00:00+07:00\ntook: 90m").expect("native values");
	assert_eq!(event.at.unix_seconds(), 1_789_826_400);
	assert_eq!(event.took.nanoseconds(), 5_400_000_000_000);

	let text = soml::to_string(&event).expect("native values");
	assert_eq!(text, "at: 2026-09-19T14:00:00Z\ntook: 1h30m\n");
	assert_eq!(
		soml::from_str::<Event>(&text).expect("it reads back"),
		event
	);
}

#[test]
fn soml_instant_does_not_read_from_a_quoted_string() {
	assert_eq!(
		read_error::<soml::Instant>("a: '2026-09-19T14:00:00Z'"),
		"invalid type: string \"2026-09-19T14:00:00Z\", expected an instant at line 1, column 4"
	);
	assert_eq!(
		read_error::<soml::Instant>("a: 1"),
		"invalid type: integer `1`, expected an instant at line 1, column 4"
	);
	assert_eq!(
		read_error::<soml::Instant>("a: 1h"),
		"invalid type: a duration, expected an instant at line 1, column 4"
	);
}

#[test]
fn soml_duration_does_not_read_from_a_quoted_string() {
	assert_eq!(
		read_error::<soml::Duration>("a: '1h'"),
		"invalid type: string \"1h\", expected a duration at line 1, column 4"
	);
	assert_eq!(
		read_error::<soml::Duration>("a: 3600"),
		"invalid type: integer `3600`, expected a duration at line 1, column 4"
	);
	assert_eq!(
		read_error::<soml::Duration>("a: 2026-09-19T14:00:00Z"),
		"invalid type: an instant, expected a duration at line 1, column 4"
	);
}

#[test]
fn soml_value_as_a_field_keeps_every_type() {
	#[derive(Deserialize)]
	struct Holder {
		a: Value,
	}

	let holder: Holder =
		soml::from_str("a: [1, 1.0, 'x', true, null, 2026-09-19T14:00:00Z, 1h, {b: []}]")
			.expect("any value");
	assert_eq!(
		holder.a,
		Value::Array(vec![
			Value::Int(1),
			Value::Float(1.0),
			Value::from("x"),
			Value::Bool(true),
			Value::Null,
			Value::Instant("2026-09-19T14:00:00Z".parse().expect("an instant")),
			Value::Duration("1h".parse().expect("a duration")),
			[("b", Value::Array(Vec::new()))].into_iter().collect(),
		])
	);
}

#[test]
fn from_str_into_value_equals_parse() {
	let text = "a: 2026-09-19T14:00:00Z\nb: [1h, 0.5, 'x']";
	assert_eq!(
		soml::from_str::<Value>(text).expect("a value"),
		text.parse::<Value>().expect("a value")
	);
}

#[test]
fn from_value_reads_a_type() {
	#[derive(Deserialize, Debug, PartialEq)]
	struct Server {
		port: u16,
		started: soml::Instant,
		timeout: std::time::Duration,
		tags: Vec<String>,
	}

	let value: Value = "port: 8080\nstarted: 2026-09-19T14:00:00Z\ntimeout: 1m30s\ntags: ['a']"
		.parse()
		.expect("a value");
	let server: Server = soml::from_value(value).expect("the value fits");
	assert_eq!(server.port, 8080);
	assert_eq!(server.started.to_string(), "2026-09-19T14:00:00Z");
	assert_eq!(server.timeout, std::time::Duration::from_secs(90));
	assert_eq!(server.tags, ["a"]);
}

#[test]
fn from_value_errors_have_no_position() {
	let value: Value = "port: 70000".parse().expect("a value");

	#[derive(Deserialize, Debug)]
	#[allow(dead_code)]
	struct Server {
		port: u16,
	}

	let error = soml::from_value::<Server>(value).expect_err("70000 is not a u16");
	assert_eq!(
		error.to_string(),
		"invalid value: integer `70000`, expected u16"
	);
	assert_eq!(error.position(), None);
	assert_eq!(error.offset(), None);
}

#[test]
fn to_value_converts_a_type() {
	#[derive(Serialize)]
	struct Server {
		port: u16,
		tags: Vec<&'static str>,
		at: soml::Instant,
	}

	let value = soml::to_value(&Server {
		port: 8080,
		tags: vec!["a"],
		at: soml::Instant::MIN,
	})
	.expect("a value");

	assert_eq!(
		value,
		[
			("port", Value::Int(8080)),
			("tags", Value::Array(vec![Value::from("a")])),
			("at", Value::Instant(soml::Instant::MIN)),
		]
		.into_iter()
		.collect()
	);
}

#[test]
fn to_value_allows_a_top_level_scalar() {
	assert_eq!(soml::to_value(&5).expect("an int"), Value::Int(5));
	assert_eq!(soml::to_value("x").expect("a string"), Value::from("x"));
}

#[test]
fn to_value_refuses_a_carriage_return() {
	assert!(soml::to_value("a\rb").is_err());
}

#[test]
fn value_round_trips_through_to_value_and_from_value() {
	let value: Value = "a: [1, 1.5, 'x', null, true, 2026-09-19T14:00:00.5Z, -1.5s, {b: {}}]"
		.parse()
		.expect("a value");
	assert_eq!(soml::to_value(&value).expect("a value"), value);
	assert_eq!(
		soml::from_value::<Value>(value.clone()).expect("a value"),
		value
	);
}

// std::time::Duration

#[test]
fn std_duration_reads_from_a_native_duration() {
	assert_eq!(
		read::<std::time::Duration>("a: 1m30s").expect("a duration"),
		std::time::Duration::from_secs(90)
	);
	assert_eq!(
		read::<std::time::Duration>("a: 1.000000001s").expect("a duration"),
		std::time::Duration::new(1, 1)
	);
	assert_eq!(
		read::<std::time::Duration>("a: 0s").expect("a duration"),
		std::time::Duration::ZERO
	);
	assert_eq!(
		read::<std::time::Duration>("a: 9223372036854775807ns").expect("the longest duration"),
		std::time::Duration::from_nanos(i64::MAX.unsigned_abs())
	);
}

#[test]
fn std_duration_refuses_a_negative_duration() {
	assert_eq!(
		read_error::<std::time::Duration>("a: -5m"),
		"The duration -5m is negative, which a std::time::Duration cannot be at line 1, column 4"
	);
}

#[test]
fn std_duration_also_reads_from_its_secs_and_nanos() {
	assert_eq!(
		read::<std::time::Duration>("a: {secs: 90, nanos: 5}").expect("serde's own shape"),
		std::time::Duration::new(90, 5)
	);
}

#[test]
fn std_duration_does_not_read_from_a_string_or_an_int() {
	assert_eq!(
		read_error::<std::time::Duration>("a: '1m'"),
		"invalid type: string \"1m\", expected struct Duration at line 1, column 4"
	);
	assert_eq!(
		read_error::<std::time::Duration>("a: 60"),
		"invalid type: integer `60`, expected struct Duration at line 1, column 4"
	);
}

#[test]
fn std_duration_is_written_as_a_native_duration() {
	assert_eq!(written(std::time::Duration::from_secs(90)), "1m30s");
	assert_eq!(written(std::time::Duration::new(1, 500_000_000)), "1.5s");
	assert_eq!(written(std::time::Duration::ZERO), "0s");
	assert_eq!(
		written(vec![std::time::Duration::from_millis(1)]),
		"[\n\t0.001s\n]"
	);
	assert_eq!(
		write_error(std::time::Duration::from_secs(10_000_000_000)),
		"A duration of 10000000000 seconds is outside the SOML range of about 292 years"
	);
	// Its seconds are already outside the int range, before the duration is recognized.
	assert_eq!(
		write_error(std::time::Duration::MAX),
		"The integer 18446744073709551615 is outside the 64-bit range of a SOML int"
	);
}

#[test]
fn a_user_struct_with_the_shape_of_std_duration_round_trips() {
	#[derive(Serialize, Deserialize, Debug, PartialEq)]
	struct Duration {
		secs: u64,
		nanos: u32,
	}

	let duration = Duration { secs: 1, nanos: 5 };
	assert_eq!(written(&duration), "1.000000005s");
	assert_eq!(
		read::<Duration>("a: 1.000000005s").expect("the same shape"),
		duration
	);
}

#[test]
fn a_user_struct_with_the_fields_of_std_duration_in_another_order_round_trips() {
	#[derive(Serialize, Deserialize, Debug, PartialEq)]
	struct Duration {
		nanos: u32,
		secs: u64,
	}

	let duration = Duration { nanos: 5, secs: 1 };
	assert_eq!(written(&duration), "1.000000005s");
	assert_eq!(
		read::<Duration>("a: 1.000000005s").expect("the same shape"),
		duration
	);
}

// jiff and chrono

#[test]
fn jiff_timestamp_reads_from_a_native_instant() {
	let timestamp = read::<jiff::Timestamp>("a: 2026-09-19T21:00:00.5+07:00").expect("an instant");
	assert_eq!(
		timestamp,
		"2026-09-19T14:00:00.5Z"
			.parse::<jiff::Timestamp>()
			.expect("a timestamp")
	);
}

#[test]
fn jiff_signed_duration_reads_from_a_native_duration() {
	assert_eq!(
		read::<jiff::SignedDuration>("a: -1h30m").expect("a duration"),
		jiff::SignedDuration::from_mins(-90)
	);
	assert_eq!(
		read::<jiff::SignedDuration>("a: 0.5s").expect("a duration"),
		jiff::SignedDuration::from_millis(500)
	);
}

#[test]
fn chrono_date_time_reads_from_a_native_instant() {
	let date_time = read::<chrono::DateTime<chrono::Utc>>("a: 2026-09-19T21:00:00.123456789+07:00")
		.expect("an instant");
	assert_eq!(date_time.timestamp(), 1_789_826_400);
	assert_eq!(date_time.timestamp_subsec_nanos(), 123_456_789);
}

#[test]
fn jiff_and_chrono_types_are_written_as_strings() {
	// They serialize as strings, so they are quoted. `soml::Instant` writes a native instant.
	let timestamp: jiff::Timestamp = "2026-09-19T14:00:00Z".parse().expect("a timestamp");
	assert_eq!(written(timestamp), "'2026-09-19T14:00:00Z'");
	assert_eq!(
		written(chrono::DateTime::<chrono::Utc>::from_timestamp(0, 0).expect("the epoch")),
		"'1970-01-01T00:00:00Z'"
	);
}

// serde_json

#[test]
fn transcoding_to_serde_json_gives_instants_and_durations_as_text() {
	let json: serde_json::Value =
		soml::from_str("at: 2026-09-19T21:00:00+07:00\ntook: 90m\nn: [1, 1.5, 'x', null, true]")
			.expect("any value");
	assert_eq!(
		json,
		serde_json::json!({
			"at": "2026-09-19T14:00:00Z",
			"took": "1h30m",
			"n": [1, 1.5, "x", null, true],
		})
	);
}

#[test]
fn transcoding_infinity_to_serde_json_gives_null() {
	// serde_json cannot hold infinity, and turns it into null.
	let json: serde_json::Value = soml::from_str("a: infinity").expect("any value");
	assert_eq!(json, serde_json::json!({"a": null}));
}

#[test]
fn soml_instant_and_value_are_text_in_json() {
	let value: Value = "a: 2026-09-19T14:00:00Z\nb: 1h\nc: 1.0"
		.parse()
		.expect("a value");
	assert_eq!(
		serde_json::to_string(&value).expect("JSON"),
		r#"{"a":"2026-09-19T14:00:00Z","b":"1h","c":1.0}"#
	);
	assert_eq!(
		serde_json::to_string(&soml::Instant::MIN).expect("JSON"),
		r#""0001-01-01T00:00:00Z""#
	);
	assert_eq!(
		serde_json::from_str::<soml::Instant>(r#""2026-09-19T21:00:00+07:00""#)
			.map(|instant| instant.to_string())
			.expect("from JSON text"),
		"2026-09-19T14:00:00Z"
	);
}

#[test]
fn serde_json_value_writes_to_soml() {
	let json = serde_json::json!({"b": [1, 2.5, "x"], "a": {"c": null}});
	assert_eq!(
		soml::to_string(&json).expect("JSON values"),
		"a: {\n\tc: null\n}\nb: [\n\t1\n\t2.5\n\t'x'\n]\n"
	);
}

// The buffered paths: flatten and untagged

#[test]
fn flatten_gives_an_instant_to_a_string_field_as_its_text() {
	#[derive(Deserialize, Debug)]
	struct Outer {
		#[serde(flatten)]
		inner: Inner,
	}

	#[derive(Deserialize, Debug)]
	struct Inner {
		at: String,
	}

	// `flatten` buffers values through `deserialize_any`, which gives an instant as its canonical text.
	let outer: Outer = soml::from_str("at: 2026-09-19T21:00:00+07:00").expect("an instant as text");
	assert_eq!(outer.inner.at, "2026-09-19T14:00:00Z");
}

#[test]
fn flatten_reads_an_instant_into_soml_instant_through_its_text() {
	#[derive(Deserialize, Debug)]
	struct Outer {
		#[serde(flatten)]
		inner: Inner,
	}

	#[derive(Deserialize, Debug)]
	struct Inner {
		at: soml::Instant,
	}

	let outer: Outer = soml::from_str("at: 2026-09-19T21:00:00+07:00").expect("an instant");
	assert_eq!(outer.inner.at.to_string(), "2026-09-19T14:00:00Z");

	// The buffer cannot tell an instant from its text, so in this one place a quoted string reads into `soml::Instant` too.
	let outer: Outer =
		soml::from_str("at: '2026-09-19T14:00:00Z'").expect("a string, through the buffer");
	assert_eq!(outer.inner.at.to_string(), "2026-09-19T14:00:00Z");
}

#[test]
fn flatten_gives_an_instant_to_a_value_as_a_string() {
	#[derive(Deserialize, Debug)]
	struct Outer {
		#[serde(flatten)]
		rest: BTreeMap<String, Value>,
	}

	// Through the buffer, an instant and a duration become their text, so a `Value` holds them as strings.
	let outer: Outer =
		soml::from_str("at: 2026-09-19T14:00:00Z\ntook: 1h\nn: 1").expect("any values");
	assert_eq!(
		outer.rest,
		BTreeMap::from([
			(String::from("at"), Value::from("2026-09-19T14:00:00Z")),
			(String::from("n"), Value::Int(1)),
			(String::from("took"), Value::from("1h")),
		])
	);
}

#[test]
fn untagged_enums_see_an_instant_as_its_text() {
	#[derive(Deserialize, Debug, PartialEq)]
	#[serde(untagged)]
	enum Either {
		Number(i64),
		Instant(soml::Instant),
		Text(String),
	}

	// The untagged buffer gives an instant as its text, so it matches `soml::Instant` through its string request, and a quoted string that looks like an instant matches it too.
	assert_eq!(read::<Either>("a: 1").expect("a number"), Either::Number(1));
	assert_eq!(
		read::<Either>("a: 2026-09-19T14:00:00Z").expect("an instant"),
		Either::Instant("2026-09-19T14:00:00Z".parse().expect("an instant"))
	);
	assert_eq!(
		read::<Either>("a: '2026-09-19T14:00:00Z'").expect("a string that looks like an instant"),
		Either::Instant("2026-09-19T14:00:00Z".parse().expect("an instant"))
	);
	assert_eq!(
		read::<Either>("a: 'x'").expect("text"),
		Either::Text(String::from("x"))
	);
}

#[test]
fn an_untagged_value_holds_an_instant_as_a_string() {
	#[derive(Deserialize, Debug, PartialEq)]
	#[serde(untagged)]
	enum Either {
		Number(i64),
		Any(Value),
	}

	assert_eq!(
		read::<Either>("a: 2026-09-19T14:00:00Z").expect("any value"),
		Either::Any(Value::from("2026-09-19T14:00:00Z"))
	);
}

#[test]
fn an_error_made_after_reading_points_at_the_value() {
	#[allow(dead_code)]
	#[derive(Debug, serde::Deserialize)]
	#[serde(try_from = "u16")]
	struct Port(u16);

	impl TryFrom<u16> for Port {
		type Error = String;

		fn try_from(value: u16) -> Result<Self, String> {
			if value == 0 {
				return Err("port 0 is reserved".to_owned());
			}

			Ok(Self(value))
		}
	}

	#[allow(dead_code)]
	#[derive(Debug, serde::Deserialize)]
	struct Config {
		port: Port,
		ports: Vec<Port>,
	}

	let error = soml::from_str::<Config>("ports: [1]\n\nport: 0").unwrap_err();
	assert_eq!(error.to_string(), "port 0 is reserved at line 3, column 7");

	let error = soml::from_str::<Config>("port: 1\nports: [\n\t1,\n\t0,\n]").unwrap_err();
	assert_eq!(error.to_string(), "port 0 is reserved at line 4, column 2");

	// The value of a newtype variant, not the variant's object.
	#[allow(dead_code)]
	#[derive(Debug, serde::Deserialize)]
	enum Listen {
		Port(Port),
	}

	let error = soml::from_str::<Vec<Listen>>("[{Port: 0}]").unwrap_err();
	assert_eq!(error.to_string(), "port 0 is reserved at line 1, column 9");
}

#[test]
fn an_instant_or_a_duration_does_not_read_from_a_key() {
	assert!(
		soml::from_str::<std::collections::BTreeMap<soml::Instant, i32>>(
			"x: {'2026-09-19T14:00:00Z': 1}"
		)
		.is_err()
	);
	assert!(soml::from_str::<std::collections::BTreeMap<soml::Duration, i32>>("'5s': 1").is_err());
}

#[test]
fn an_error_made_after_reading_the_whole_document_has_a_position() {
	#[allow(dead_code)]
	#[derive(Debug, serde::Deserialize)]
	#[serde(try_from = "std::collections::BTreeMap<String, i32>")]
	struct Checked(std::collections::BTreeMap<String, i32>);

	impl TryFrom<std::collections::BTreeMap<String, i32>> for Checked {
		type Error = String;

		fn try_from(map: std::collections::BTreeMap<String, i32>) -> Result<Self, String> {
			if map.len() > 1 {
				return Err("too many".to_owned());
			}

			Ok(Self(map))
		}
	}

	let error = soml::from_str::<Checked>("# Head\na: 1\nb: 2").unwrap_err();
	assert_eq!(error.to_string(), "too many at line 2, column 1");
}

#[test]
fn every_integer_key_type_reads_back() {
	let map = std::collections::BTreeMap::from([(u128::MAX, 1), (0, 2)]);
	let text = soml::to_string(&map).expect("written");
	assert_eq!(
		soml::from_str::<std::collections::BTreeMap<u128, i32>>(&text).expect("read"),
		map
	);

	let map = std::collections::BTreeMap::from([(i128::MIN, 1), (-1, 2)]);
	let text = soml::to_string(&map).expect("written");
	assert_eq!(
		soml::from_str::<std::collections::BTreeMap<i128, i32>>(&text).expect("read"),
		map
	);
}

#[test]
fn to_value_refuses_a_carriage_return_in_a_field_or_variant_name() {
	#[derive(serde::Serialize)]
	struct Renamed {
		#[serde(rename = "x\ry")]
		field: i32,
	}

	#[derive(serde::Serialize)]
	enum Variant {
		#[serde(rename = "a\rb")]
		Unit,
		#[serde(rename = "c\rd")]
		Newtype(i32),
	}

	assert!(soml::to_value(&Renamed { field: 1 }).is_err());
	assert!(soml::to_value(&Variant::Unit).is_err());
	assert!(soml::to_value(&Variant::Newtype(1)).is_err());
}

#[test]
fn a_key_that_reads_as_an_instant_or_a_duration_is_still_only_a_string() {
	// The key's text is a valid instant or duration, so only the rule that a key is a string refuses it.
	let error = soml::from_str::<std::collections::BTreeMap<soml::Instant, i32>>(
		"'2026-09-19T14:00:00Z': 1",
	)
	.unwrap_err();
	assert_eq!(
		error.to_string(),
		"A key is a string, so it cannot be read as an instant at line 1, column 1"
	);
	let error =
		soml::from_str::<std::collections::BTreeMap<soml::Duration, i32>>("5s: 1").unwrap_err();
	assert_eq!(
		error.to_string(),
		"A key is a string, so it cannot be read as a duration at line 1, column 1"
	);
}

#[test]
fn an_instant_or_a_duration_cannot_be_written_as_a_key() {
	let instant: soml::Instant = "2026-09-19T14:00:00Z".parse().expect("an instant");
	assert!(
		soml::to_string(&std::collections::BTreeMap::from([(instant, 1)]))
			.unwrap_err()
			.to_string()
			.ends_with("not an instant")
	);
	assert!(
		soml::to_string(&std::collections::BTreeMap::from([(
			soml::Duration::from_nanoseconds(1),
			1
		)]))
		.unwrap_err()
		.to_string()
		.ends_with("not a duration")
	);
}

#[test]
fn an_enum_variant_reads_from_a_string_with_escapes() {
	#[derive(Deserialize, Debug, PartialEq, Eq, PartialOrd, Ord)]
	enum Mode {
		Fast,
	}

	assert_eq!(
		read::<Mode>("a: \"\\u{46}ast\"").expect("a variant"),
		Mode::Fast
	);
	assert_eq!(
		read::<std::collections::BTreeMap<Mode, i32>>("a: {\"\\u{46}ast\": 1}")
			.expect("a variant key"),
		std::collections::BTreeMap::from([(Mode::Fast, 1)])
	);
}

#[test]
fn a_user_struct_named_duration_that_is_not_a_std_duration_is_an_object() {
	#[derive(Serialize, Deserialize, Debug, PartialEq)]
	struct Duration {
		secs: i64,
		nanos: u32,
	}

	// A negative second count or a nanosecond count of a second or more is not the shape of a `std::time::Duration`.
	for duration in [
		Duration { secs: -1, nanos: 0 },
		Duration {
			secs: 1,
			nanos: 1_000_000_000,
		},
	] {
		let text = written(&duration);
		assert!(text.starts_with('{'), "{text:?}");
		assert_eq!(
			read::<Duration>(&format!("a: {text}")).expect("an object"),
			duration
		);
	}
}
