use assert_tv::{
    TestMode, TestValue, TestVector, TestVectorActive, TestVectorFileFormat, TestVectorNOP,
};
#[cfg(feature = "tls")]
use assert_tv::initialize_tv_case_from_file;
use rand::RngExt;
use serde_json::{Map, Value};
use std::io::Read;
use std::path::Path;
use std::str::FromStr;

fn custom_serialize_fn(value: &u64) -> anyhow::Result<serde_json::Value> {
    Ok(serde_json::Value::String(value.to_string()))
}

fn custom_deserialize_fn(value: &serde_json::Value) -> anyhow::Result<u64> {
    let value_as_astr: String =
        serde_json::from_value(value.clone()).map_err(anyhow::Error::from)?;
    u64::from_str(&value_as_astr).map_err(anyhow::Error::from)
}

struct SomeTestFields {
    a: TestValue<u64>,
    b: TestValue<String>,
}

impl assert_tv::TestVectorSet for SomeTestFields {
    fn start<TV: assert_tv::TestVector>() -> Self {
        Self {
            a: assert_tv::TestValue {
                name: Some(String::from("a")),
                description: Some(String::from("a is a u64")),
                test_value_field_code_location: format!("{}:{}", file!(), line!()),
                serializer: if TV::is_test_vector_enabled() {
                    Some(std::boxed::Box::new(custom_serialize_fn))
                } else {
                    None
                },
                deserializer: if TV::is_test_vector_enabled() {
                    Some(std::boxed::Box::new(custom_deserialize_fn))
                } else {
                    None
                },
                compress: true,
                offload: false,
                _data_marker: std::default::Default::default(),
            },
            b: assert_tv::TestValue {
                name: None,
                description: None,
                test_value_field_code_location: format!("{}:{}", file!(), line!()),
                serializer: Some(Box::new(|v| {
                    serde_json::to_value(v).map_err(anyhow::Error::from)
                })),
                deserializer: Some(Box::new(|v| {
                    serde_json::from_value(v.clone()).map_err(anyhow::Error::from)
                })),
                compress: true,
                offload: true,
                _data_marker: std::default::Default::default(),
            },
        }
    }
}

fn some_functionality_with_internal_randomness<TV: TestVector>() -> u64 {
    let test_fields = TV::initialize_values::<SomeTestFields>();

    let mut rng = rand::rng();
    let a: u64 = rng.random();
    let a: u64 = TV::expose_value(&test_fields.a, a);
    a
}

fn some_other_functionality<TV: TestVector>(a: u64) {
    let test_fields = TV::initialize_values::<SomeTestFields>();
    let y: bool = a % 2 == 0;
    let b = if y {
        "abc".to_string()
    } else {
        "def".to_string()
    };
    TV::check_value(&test_fields.b, &b);
}

#[test]
fn test_manual_set() {
    let _guard = TestVectorActive::initialize_test_vector(
        "manual_tv.toml",
        TestVectorFileFormat::Toml,
        TestMode::Init,
    );
    // The global (non-`tls`) backend serializes sessions via a non-reentrant
    // lock, so a second init on the same thread would deadlock rather than
    // return an error. Only the `tls` backend rejects double-init outright.
    #[cfg(feature = "tls")]
    {
        let should_be_err = initialize_tv_case_from_file(
            "manual_tv.toml",
            TestVectorFileFormat::Toml,
            TestMode::Init,
        );
        if should_be_err.is_ok() {
            panic!("Should not be able to initialize manual_tv.toml again");
        }
    }

    let a = some_functionality_with_internal_randomness::<TestVectorActive>();
    some_other_functionality::<TestVectorActive>(a);

    assert_tv::finalize_tv_case().expect("Error finalizing test vector case");
    drop(_guard);

    let tv_file_path = Path::new("manual_tv.toml");
    let mut tv_file = std::fs::File::open(tv_file_path)
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to open test vector file ({:?}): {}",
                tv_file_path,
                e
            )
        })
        .unwrap();
    let mut tv_file_content = String::new();
    tv_file.read_to_string(&mut tv_file_content).unwrap();

    let test_vector: serde_json::value::Value = toml::from_str(tv_file_content.as_str()).unwrap();
    println!("{test_vector:?}");
    let a_entry: Map<String, Value> = test_vector
        .as_object()
        .unwrap()
        .get("entries")
        .unwrap()
        .as_array()
        .unwrap()[0]
        .as_object()
        .unwrap()
        .clone();
    assert_eq!(
        a_entry.get("name").unwrap().as_str().unwrap(),
        "a".to_string()
    );
    let a_value: &str = a_entry.get("value").unwrap().as_str().unwrap();
    let captured_a_value: u64 = u64::from_str(a_value).unwrap();
    assert_eq!(a, captured_a_value);

    // test in check mode
    let _guard = TestVectorActive::initialize_test_vector(
        "manual_tv.toml",
        TestVectorFileFormat::Toml,
        TestMode::Check,
    );
    let a = some_functionality_with_internal_randomness::<TestVectorActive>();
    assert_eq!(a, captured_a_value);
    some_other_functionality::<TestVectorActive>(a);
    drop(_guard);

    // test no impl
    let a = some_functionality_with_internal_randomness::<TestVectorNOP>();
    assert_ne!(a, captured_a_value); // a should (with extreme likelihood) not be the same value that was exposed
    some_other_functionality::<TestVectorNOP>(a);

    // delete manual_tv.toml
    std::fs::remove_file(tv_file_path).unwrap();
    std::fs::remove_file(Path::new("manual_tv.toml_offloaded_value_1.zstd")).unwrap()
}
