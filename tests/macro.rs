/*!
The `soml!` macro.
*/

use soml::{Value, soml};

#[test]
fn builds_every_kind_of_value() {
	let instant: soml::Instant = "2026-09-19T14:00:00Z".parse().expect("valid");

	let value = soml!({
		name: "api",
		"deployed-at": instant,
		replicas: 3,
		negative: -3,
		ratio: 0.5,
		enabled: true,
		owner: null,
		empty: {},
		none: [],
		tags: ["a", "b",],
		nested: {list: [[1, 2], {x: null}]},
	});

	let expected: Value = "
		name: 'api'
		deployed-at: 2026-09-19T14:00:00Z
		replicas: 3
		negative: -3
		ratio: 0.5
		enabled: true
		owner: null
		empty: {}
		none: []
		tags: ['a', 'b']
		nested: {list: [[1, 2], {x: null}]}
	"
	.parse()
	.expect("valid");

	assert_eq!(value, expected);
}

#[test]
fn a_value_can_be_any_expression() {
	let numbers = vec![1, 2, 3];

	let value = soml!({
		sum: numbers.iter().sum::<i32>(),
		call: std::cmp::max(1, 2),
		list: numbers.clone(),
		macro_call: vec![4, 5].len() as i64,
		grouped: (1 + 2) * 3,
		generic: std::collections::HashMap::<String, i32>::new().len() as i64,
	});

	assert_eq!(value.get("sum"), Some(&Value::Int(6)));
	assert_eq!(value.get("call"), Some(&Value::Int(2)));
	assert_eq!(value.get("list"), Some(&Value::from(vec![1, 2, 3])));
	assert_eq!(value.get("macro_call"), Some(&Value::Int(2)));
	assert_eq!(value.get("grouped"), Some(&Value::Int(9)));
	assert_eq!(value.get("generic"), Some(&Value::Int(0)));
}

#[test]
fn builds_scalars_and_top_level_arrays() {
	assert_eq!(soml!(null), Value::Null);
	assert_eq!(soml!(1.5), Value::Float(1.5));
	assert_eq!(
		soml!([1, "two", null]),
		Value::Array(vec![Value::Int(1), Value::from("two"), Value::Null])
	);
}

#[test]
fn a_long_literal_fits_the_default_recursion_limit() {
	let value = soml!([
		0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9,
		0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9,
		0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9,
		0, 1, 2, 3, 4, 5, 6, 7, 8, 9,
	]);

	assert_eq!(value.as_array().map(Vec::len), Some(100));
}

#[test]
#[should_panic(expected = "Duplicate key “a” in soml!")]
fn a_duplicate_key_panics() {
	let _ = soml!({a: 1, a: 2});
}

#[test]
fn a_raw_identifier_key_has_no_prefix() {
	let value = soml!({r#type: 1, r#match: 2, plain: 3});
	assert_eq!(
		value
			.as_object()
			.map(|object| object.keys().cloned().collect::<Vec<_>>()),
		Some(vec![
			"match".to_owned(),
			"plain".to_owned(),
			"type".to_owned()
		])
	);
}

#[test]
fn a_long_object_fits_the_default_recursion_limit() {
	let value = soml!({
		k0: 0, k1: 1, k2: 2, k3: 3, k4: 4, k5: 5, k6: 6, k7: 7, k8: 8, k9: 9, k10: 0, k11: 1, k12: 2, k13: 3, k14: 4, k15: 5, k16: 6, k17: 7, k18: 8, k19: 9,
		k20: 0, k21: 1, k22: 2, k23: 3, k24: 4, k25: 5, k26: 6, k27: 7, k28: 8, k29: 9, k30: 0, k31: 1, k32: 2, k33: 3, k34: 4, k35: 5, k36: 6, k37: 7, k38: 8, k39: 9,
		k40: 0, k41: 1, k42: 2, k43: 3, k44: 4, k45: 5, k46: 6, k47: 7, k48: 8, k49: 9, k50: 0, k51: 1, k52: 2, k53: 3, k54: 4, k55: 5, k56: 6, k57: 7, k58: 8, k59: 9,
		k60: 0, k61: 1, k62: 2, k63: 3, k64: 4, k65: 5, k66: 6, k67: 7, k68: 8, k69: 9, k70: 0, k71: 1, k72: 2, k73: 3, k74: 4, k75: 5, k76: 6, k77: 7, k78: 8, k79: 9,
		k80: 0, k81: 1, k82: 2, k83: 3, k84: 4, k85: 5, k86: 6, k87: 7, k88: 8, k89: 9, k90: 0, k91: 1, k92: 2, k93: 3, k94: 4, k95: 5, k96: 6, k97: 7, k98: 8, k99: 9,
	});

	assert_eq!(value.as_object().map(soml::Object::len), Some(100));
}
