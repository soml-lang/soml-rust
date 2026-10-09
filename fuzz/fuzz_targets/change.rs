#![no_main]

use libfuzzer_sys::fuzz_target;
use soml::{Document, PathSegment, Value};

/**
The paths to every value in a value, so the changes reach real places in the document.
*/
fn paths(value: &Value, prefix: &mut Vec<String>, output: &mut Vec<Vec<String>>) {
	match value {
		Value::Object(object) => {
			for (key, member) in object {
				prefix.push(key.clone());
				output.push(prefix.clone());
				paths(member, prefix, output);
				prefix.pop();
			}
		}
		Value::Array(items) => {
			for (index, item) in items.iter().enumerate() {
				prefix.push(format!("#{index}"));
				output.push(prefix.clone());
				paths(item, prefix, output);
				prefix.pop();
			}
		}
		_ => {}
	}
}

fn segments(path: &[String]) -> Vec<PathSegment<'_>> {
	path.iter().map(|segment| segment.strip_prefix('#').and_then(|index| index.parse().ok()).map_or(PathSegment::Key(segment), PathSegment::Index)).collect()
}

/*
Every `set` and `remove` that succeeds leaves a valid document with the expected value.
*/
fuzz_target!(|text: &str| {
	let Ok(mut document) = text.parse::<Document>() else {
		return;
	};

	let mut expected = document.to_value().expect("an unchanged document is valid");
	let mut all = Vec::new();
	paths(&expected, &mut Vec::new(), &mut all);

	for (step, path) in all.iter().take(8).enumerate() {
		let segments = segments(path);

		// A new member next to the value, in the same object.
		if step % 3 == 2 {
			let (_, parent) = segments.split_last().expect("a path is not empty");
			let mut path = parent.to_vec();
			let key = format!("new-{step}");
			path.push(PathSegment::Key(&key));

			if value_at(&mut expected, parent).is_some_and(|value| value.as_object().is_some_and(|object| !object.contains_key(&key))) && document.set(path.clone(), Value::Int(step as i64)).is_ok() {
				value_at(&mut expected, parent).and_then(Value::as_object_mut).expect("an object").insert(key.clone(), Value::Int(step as i64));
			}
		}
		// Only replacements: an earlier removal may have taken the path away, and then `set` would add it.
		else if step % 2 == 0 {
			if value_at(&mut expected, &segments).is_some() && document.set(segments.clone(), Value::Int(step as i64)).is_ok() {
				*value_at(&mut expected, &segments).expect("the path was found in the value") = Value::Int(step as i64);
			}
		} else if document.remove(segments.clone()).unwrap_or(false) {
			remove_at(&mut expected, &segments);
		}

		let printed = document.to_string();
		let value: Value = printed.parse().unwrap_or_else(|error| panic!("{error}\n{printed}"));
		assert_eq!(value, expected, "{printed}");
	}
});

fn value_at<'a>(value: &'a mut Value, path: &[PathSegment<'_>]) -> Option<&'a mut Value> {
	let mut value = value;

	for segment in path {
		value = match segment {
			PathSegment::Key(key) => value.get_mut(*key)?,
			PathSegment::Index(index) => value.get_mut(*index)?,
		};
	}

	Some(value)
}

fn remove_at(value: &mut Value, path: &[PathSegment<'_>]) {
	let (last, parents) = path.split_last().expect("a path is not empty");

	let Some(parent) = value_at(value, parents) else {
		return;
	};

	match (parent, last) {
		(Value::Object(object), PathSegment::Key(key)) => {
			object.remove(*key);
		}
		(Value::Array(items), PathSegment::Index(index)) => {
			items.remove(*index);
		}
		_ => {}
	}
}
