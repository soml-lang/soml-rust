/*!
The language-neutral conformance suite in `tests/conformance`, a copy of the suite in the [`soml` spec repository](https://github.com/soml-lang/soml/tree/main/conformance). To update it, check out that repository next to this one and run `../soml/sync-conformance.sh tests/conformance`.

- `valid/**/name.soml` must parse to the tagged value in `name.json`, also be accepted when every value is skipped, serialize with `to_string_canonical` to exactly `name.canonical.soml`, and format to exactly `name.formatted.soml`.
- `invalid/**/name.soml` must be rejected for the reason, and at the line and column, in `invalid-reasons.json`, also when every value is skipped.
- `edit/name.json` holds a formatted `document`, a `path`, a tagged `value`, which is left out for a removal, and the `expected` document after the change, which `Document::set` or `Document::remove` must give exactly, or `error: true` when the change must fail because its path does not fit the document.

Every case runs, and the test reports every failure at once.
*/

use soml::{LineColumn, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/conformance")
}

/**
The case names under `directory`, such as `string/escapes`, sorted.
*/
fn cases(directory: &str) -> Vec<String> {
	fn walk(directory: &Path, base: &Path, names: &mut Vec<String>) {
		for entry in fs::read_dir(directory).expect("the corpus directory exists") {
			let path = entry.expect("the corpus directory is readable").path();

			if path.is_dir() {
				walk(&path, base, names);
				continue;
			}

			let relative = path
				.strip_prefix(base)
				.expect("the path is in the corpus")
				.to_string_lossy()
				.into_owned();

			// The canonical and formatted forms are companions of a case, not cases.
			if let Some(name) = relative.strip_suffix(".soml")
				&& !name.ends_with(".canonical")
				&& !name.ends_with(".formatted")
			{
				names.push(name.to_owned());
			}
		}
	}

	let base = root().join(directory);
	let mut names = Vec::new();
	walk(&base, &base, &mut names);
	names.sort();
	names
}

/**
Converts the tagged JSON of the suite, where a scalar is `{"type": …, "value": …}`, into a `Value`.
*/
fn tagged(json: &serde_json::Value) -> Value {
	match json {
		serde_json::Value::Array(items) => Value::Array(items.iter().map(tagged).collect()),
		serde_json::Value::Object(object) => {
			if object.len() == 2
				&& let Some(serde_json::Value::String(kind)) = object.get("type")
				&& let Some(serde_json::Value::String(text)) = object.get("value")
			{
				return match kind.as_str() {
					"string" => Value::String(text.clone()),
					"int" => Value::Int(text.parse().expect("an int")),
					"float" => Value::Float(match text.as_str() {
						"infinity" => f64::INFINITY,
						"-infinity" => f64::NEG_INFINITY,
						text => text.parse().expect("a float"),
					}),
					"bool" => Value::Bool(text == "true"),
					"null" => Value::Null,
					"instant" => Value::Instant(text.parse().expect("an instant")),
					"duration" => Value::Duration(soml::Duration::from_nanoseconds(
						text.parse().expect("a duration"),
					)),
					kind => panic!("Unknown tag {kind}"),
				};
			}

			Value::Object(
				object
					.iter()
					.map(|(key, value)| (key.clone(), tagged(value)))
					.collect(),
			)
		}
		_ => panic!("The tagged JSON holds only objects, arrays, and strings, not {json}"),
	}
}

/**
Floats compare by their bits, so that a stray negative zero is caught.
*/
fn same(left: &Value, right: &Value) -> bool {
	match (left, right) {
		(Value::Float(left), Value::Float(right)) => left.to_bits() == right.to_bits(),
		(Value::Array(left), Value::Array(right)) => {
			left.len() == right.len()
				&& left
					.iter()
					.zip(right)
					.all(|(left, right)| same(left, right))
		}
		(Value::Object(left), Value::Object(right)) => {
			left.len() == right.len()
				&& left
					.iter()
					.zip(right)
					.all(|((left_key, left), (right_key, right))| {
						left_key == right_key && same(left, right)
					})
		}
		_ => left == right,
	}
}

#[test]
fn the_suite_is_there() {
	assert!(cases("valid").len() > 300);
	assert!(cases("invalid").len() > 300);
}

#[test]
fn valid_cases_read_to_their_value_and_write_their_canonical_form() {
	let mut failures = Vec::new();

	for name in cases("valid") {
		let base = root().join("valid").join(&name);
		let bytes = fs::read(base.with_extension("soml")).expect("the case exists");
		let expected = tagged(
			&serde_json::from_slice(
				&fs::read(base.with_extension("json")).expect("the value exists"),
			)
			.expect("valid JSON"),
		);
		let canonical = fs::read_to_string(base.with_extension("canonical.soml"))
			.expect("the canonical form exists");

		let value = match soml::from_slice::<Value>(&bytes) {
			Ok(value) => value,
			Err(error) => {
				failures.push(format!("{name}: rejected: {error}"));
				continue;
			}
		};

		if !same(&value, &expected) {
			failures.push(format!("{name}: read {value:?}, expected {expected:?}"));
			continue;
		}

		// A type that skips every value must still check the whole document.
		if let Err(error) = soml::from_slice::<serde::de::IgnoredAny>(&bytes) {
			failures.push(format!("{name}: rejected when skipped: {error}"));
			continue;
		}

		match soml::to_string_canonical(&value) {
			Ok(written) if written == canonical => {}
			Ok(written) => {
				failures.push(format!("{name}: wrote {written:?}, expected {canonical:?}"));
				continue;
			}
			Err(error) => {
				failures.push(format!("{name}: could not write: {error}"));
				continue;
			}
		}

		// Canonical form is a valid document with the same value, and a fixed point.
		match soml::from_str::<Value>(&canonical) {
			Ok(reread)
				if same(&reread, &expected)
					&& soml::to_string_canonical(&reread).ok().as_ref() == Some(&canonical) => {}
			Ok(reread) => failures.push(format!("{name}: the canonical form reads as {reread:?}")),
			Err(error) => failures.push(format!("{name}: the canonical form is rejected: {error}")),
		}
	}

	assert!(
		failures.is_empty(),
		"{} failures:\n{}",
		failures.len(),
		failures.join("\n")
	);
}

#[test]
fn valid_cases_format_to_their_formatted_form() {
	let mut failures = Vec::new();

	for name in cases("valid") {
		let base = root().join("valid").join(&name);
		let text = fs::read_to_string(base.with_extension("soml")).expect("the case exists");
		let expected = tagged(
			&serde_json::from_slice(
				&fs::read(base.with_extension("json")).expect("the value exists"),
			)
			.expect("valid JSON"),
		);
		let formatted = fs::read_to_string(base.with_extension("formatted.soml"))
			.expect("the formatted form exists");

		match soml::format(&text) {
			Ok(written) if written == formatted => {}
			Ok(written) => {
				failures.push(format!(
					"{name}: formatted {written:?}, expected {formatted:?}"
				));
				continue;
			}
			Err(error) => {
				failures.push(format!("{name}: could not format: {error}"));
				continue;
			}
		}

		// The formatted form is a valid document with the same value, and a fixed point.
		match soml::from_str::<Value>(&formatted) {
			Ok(reread) if same(&reread, &expected) => {}
			Ok(reread) => failures.push(format!("{name}: the formatted form reads as {reread:?}")),
			Err(error) => failures.push(format!("{name}: the formatted form is rejected: {error}")),
		}

		if soml::format(&formatted).ok().as_ref() != Some(&formatted) {
			failures.push(format!("{name}: the formatted form is not a fixed point"));
		}
	}

	assert!(
		failures.is_empty(),
		"{} failures:\n{}",
		failures.len(),
		failures.join("\n")
	);
}

#[derive(serde::Deserialize)]
struct Reason {
	reason: String,
	line: usize,
	column: usize,
}

#[test]
fn invalid_cases_are_rejected_where_the_reference_rejects_them() {
	let reasons: BTreeMap<String, Reason> = serde_json::from_slice(
		&fs::read(root().join("invalid-reasons.json")).expect("the reasons exist"),
	)
	.expect("valid JSON");
	let mut failures = Vec::new();

	for name in cases("invalid") {
		let bytes = fs::read(root().join("invalid").join(&name).with_extension("soml"))
			.expect("the case exists");

		let error = match soml::from_slice::<Value>(&bytes) {
			Ok(value) => {
				failures.push(format!("{name}: accepted as {value:?}"));
				continue;
			}
			Err(error) => error,
		};

		// A type that skips every value must get the same error.
		match soml::from_slice::<serde::de::IgnoredAny>(&bytes) {
			Ok(_) => failures.push(format!("{name}: accepted when skipped")),
			Err(skipped) if skipped.to_string() != error.to_string() => {
				failures.push(format!(
					"{name}: {skipped} when skipped, but {error} when read"
				));
			}
			Err(_) => {}
		}

		let reason = reasons
			.get(&name)
			.unwrap_or_else(|| panic!("{name} has no reason"));

		if error.kind() != soml::ErrorKind::Syntax {
			failures.push(format!("{name}: {error} has the kind {:?}", error.kind()));
		}

		let expected = LineColumn {
			line: reason.line,
			column: reason.column,
		};

		if error.position() != Some(expected) || error.message() != reason.reason {
			failures.push(format!(
				"{name}: {error}, but the reference says {} at line {}, column {}",
				reason.reason, reason.line, reason.column
			));
		}
	}

	assert!(
		failures.is_empty(),
		"{} failures:\n{}",
		failures.len(),
		failures.join("\n")
	);
}

/**
A change to a document, from `edit/`.
*/
#[derive(serde::Deserialize)]
struct EditCase {
	document: String,
	path: Vec<serde_json::Value>,
	value: Option<serde_json::Value>,
	expected: Option<String>,
	#[serde(default)]
	error: bool,
}

#[test]
fn edit_cases_give_their_expected_document() {
	let directory = root().join("edit");
	let mut names: Vec<String> = fs::read_dir(&directory)
		.expect("the edit cases exist")
		.filter_map(|entry| {
			let name = entry.expect("the edit cases are readable").file_name();
			name.to_str()?.strip_suffix(".json").map(str::to_owned)
		})
		.collect();
	names.sort();
	assert!(names.len() > 40);
	let mut failures = Vec::new();

	for name in names {
		let case: EditCase = serde_json::from_slice(
			&fs::read(directory.join(&name).with_extension("json")).expect("the case exists"),
		)
		.expect("valid JSON");

		if soml::format(&case.document).ok().as_ref() != Some(&case.document) {
			failures.push(format!("{name}: the document is not formatted"));
			continue;
		}

		let path: Vec<soml::PathSegment<'_>> = case
			.path
			.iter()
			.map(|segment| match segment {
				serde_json::Value::String(key) => soml::PathSegment::Key(key),
				serde_json::Value::Number(index) => soml::PathSegment::Index(
					index
						.as_u64()
						.and_then(|index| usize::try_from(index).ok())
						.expect("an index"),
				),
				_ => panic!("A path holds keys and indexes, not {segment}"),
			})
			.collect();
		let mut document: soml::Document = case.document.parse().expect("a valid document");

		let result = match &case.value {
			Some(value) => document.set(path, tagged(value)),
			None => document.remove(path).map(|_| ()),
		};

		if case.error {
			match result {
				// A failed change leaves the document as it was.
				Err(_) if document.to_string() == case.document => {}
				Err(_) => failures.push(format!("{name}: failed, but changed the document")),
				Ok(()) => failures.push(format!(
					"{name}: gave {:?}, expected an error",
					document.to_string()
				)),
			}

			continue;
		}

		let expected = case
			.expected
			.expect("a case without an error has an expected document");

		match result {
			Ok(()) if document.to_string() == expected => {}
			Ok(()) => failures.push(format!(
				"{name}: gave {:?}, expected {:?}",
				document.to_string(),
				expected
			)),
			Err(error) => failures.push(format!("{name}: {error}")),
		}

		if soml::format(&expected).ok().as_ref() != Some(&expected) {
			failures.push(format!("{name}: the expected document is not formatted"));
		}
	}

	assert!(
		failures.is_empty(),
		"{} failures:\n{}",
		failures.len(),
		failures.join("\n")
	);
}
