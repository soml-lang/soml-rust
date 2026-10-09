#![no_main]

use libfuzzer_sys::fuzz_target;

/*
The syntax tree accepts exactly what the reader accepts, prints back the exact text, and has the same value.
*/
fuzz_target!(|text: &str| {
	let read = soml::from_str::<soml::Value>(text);
	let document = text.parse::<soml::Document>();
	assert_eq!(read.is_ok(), document.is_ok());

	let (Ok(value), Ok(document)) = (read, document) else {
		return;
	};

	assert_eq!(document.to_string(), text);
	assert_eq!(document.to_value().expect("an unchanged document is valid"), value);
	let _ = document.comments();
});
