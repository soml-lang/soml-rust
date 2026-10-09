#![no_main]

use libfuzzer_sys::fuzz_target;

/*
Any input is either rejected, or it reads as a value whose canonical form reads back as the same value and is a fixed point.
*/
fuzz_target!(|bytes: &[u8]| {
	let read = soml::from_slice::<soml::Value>(bytes);

	// serde reads a document in one pass, and `Value::from_str` reads the parsed tree, so the two must agree on every input, also on the error. A type that skips every value must too.
	if let Ok(text) = std::str::from_utf8(bytes) {
		let parsed = text.parse::<soml::Value>().map_err(|error| error.to_string());
		assert_eq!(read.as_ref().map_err(ToString::to_string), parsed.as_ref().map_err(String::clone));
		let skipped = soml::from_str::<serde::de::IgnoredAny>(text).map_err(|error| error.to_string());
		assert_eq!(skipped.map(|_| ()), parsed.map(|_| ()));
	}

	let Ok(value) = read else {
		return;
	};

	let canonical = soml::to_string_canonical(&value).expect("a value that was read can be written");
	// A `Value` keeps its members sorted, so `to_string`, which writes as it serializes, must give the same text as canonical form, which goes through the tree.
	assert_eq!(soml::to_string(&value).expect("written as it serializes"), canonical);
	let reread: soml::Value = canonical.parse().expect("canonical form can be read");
	assert_eq!(reread, value);
	assert_eq!(soml::to_string_canonical(&reread).expect("written again"), canonical);
});
