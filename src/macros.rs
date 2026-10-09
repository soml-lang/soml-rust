/**
Builds a [`Value`](crate::Value) with JSON-like syntax.

```rust
let value = soml::soml!({
	name: "api",
	"deployed-at": "2026-09-19T14:00:00Z".parse::<soml::Instant>()?,
	replicas: 3,
	ratio: 0.5,
	tags: ["prod", "eu-west"],
	owner: null,
	limits: {cpu: 250, memory: 512},
});

assert_eq!(soml::to_string(&value)?, "\
deployed-at: 2026-09-19T14:00:00Z
limits: {
	cpu: 250
	memory: 512
}
name: 'api'
owner: null
ratio: 0.5
replicas: 3
tags: [
	'prod'
	'eu-west'
]
");
# Ok::<(), soml::Error>(())
```

- A key is an identifier, such as `name` or `r#type`, which gives the key `type`, or a string literal, such as `"deployed-at"` for a key that is not a Rust identifier.
- A value is `null`, a nested `{…}` or `[…]`, or any expression with a `From` conversion to `Value`, such as an `i32`, an `i64`, an `f64`, a `bool`, a string, an [`Instant`](crate::Instant), or a [`Duration`](crate::Duration).
- A key that is there twice panics, because SOML treats a duplicate key as an error, and a value that only keeps the last one would hide a mistake.
- The macro reads one item or member at a time, so a literal with more than about 120 of them at one level reaches the compiler's recursion limit, as with `json!`. Raise it with `#![recursion_limit = "256"]` in your crate.
*/
#[macro_export]
macro_rules! soml {
	($($value:tt)+) => {
		$crate::__soml_value!($($value)+)
	};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __soml_value {
	(null) => {
		$crate::Value::Null
	};
	([]) => {
		$crate::Value::Array(::std::vec::Vec::new())
	};
	([ $($items:tt)+ ]) => {
		$crate::Value::Array({
			let mut items = ::std::vec::Vec::new();
			$crate::__soml_array!(items $($items)+);
			items
		})
	};
	({}) => {
		$crate::Value::Object($crate::Object::new())
	};
	({ $($members:tt)+ }) => {
		$crate::Value::Object({
			let mut object = $crate::Object::new();
			$crate::__soml_object!(object $($members)+);
			object
		})
	};
	($value:expr) => {
		$crate::Value::from($value)
	};
}

/**
Adds the items one at a time. `null`, `[…]`, and `{…}` come before a general expression, which would take them as Rust.
*/
#[doc(hidden)]
#[macro_export]
macro_rules! __soml_array {
	($items:ident) => {};
	($items:ident null $(, $($rest:tt)*)?) => {
		$items.push($crate::Value::Null);
		$($crate::__soml_array!($items $($rest)*);)?
	};
	($items:ident [$($array:tt)*] $(, $($rest:tt)*)?) => {
		$items.push($crate::__soml_value!([$($array)*]));
		$($crate::__soml_array!($items $($rest)*);)?
	};
	($items:ident {$($object:tt)*} $(, $($rest:tt)*)?) => {
		$items.push($crate::__soml_value!({$($object)*}));
		$($crate::__soml_array!($items $($rest)*);)?
	};
	($items:ident $value:expr $(, $($rest:tt)*)?) => {
		$items.push($crate::Value::from($value));
		$($crate::__soml_array!($items $($rest)*);)?
	};
}

/**
Adds the members one at a time, each in one step, so a long object fits the recursion limit as well as a long array. A key is an identifier or a string literal, and values are read as in `__soml_array`.
*/
#[doc(hidden)]
#[macro_export]
macro_rules! __soml_object {
	($object:ident) => {};
	($object:ident $key:tt : null $(, $($rest:tt)*)?) => {
		$crate::__soml_insert!($object, $crate::__soml_key!($key), $crate::Value::Null);
		$($crate::__soml_object!($object $($rest)*);)?
	};
	($object:ident $key:tt : [$($array:tt)*] $(, $($rest:tt)*)?) => {
		$crate::__soml_insert!($object, $crate::__soml_key!($key), $crate::__soml_value!([$($array)*]));
		$($crate::__soml_object!($object $($rest)*);)?
	};
	($object:ident $key:tt : {$($members:tt)*} $(, $($rest:tt)*)?) => {
		$crate::__soml_insert!($object, $crate::__soml_key!($key), $crate::__soml_value!({$($members)*}));
		$($crate::__soml_object!($object $($rest)*);)?
	};
	($object:ident $key:tt : $value:expr $(, $($rest:tt)*)?) => {
		$crate::__soml_insert!($object, $crate::__soml_key!($key), $crate::Value::from($value));
		$($crate::__soml_object!($object $($rest)*);)?
	};
}

/**
A key: an identifier, without the `r#` of a raw identifier such as `r#type`, or a string literal.
*/
#[doc(hidden)]
#[macro_export]
macro_rules! __soml_key {
	($key:ident) => {{
		let key = ::std::stringify!($key);
		key.strip_prefix("r#").unwrap_or(key)
	}};
	($key:literal) => {
		$key
	};
}

/**
Adds a member, and panics when the key is already there.
*/
#[doc(hidden)]
#[macro_export]
macro_rules! __soml_insert {
	($object:ident, $key:expr, $value:expr) => {
		match $object.entry(::std::convert::Into::into($key)) {
			::std::collections::btree_map::Entry::Vacant(entry) => {
				entry.insert($value);
			}
			::std::collections::btree_map::Entry::Occupied(entry) => {
				::std::panic!("Duplicate key “{}” in soml!", entry.key());
			}
		}
	};
}
