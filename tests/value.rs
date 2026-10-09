/*!
The `Value` API: lookups, accessors, conversions, and parsing.
*/

#![allow(clippy::tabs_in_doc_comments)]

use soml::{Duration, Instant, Object, Value};

fn document() -> Value {
	"port: 8080\nratio: 0.5\nname: 'x'\non: true\nnothing: null\nat: 2026-09-19T14:00:00Z\ntook: 1h\nhosts: ['a', 'b']\nserver: {port: 1}"
		.parse()
		.expect("a valid document")
}

#[test]
fn get_finds_a_member_by_key() {
	let value = document();
	assert_eq!(value.get("port"), Some(&Value::Int(8080)));
	assert_eq!(value.get("missing"), None);
	assert_eq!(value.get(&String::from("port")), Some(&Value::Int(8080)));
	assert_eq!(
		value.get("server").and_then(|server| server.get("port")),
		Some(&Value::Int(1))
	);
}

#[test]
fn get_finds_an_item_by_index() {
	let value = document();
	let hosts = value.get("hosts").expect("hosts");
	assert_eq!(hosts.get(0), Some(&Value::from("a")));
	assert_eq!(hosts.get(1), Some(&Value::from("b")));
	assert_eq!(hosts.get(2), None);
}

#[test]
fn get_is_none_for_the_wrong_kind_of_value() {
	let value = document();
	assert_eq!(value.get(0), None);
	assert_eq!(value.get("hosts").and_then(|hosts| hosts.get("a")), None);
	assert_eq!(value.get("port").and_then(|port| port.get("a")), None);
	assert_eq!(value.get("port").and_then(|port| port.get(0)), None);
}

#[test]
fn get_mut_changes_a_member_and_an_item() {
	let mut value = document();

	if let Some(port) = value.get_mut("port") {
		*port = Value::Int(9090);
	}

	if let Some(host) = value.get_mut("hosts").and_then(|hosts| hosts.get_mut(1)) {
		*host = Value::from("c");
	}

	assert_eq!(value.get("port"), Some(&Value::Int(9090)));
	assert_eq!(
		value.get("hosts").and_then(|hosts| hosts.get(1)),
		Some(&Value::from("c"))
	);
	assert_eq!(value.get_mut("missing"), None);

	// A `&String` indexes as its text.
	let key = "port".to_owned();
	*value.get_mut(&key).expect("a member") = Value::Int(1);
	assert_eq!(value.get(&key), Some(&Value::Int(1)));
	assert_eq!(value.get_mut(&"missing".to_owned()), None);
}

#[test]
fn accessors_return_their_own_type_only() {
	let value = document();
	let member = |key: &str| value.get(key).expect("the member exists");

	assert_eq!(member("port").as_i64(), Some(8080));
	assert_eq!(member("ratio").as_f64(), Some(0.5));
	assert_eq!(member("name").as_str(), Some("x"));
	assert_eq!(member("on").as_bool(), Some(true));
	assert!(member("nothing").is_null());
	assert_eq!(
		member("at").as_instant().map(Instant::unix_seconds),
		Some(1_789_826_400)
	);
	assert_eq!(
		member("took").as_duration().map(Duration::nanoseconds),
		Some(3_600_000_000_000)
	);
	assert_eq!(member("hosts").as_array().map(Vec::len), Some(2));
	assert_eq!(member("server").as_object().map(Object::len), Some(1));

	assert_eq!(member("name").as_i64(), None);
	assert_eq!(member("port").as_str(), None);
	assert_eq!(member("port").as_bool(), None);
	assert!(!member("port").is_null());
	assert_eq!(member("name").as_instant(), None);
	assert_eq!(member("at").as_duration(), None);
	assert_eq!(member("at").as_str(), None);
	assert_eq!(member("server").as_array(), None);
	assert_eq!(member("hosts").as_object(), None);
}

#[test]
fn as_f64_does_not_convert_an_int() {
	assert_eq!(Value::Int(1).as_f64(), None);
	assert_eq!(Value::Float(1.0).as_i64(), None);
}

#[test]
fn an_int_and_a_float_are_not_equal() {
	assert_ne!(Value::Int(3), Value::Float(3.0));
}

#[test]
fn mutable_accessors_change_a_collection() {
	let mut value = document();

	if let Some(hosts) = value.get_mut("hosts").and_then(Value::as_array_mut) {
		hosts.push(Value::from("c"));
	}

	if let Some(object) = value.as_object_mut() {
		object.remove("server");
	}

	assert_eq!(
		value.get("hosts").and_then(Value::as_array).map(Vec::len),
		Some(3)
	);
	assert_eq!(value.get("server"), None);
	assert_eq!(Value::Int(1).as_array_mut(), None);
	assert_eq!(Value::Int(1).as_object_mut(), None);
}

#[test]
fn from_converts_primitives() {
	assert_eq!(Value::from(1i8), Value::Int(1));
	assert_eq!(Value::from(1i16), Value::Int(1));
	assert_eq!(Value::from(1i32), Value::Int(1));
	assert_eq!(Value::from(i64::MIN), Value::Int(i64::MIN));
	assert_eq!(Value::from(255u8), Value::Int(255));
	assert_eq!(Value::from(65_535u16), Value::Int(65_535));
	assert_eq!(Value::from(u32::MAX), Value::Int(4_294_967_295));
	assert_eq!(Value::from(1.5), Value::Float(1.5));
	assert_eq!(Value::from(0.1f32), Value::Float(0.1));
	assert_eq!(Value::from(true), Value::Bool(true));
	assert_eq!(Value::from("x"), Value::String(String::from("x")));
	assert_eq!(
		Value::from(String::from("x")),
		Value::String(String::from("x"))
	);
	assert_eq!(Value::from(Instant::MIN), Value::Instant(Instant::MIN));
	assert_eq!(Value::from(Duration::MAX), Value::Duration(Duration::MAX));
}

#[test]
fn from_converts_collections_and_options() {
	assert_eq!(
		Value::from(vec![1, 2]),
		Value::Array(vec![Value::Int(1), Value::Int(2)])
	);
	assert_eq!(Value::from(["a", "b"]), Value::from(vec!["a", "b"]));
	assert_eq!(Value::from(Object::new()), Value::Object(Object::new()));
	assert_eq!(Value::from(None::<i32>), Value::Null);
	assert_eq!(Value::from(Some("x")), Value::from("x"));
	assert_eq!(Value::default(), Value::Null);
}

#[test]
fn from_iterator_builds_an_array_or_an_object() {
	let array: Value = (1..=3).collect();
	assert_eq!(
		array,
		Value::Array(vec![Value::Int(1), Value::Int(2), Value::Int(3)])
	);

	let object: Value = [("b", 2), ("a", 1)].into_iter().collect();
	assert_eq!(
		object,
		"a: 1\nb: 2".parse::<Value>().expect("a valid document")
	);

	let object: Value = vec![(String::from("a"), Value::Null)].into_iter().collect();
	assert_eq!(object.get("a"), Some(&Value::Null));
}

#[test]
fn from_str_parses_a_document() {
	let value: Value = "a: 1".parse().expect("a valid document");
	assert_eq!(value, [("a", 1)].into_iter().collect());
	assert_eq!(
		"5".parse::<Value>()
			.expect_err("a scalar is not a document")
			.to_string(),
		"A bare value is not a document. A document is an object or an array, so write it as `key: value` or `[value]` at line 1, column 1"
	);
}

#[test]
fn object_equality_ignores_member_order() {
	assert_eq!(
		"a: 1\nb: 2".parse::<Value>().expect("valid"),
		"b: 2\na: 1".parse::<Value>().expect("valid")
	);
}

#[test]
fn a_parsed_value_writes_back_to_the_same_value() {
	let value = document();
	let text = soml::to_string_canonical(&value).expect("a document");
	assert_eq!(
		text.parse::<Value>().expect("canonical form reads back"),
		value
	);
}

#[test]
fn an_f32_from_another_format_is_the_float_it_shows() {
	use serde::Deserialize;
	use serde::de::IntoDeserializer;

	let deserializer: serde::de::value::F32Deserializer<serde::de::value::Error> =
		0.1f32.into_deserializer();
	assert_eq!(Value::deserialize(deserializer), Ok(Value::from(0.1f32)));
	assert_eq!(Value::from(0.1f32), Value::Float(0.1));
}
