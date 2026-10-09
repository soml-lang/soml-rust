/*!
The nesting limit, which the spec fixes at exactly 100 levels. Every array and object is a level, including the document's own collection, braced or not.
*/

#![allow(clippy::tabs_in_doc_comments)]

use serde::{Deserialize, Serialize};
use soml::Value;

const LIMIT: usize = 100;

fn rejection(text: &str) -> String {
	match text.parse::<Value>() {
		Ok(_) => panic!("a document of {} bytes should be rejected", text.len()),
		Err(error) => error.to_string(),
	}
}

fn accepts(text: &str) -> bool {
	text.parse::<Value>().is_ok()
}

/**
`depth` arrays inside each other, as a whole document.
*/
fn arrays(depth: usize) -> String {
	format!("{}{}", "[".repeat(depth), "]".repeat(depth))
}

/**
`depth` braced objects inside each other, as a whole document.
*/
fn objects(depth: usize) -> String {
	format!("{}1{}", "{a: ".repeat(depth), "}".repeat(depth))
}

/**
A `Value` of `depth` arrays inside each other.
*/
fn nested_arrays(depth: usize) -> Value {
	let mut value = Value::Array(Vec::new());

	for _ in 1..depth {
		value = Value::Array(vec![value]);
	}

	value
}

/**
A `Value` of `depth` objects inside each other.
*/
fn nested_objects(depth: usize) -> Value {
	let mut value = Value::Object(soml::Object::new());

	for _ in 1..depth {
		value = [("a", value)].into_iter().collect();
	}

	value
}

#[test]
fn an_empty_array_has_a_depth_of_one() {
	assert!(accepts("[]"));
}

#[test]
fn accepts_arrays_nested_to_the_limit() {
	assert!(accepts(&arrays(LIMIT)));
}

#[test]
fn rejects_arrays_nested_past_the_limit_at_the_first_level_too_deep() {
	assert_eq!(
		rejection(&arrays(LIMIT + 1)),
		"The document is nested more than 100 levels deep at line 1, column 101"
	);
}

#[test]
fn accepts_braced_objects_nested_to_the_limit() {
	assert!(accepts(&objects(LIMIT)));
}

#[test]
fn rejects_braced_objects_nested_past_the_limit() {
	// Each level is `{a: `, four characters, so the 101st `{` is at column 401.
	assert_eq!(
		rejection(&objects(LIMIT + 1)),
		"The document is nested more than 100 levels deep at line 1, column 401"
	);
}

#[test]
fn the_brace_less_top_level_counts_as_a_level() {
	// `a: [...]` with 99 arrays is the top-level object plus 99 arrays, so 100 levels.
	assert!(accepts(&format!("a: {}", arrays(LIMIT - 1))));
	assert_eq!(
		rejection(&format!("a: {}", arrays(LIMIT))),
		"The document is nested more than 100 levels deep at line 1, column 103"
	);
}

#[test]
fn a_braced_top_level_reaches_the_limit_at_the_same_depth_as_a_brace_less_one() {
	assert!(accepts(&format!("{{a: {}}}", arrays(LIMIT - 1))));
	assert_eq!(
		rejection(&format!("{{a: {}}}", arrays(LIMIT))),
		"The document is nested more than 100 levels deep at line 1, column 104"
	);
}

#[test]
fn a_key_with_many_dots_is_an_error_not_nesting() {
	// The key is too long for the message to suggest a spelling, so it gets the general one.
	assert_eq!(
		rejection(&format!("{}: 1", vec!["a"; 100_000].join("."))),
		"A key cannot contain “.” unless it is quoted. Quote the whole key, or use braces to nest, as in a: {b: …} at line 1, column 2"
	);
}

#[test]
fn very_deep_arrays_are_an_error_not_a_crash() {
	assert_eq!(
		rejection(&arrays(100_000)),
		"The document is nested more than 100 levels deep at line 1, column 101"
	);
}

#[test]
fn deep_documents_read_into_a_recursive_type() {
	#[derive(Deserialize)]
	struct Node {
		a: Option<Box<Node>>,
	}

	let text = format!("{}null{}", "{a: ".repeat(LIMIT), "}".repeat(LIMIT));
	let mut node: Node = soml::from_str(&text).expect("100 levels are allowed");
	let mut depth = 1;

	while let Some(child) = node.a {
		node = *child;
		depth += 1;
	}

	assert_eq!(depth, LIMIT);
}

// Writing

#[test]
fn writes_arrays_nested_to_the_limit() {
	let text = soml::to_string(&nested_arrays(LIMIT)).expect("100 levels can be written");
	assert!(accepts(&text));
	assert_eq!(
		text.parse::<Value>().expect("it reads back"),
		nested_arrays(LIMIT)
	);
}

#[test]
fn refuses_to_write_arrays_nested_past_the_limit() {
	let error = soml::to_string(&nested_arrays(LIMIT + 1)).expect_err("101 levels cannot be read");
	assert_eq!(
		error.to_string(),
		"The value is nested more than 100 levels deep, so no reader would accept the document"
	);
	assert_eq!(error.position(), None);
}

#[test]
fn writes_objects_nested_to_the_limit() {
	let text = soml::to_string(&nested_objects(LIMIT)).expect("100 levels can be written");
	assert_eq!(
		text.parse::<Value>().expect("it reads back"),
		nested_objects(LIMIT)
	);
}

#[test]
fn refuses_to_write_objects_nested_past_the_limit() {
	let error = soml::to_string(&nested_objects(LIMIT + 1)).expect_err("101 levels cannot be read");
	assert_eq!(
		error.message(),
		"The value is nested more than 100 levels deep, so no reader would accept the document"
	);
}

#[test]
fn to_value_accepts_a_value_nested_to_the_limit() {
	assert_eq!(
		soml::to_value(&nested_arrays(LIMIT)).expect("100 levels"),
		nested_arrays(LIMIT)
	);
}

#[test]
fn to_value_refuses_a_value_nested_past_the_limit() {
	let error = soml::to_value(&nested_arrays(LIMIT + 1)).expect_err("101 levels");
	assert_eq!(
		error.message(),
		"The value is nested more than 100 levels deep, so no reader would accept the document"
	);
}

/**
A recursive type, for nesting through derived `Serialize`.
*/
#[derive(Serialize)]
struct Chain {
	next: Option<Box<Chain>>,
}

fn chain(depth: usize) -> Chain {
	let mut chain = Chain { next: None };

	for _ in 1..depth {
		chain = Chain {
			next: Some(Box::new(chain)),
		};
	}

	chain
}

#[test]
fn serializes_a_derived_type_nested_to_the_limit() {
	let text = soml::to_string(&chain(LIMIT)).expect("100 levels");
	assert!(accepts(&text));
	assert!(soml::to_value(&chain(LIMIT)).is_ok());
}

#[test]
fn refuses_a_derived_type_nested_past_the_limit() {
	assert!(soml::to_string(&chain(LIMIT + 1)).is_err());
	assert!(soml::to_value(&chain(LIMIT + 1)).is_err());
}

#[test]
fn a_variant_with_a_value_is_one_level_for_its_object() {
	#[derive(Serialize)]
	enum Wrapper {
		Tuple(Value, i32),
		Newtype(Value),
	}

	// A tuple variant is an object holding an array, so two levels around its fields.
	assert!(soml::to_value(&[Wrapper::Tuple(nested_arrays(LIMIT - 3), 1)]).is_ok());
	assert!(soml::to_value(&[Wrapper::Tuple(nested_arrays(LIMIT - 2), 1)]).is_err());

	// A newtype variant is an object holding the value, so one level around it.
	assert!(soml::to_value(&[Wrapper::Newtype(nested_arrays(LIMIT - 2))]).is_ok());
	assert!(soml::to_value(&[Wrapper::Newtype(nested_arrays(LIMIT - 1))]).is_err());
}

#[test]
fn bytes_are_an_array_level() {
	/**
	Bytes inside `arrays` arrays.
	*/
	struct Bytes {
		arrays: usize,
	}

	impl Serialize for Bytes {
		fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
			if self.arrays == 0 {
				return serializer.serialize_bytes(&[1, 2]);
			}

			[Self {
				arrays: self.arrays - 1,
			}]
			.serialize(serializer)
		}
	}

	assert_eq!(
		soml::to_value(&Bytes { arrays: 0 }).expect("bytes are an array"),
		Value::Array(vec![Value::Int(1), Value::Int(2)])
	);
	assert!(soml::to_value(&Bytes { arrays: LIMIT - 1 }).is_ok());
	assert!(soml::to_value(&Bytes { arrays: LIMIT }).is_err());
}

#[test]
fn to_value_refuses_a_struct_named_duration_past_the_limit() {
	#[derive(Serialize)]
	struct Duration {
		value: i32,
	}

	/**
	A `Duration` inside `arrays` arrays.
	*/
	struct Nested {
		arrays: usize,
	}

	impl Serialize for Nested {
		fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
			if self.arrays == 0 {
				return Duration { value: 1 }.serialize(serializer);
			}

			[Self {
				arrays: self.arrays - 1,
			}]
			.serialize(serializer)
		}
	}

	// 100 arrays and the object make 101 levels. `to_string` refuses it, so `to_value` must too.
	assert!(soml::to_string(&Nested { arrays: LIMIT }).is_err());
	assert!(soml::to_value(&Nested { arrays: LIMIT }).is_err());
}
