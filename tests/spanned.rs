/*!
`Spanned`: values with the byte range they were read from.
*/

use soml::{Spanned, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

fn text_of<'a, T>(text: &'a str, spanned: &Spanned<T>) -> &'a str {
	&text[spanned.span().expect("a span")]
}

#[test]
fn a_field_has_the_range_of_its_value() {
	#[derive(serde::Deserialize)]
	struct Config {
		name: Spanned<String>,
		port: Spanned<u16>,
		tags: Spanned<Vec<Spanned<String>>>,
	}

	let text = "name: 'api'\nport: 0x1F90 # The default.\ntags: [\n\t'a',\n\t\"b\",\n]\n";
	let config: Config = soml::from_str(text).expect("valid");
	assert_eq!(text_of(text, &config.name), "'api'");
	assert_eq!(text_of(text, &config.port), "0x1F90");
	assert_eq!(*config.port, 8080);
	assert_eq!(text_of(text, &config.tags), "[\n\t'a',\n\t\"b\",\n]");
	assert_eq!(text_of(text, &config.tags[1]), "\"b\"");
}

#[test]
fn the_root_spans_its_members_or_its_brackets() {
	let text = "# Head\na: 1\nb: [2] # Tail\n";
	let root: Spanned<Value> = soml::from_str(text).expect("valid");
	assert_eq!(text_of(text, &root), "a: 1\nb: [2]");

	let text = "  {a: 1}  ";
	let root: Spanned<BTreeMap<String, i32>> = soml::from_str(text).expect("valid");
	assert_eq!(text_of(text, &root), "{a: 1}");
}

#[test]
fn optional_and_map_values_have_spans() {
	#[derive(serde::Deserialize)]
	struct Config {
		missing: Option<Spanned<i32>>,
		present: Option<Spanned<i32>>,
		limits: BTreeMap<String, Spanned<i32>>,
	}

	let text = "present: 5\nlimits: {cpu: 1, memory: 22}";
	let config: Config = soml::from_str(text).expect("valid");
	assert!(config.missing.is_none());
	assert_eq!(
		text_of(text, config.present.as_ref().expect("present")),
		"5"
	);
	assert_eq!(text_of(text, &config.limits["memory"]), "22");
}

#[test]
fn a_spanned_value_keeps_its_soml_type() {
	#[derive(serde::Deserialize)]
	struct Config {
		at: Spanned<soml::Instant>,
		took: Spanned<soml::Duration>,
		timeout: Spanned<std::time::Duration>,
		any: Spanned<Value>,
	}

	let text = "at: 2026-09-19T14:00:00Z\ntook: 1m30s\ntimeout: 5s\nany: 2026-01-01T00:00:00Z";
	let config: Config = soml::from_str(text).expect("valid");
	assert_eq!(config.at.to_string(), "2026-09-19T14:00:00Z");
	assert_eq!(text_of(text, &config.took), "1m30s");
	assert_eq!(config.timeout.as_secs(), 5);
	assert!(matches!(*config.any, Value::Instant(_)));
}

#[test]
fn a_value_from_elsewhere_has_no_span() {
	let spanned: Spanned<i32> = soml::from_value(Value::Int(1)).expect("an int");
	assert_eq!((*spanned, spanned.span()), (1, None));

	let spanned: Spanned<i32> = serde_json::from_str("1").expect("an int");
	assert_eq!((*spanned, spanned.span()), (1, None));

	#[derive(serde::Deserialize)]
	struct Outer {
		#[serde(flatten)]
		inner: BTreeMap<String, Spanned<i32>>,
	}

	let outer: Outer = soml::from_str("a: 1").expect("valid");
	assert_eq!(outer.inner["a"].span(), None);
}

#[test]
fn a_value_read_through_a_json_value_has_no_span() {
	#[derive(serde::Deserialize)]
	struct Config {
		port: Spanned<u16>,
		hosts: Spanned<Vec<Spanned<String>>>,
		limits: Spanned<BTreeMap<String, Spanned<f64>>>,
		owner: Option<Spanned<String>>,
	}

	let json =
		serde_json::json!({"port": 80, "hosts": ["a"], "limits": {"cpu": 0.5}, "owner": null});
	let config: Config = serde_json::from_value(json).expect("valid");
	assert_eq!((*config.port, config.port.span()), (80, None));
	assert_eq!(*config.hosts[0], "a");
	assert_eq!(config.hosts[0].span(), None);
	assert_eq!(*config.limits["cpu"], 0.5);
	assert_eq!(config.limits.span(), None);
	assert!(config.owner.is_none());
}

#[test]
fn a_value_from_a_deserializer_that_reads_a_newtype_struct_as_its_content_has_no_span() {
	use serde::Deserialize;
	use serde::de::IntoDeserializer;
	use serde::de::value::{Error, MapDeserializer, SeqDeserializer};

	fn read<'de, T: Deserialize<'de>>(
		deserializer: impl serde::Deserializer<'de, Error = Error>,
	) -> Spanned<T> {
		let spanned = Spanned::<T>::deserialize(deserializer).expect("a value");
		assert_eq!(spanned.span(), None);
		spanned
	}

	// serde's own deserializers give a newtype struct's content, as some config and environment variable crates do.
	assert_eq!(*read::<u32>(5u32.into_deserializer()), 5);
	assert_eq!(*read::<i64>((-5i64).into_deserializer()), -5);
	assert_eq!(*read::<f64>(1.5f64.into_deserializer()), 1.5);
	assert!(*read::<bool>(true.into_deserializer()));
	assert_eq!(*read::<char>('é'.into_deserializer()), 'é');
	assert_eq!(*read::<String>("x".into_deserializer()), "x");
	assert_eq!(*read::<String>("y".to_owned().into_deserializer()), "y");
	assert_eq!(*read::<()>(().into_deserializer()), ());
	assert_eq!(*read::<Option<u8>>(().into_deserializer()), None);
	assert_eq!(
		*read::<Vec<u8>>(SeqDeserializer::new([1u8, 2].into_iter())),
		vec![1, 2]
	);

	#[derive(Deserialize, Debug, PartialEq)]
	enum Mode {
		Fast,
	}

	assert_eq!(*read::<Mode>("Fast".into_deserializer()), Mode::Fast);

	let map: BTreeMap<String, Spanned<u32>> = BTreeMap::deserialize(
		MapDeserializer::<_, Error>::new([("a".to_owned(), 5u32)].into_iter()),
	)
	.expect("a map");
	assert_eq!(*map["a"], 5);
}

#[test]
fn a_spanned_value_serializes_and_compares_as_its_value() {
	assert_eq!(
		soml::to_string(&BTreeMap::from([("a", Spanned::new(1))])).expect("written"),
		"a: 1\n"
	);
	let read: Spanned<i32> = soml::from_str::<BTreeMap<String, Spanned<i32>>>("a: 1")
		.expect("valid")
		.remove("a")
		.expect("a");
	assert_eq!(read, Spanned::new(1));
	assert_ne!(read, Spanned::new(2));
	assert!(read < Spanned::new(2));
	assert_eq!(read.cmp(&Spanned::new(0)), std::cmp::Ordering::Greater);
	assert_eq!(Spanned::new(f64::NAN).partial_cmp(&Spanned::new(1.0)), None);
}

#[test]
fn an_error_in_a_spanned_value_points_at_the_value() {
	#[allow(dead_code)]
	#[derive(Debug, serde::Deserialize)]
	struct Config {
		port: Spanned<u16>,
	}

	assert_eq!(
		soml::from_str::<Config>("\nport: 'x'")
			.unwrap_err()
			.to_string(),
		"invalid type: string \"x\", expected u16 at line 2, column 7"
	);
}

/**
A document as a tree of spanned values, read with a visitor of its own, because `#[serde(untagged)]` reads through serde's buffer, which has no spans.
*/
#[derive(serde::Deserialize)]
#[serde(transparent)]
struct Node(Spanned<Inner>);

enum Inner {
	Array(Vec<Node>),
	Object(BTreeMap<String, Node>),
	Scalar,
}

impl<'de> serde::Deserialize<'de> for Inner {
	fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
		struct InnerVisitor;

		impl<'de> serde::de::Visitor<'de> for InnerVisitor {
			type Value = Inner;

			fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
				formatter.write_str("any value")
			}

			fn visit_seq<A: serde::de::SeqAccess<'de>>(
				self,
				mut sequence: A,
			) -> Result<Inner, A::Error> {
				let mut items = Vec::new();

				while let Some(item) = sequence.next_element()? {
					items.push(item);
				}

				Ok(Inner::Array(items))
			}

			fn visit_map<A: serde::de::MapAccess<'de>>(
				self,
				mut map: A,
			) -> Result<Inner, A::Error> {
				let mut members = BTreeMap::new();

				while let Some((key, value)) = map.next_entry()? {
					members.insert(key, value);
				}

				Ok(Inner::Object(members))
			}

			fn visit_bool<E>(self, _value: bool) -> Result<Inner, E> {
				Ok(Inner::Scalar)
			}

			fn visit_i64<E>(self, _value: i64) -> Result<Inner, E> {
				Ok(Inner::Scalar)
			}

			fn visit_f64<E>(self, _value: f64) -> Result<Inner, E> {
				Ok(Inner::Scalar)
			}

			fn visit_str<E>(self, _value: &str) -> Result<Inner, E> {
				Ok(Inner::Scalar)
			}

			fn visit_unit<E>(self) -> Result<Inner, E> {
				Ok(Inner::Scalar)
			}
		}

		deserializer.deserialize_any(InnerVisitor)
	}
}

/**
The span of every value at its path.
*/
fn spans(text: &str) -> Vec<(Vec<soml::PathSegment<'static>>, std::ops::Range<usize>)> {
	let root: Node = soml::from_str(text).expect("valid");
	let mut output = Vec::new();
	let mut stack = vec![(Vec::new(), root)];

	while let Some((path, Node(node))) = stack.pop() {
		output.push((
			path.clone(),
			node.span().expect("a value from a document has a span"),
		));

		match node.into_inner() {
			Inner::Array(items) => {
				for (index, item) in items.into_iter().enumerate() {
					let mut path = path.clone();
					path.push(soml::PathSegment::Index(index));
					stack.push((path, item));
				}
			}
			Inner::Object(members) => {
				for (key, member) in members {
					let mut path = path.clone();
					path.push(soml::PathSegment::Key(Box::leak(key.into_boxed_str())));
					stack.push((path, member));
				}
			}
			Inner::Scalar => {}
		}
	}

	output
}

#[test]
fn spans_match_the_syntax_tree_on_the_corpus() {
	let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/conformance/valid");
	let mut checked = 0;

	for category in fs::read_dir(directory).expect("the corpus exists") {
		for entry in fs::read_dir(category.expect("a category").path()).expect("a category") {
			let path = entry.expect("a case").path();

			// The canonical and formatted forms are companions of a case, not cases.
			let Some(text) = path
				.to_str()
				.filter(|path| {
					path.ends_with(".soml")
						&& !path.ends_with(".canonical.soml")
						&& !path.ends_with(".formatted.soml")
				})
				.and_then(|_| fs::read_to_string(&path).ok())
			else {
				continue;
			};

			let document: soml::Document = text.parse().expect("valid");

			for (value_path, span) in spans(&text) {
				let node = document
					.get(value_path.clone())
					.expect("every value has a node");
				assert_eq!(
					node.span(),
					Some(span.clone()),
					"{} at {value_path:?}",
					path.display()
				);
				checked += 1;
			}
		}
	}

	assert!(checked > 1000, "{checked}");
}

#[test]
fn a_map_key_has_no_span_but_its_value_has_one() {
	let map: BTreeMap<Spanned<String>, Spanned<i32>> = soml::from_str("a: 1").expect("valid");
	let (key, value) = map.iter().next().expect("a member");
	assert_eq!((key.as_str(), key.span()), ("a", None));
	assert_eq!(value.span(), Some(3..4));
}

#[test]
fn a_spanned_value_displays_as_its_value() {
	let name: Spanned<String> = soml::from_str::<BTreeMap<String, Spanned<String>>>("name: 'api'")
		.expect("valid")
		.remove("name")
		.expect("a name");
	assert_eq!(name.to_string(), "api");
}
