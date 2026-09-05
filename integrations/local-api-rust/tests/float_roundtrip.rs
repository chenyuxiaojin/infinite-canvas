use serde_json::Value;

#[test]
fn external_json_coordinates_keep_compiler_expected_bits_through_api_serialization() {
    // Avoid constructing the input with json!: the bug occurs while parsing external decimal text.
    let source = r#"{"viewport":{"y":222.56302150354014,"x":-222.56302150354014,"k":0.10000000000000002},"node":{"x":1.0000000000000002}}"#;
    let expected = [
        ("/viewport/y", 222.56302150354014_f64.to_bits()),
        ("/viewport/x", (-222.56302150354014_f64).to_bits()),
        ("/viewport/k", 0.10000000000000002_f64.to_bits()),
        ("/node/x", 1.0000000000000002_f64.to_bits()),
    ];
    let mut raw = source.to_owned();
    for round in 0..50 {
        let value: Value = serde_json::from_str(&raw).unwrap();
        for (pointer, bits) in expected {
            assert_eq!(
                value.pointer(pointer).unwrap().as_f64().unwrap().to_bits(),
                bits,
                "round {round}: {pointer}"
            );
        }
        raw = serde_json::to_string(&value).unwrap();
    }
}
