use crate::{DynDeserializer, DynSerializer, TestMode, TestVectorFileFormat, TlsEnvGuard};
use anyhow::{Context, anyhow, bail};
use log::warn;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
pub struct TestVectorEntry {
    entry_type: TestVectorEntryType,
    description: Option<String>,
    name: Option<String>,
    #[serde(default = "default_null", skip_serializing_if = "is_null")]
    value: serde_json::Value,
    code_location: Option<String>,
    test_vec_set_code_location: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "is_false")]
    offload: bool,
}

#[derive(Serialize, Deserialize, Debug, Copy, Eq, PartialEq, Clone)]
/// Kind of entry stored in a test vector file.
pub enum TestVectorEntryType {
    /// Constant input captured and, in `Check` mode, injected back.
    Const,
    /// Output that is validated against the stored value in `Check` mode.
    Output,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
pub struct TestVectorData {
    pub entries: Vec<TestVectorEntry>,
}

/// Internal environment holding the currently loaded and recorded test vectors.
///
/// Exposed to allow advanced/manual control; typical tests rely on the
/// `#[test_vec_case]` macro or the high‑level `initialize_tv_case_from_file`/`finalize_tv_case` pair.
pub struct TestVecEnv {
    pub(crate) tv_file_path: PathBuf,
    file_format: TestVectorFileFormat,
    loaded_tv_data: TestVectorData,
    recorded_tv_data: TestVectorData,
    test_mode: TestMode,
}

impl TestVectorData {
    fn load_from_file<T: Into<PathBuf>>(
        tv_file_path: T,
        file_format: TestVectorFileFormat,
    ) -> anyhow::Result<Self> {
        let tv_file_path = tv_file_path.into();

        let mut tv_file = std::fs::File::open(tv_file_path.clone()).map_err(|e| {
            anyhow::anyhow!(
                "Failed to open test vector file ({:?}): {}",
                tv_file_path,
                e
            )
        })?;
        let mut tv_data: TestVectorData = match file_format {
            TestVectorFileFormat::Json => serde_json::from_reader(tv_file).map_err(|e| {
                anyhow::anyhow!(
                    "Failed to parse test vector file ({:?}) as json: {}",
                    tv_file_path,
                    e
                )
            })?,
            TestVectorFileFormat::Yaml => serde_yaml::from_reader(tv_file).map_err(|e| {
                anyhow::anyhow!(
                    "Failed to parse test vector file ({:?}) as yaml: {}",
                    tv_file_path,
                    e
                )
            })?,
            TestVectorFileFormat::Toml => {
                let mut buffer: String = String::new();
                tv_file
                    .read_to_string(&mut buffer)
                    .map_err(|e| anyhow::anyhow!("Failed to read test vector file: {}", e))?;
                toml::from_str(buffer.as_ref()).map_err(|e| {
                    anyhow::anyhow!(
                        "Failed to parse test vector file ({:?}) as toml: {}",
                        tv_file_path,
                        e
                    )
                })?
            }
        };
        tv_data.load_offloaded_values(tv_file_path.clone())?;
        Ok(tv_data)
    }

    fn load_offloaded_values(&mut self, tv_file_path: PathBuf) -> anyhow::Result<()> {
        for (entry_index, entry) in self.entries.iter_mut().enumerate() {
            if !(entry.offload) {
                continue;
            }
            if !entry.value.is_null() {
                warn!("Test value entry is set to offload but still has a value already loaded")
            }
            let offloaded_path = append_suffix_to_filename(
                &tv_file_path,
                format!("_offloaded_value_{}.zstd", entry_index).as_str(),
            );
            let mut offloaded_value_file =
                std::fs::File::open(offloaded_path.clone()).map_err(|e| {
                    anyhow::anyhow!(
                        "Failed to open offloaded value file ({:?}): {}",
                        offloaded_path,
                        e
                    )
                })?;
            let mut offloaded_value_bytes = Vec::new();
            offloaded_value_file
                .read_to_end(&mut offloaded_value_bytes)
                .map_err(|e| anyhow::anyhow!("Failed to read offloaded value file: {}", e))?;
            drop(offloaded_value_file);

            #[cfg(feature = "zstd-offload")]
            let offloaded_value_bytes = decompress(offloaded_value_bytes)?;

            let offloaded_value: serde_json::value::Value =
                serde_json::from_slice(&offloaded_value_bytes).map_err(|e| {
                    anyhow::anyhow!("Failed to parse offloaded value as a json value: {}", e)
                })?;
            entry.value = offloaded_value;
        }
        Ok(())
    }

    fn save_offloaded_values(&mut self, tv_file_path: PathBuf) -> anyhow::Result<()> {
        for (entry_index, entry) in self.entries.iter_mut().enumerate() {
            if !entry.offload {
                continue;
            }

            let offloaded_path = append_suffix_to_filename(
                &tv_file_path,
                format!("_offloaded_value_{}.zstd", entry_index).as_str(),
            );

            let serialized = serde_json::to_vec(&entry.value).map_err(|e| {
                anyhow::anyhow!("Failed to serialize value at index {}: {}", entry_index, e)
            })?;

            #[cfg(feature = "zstd-offload")]
            let serialized = compress(serialized)?;

            let mut file = std::fs::File::create(&offloaded_path).map_err(|e| {
                anyhow::anyhow!(
                    "Failed to create or overwrite offloaded value file at {:?}: {}",
                    offloaded_path,
                    e
                )
            })?;

            file.write_all(&serialized).map_err(|e| {
                anyhow::anyhow!(
                    "Failed to write to offloaded value file at {:?}: {}",
                    offloaded_path,
                    e
                )
            })?;

            entry.value = serde_json::Value::Null;
        }
        Ok(())
    }

    fn store_to_file<T: Into<PathBuf>>(
        &mut self,
        tv_file_path: T,
        file_format: TestVectorFileFormat,
    ) -> anyhow::Result<()> {
        let tv_file_path = tv_file_path.into();
        self.save_offloaded_values(tv_file_path.clone())?;
        if let Some(parent) = tv_file_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                anyhow::anyhow!(
                    "Failed to create parent directories for test vector file ({:?}): {}",
                    tv_file_path,
                    e
                )
            })?;
        }
        let mut tv_file = std::fs::File::create(tv_file_path)
            .map_err(|e| anyhow::anyhow!("Failed to create test vector file: {}", e))?;
        match file_format {
            TestVectorFileFormat::Json => serde_json::to_writer_pretty(tv_file, &self)
                .map_err(|e| anyhow::anyhow!("Failed to write test vector file as json: {}", e))?,
            TestVectorFileFormat::Yaml => serde_yaml::to_writer(tv_file, &self)
                .map_err(|e| anyhow::anyhow!("Failed to write test vector file as yaml: {}", e))?,
            TestVectorFileFormat::Toml => {
                let tv_serialized: String = toml::to_string(&self).map_err(|e| {
                    anyhow::anyhow!("Failed to serialize test vector as toml: {}", e)
                })?;
                tv_file
                    .write(tv_serialized.as_bytes())
                    .map_err(|e| anyhow::anyhow!("Failed to write test vector: {}", e))?;
            }
        };
        Ok(())
    }
}

/// Create a test‑vector session from the given file and mode.
///
/// - In `Init`, starts with an empty in‑memory vector and writes it on finalize
///   if missing or changed.
/// - In `Check`, loads and uses the existing file for validation.
///
/// Returns a guard that must be kept alive for the duration of the session; dropping it
/// clears the global/thread‑local environment.
pub fn initialize_tv_case_from_file<T: Into<PathBuf>>(
    tv_file_path: T,
    file_format: TestVectorFileFormat,
    test_mode: TestMode,
) -> anyhow::Result<TlsEnvGuard> {
    let tv_file_path: PathBuf = tv_file_path.into();
    let loaded_tv_data = match test_mode {
        TestMode::Init => TestVectorData {
            entries: Vec::new(),
        },
        TestMode::Check => {
            TestVectorData::load_from_file(&tv_file_path, file_format).map_err(|e| {
                anyhow!("Error loading test vector. You may need to switch to init mode. Internal error: {}", e)
            })?
        }
    };
    let tv_env = TestVecEnv {
        tv_file_path,
        loaded_tv_data,
        recorded_tv_data: TestVectorData {
            entries: Vec::new(),
        },
        file_format,
        test_mode,
    };
    TestVecEnv::initialize_with(tv_env)
}

/// Finalize the current test‑vector session.
///
/// In `Init` mode, writes the recorded entries to disk (overwriting the file)
/// when content changed or the file does not exist. In `Check` mode, this is a no‑op.
pub fn finalize_tv_case() -> anyhow::Result<()> {
    TestVecEnv::with_global(|tv_env| {
        match tv_env.test_mode {
            TestMode::Check => {
                // In check mode, test vectors are not updated
            }
            TestMode::Init => {
                // In both init mode, the test vector file is update if necessary
                let update_required = tv_env.loaded_tv_data != tv_env.recorded_tv_data ||  // Test vectors have changed
                        !tv_env.tv_file_path.is_file(); // OR test vector file does not exist
                if update_required {
                    tv_env
                        .recorded_tv_data
                        .store_to_file(&tv_env.tv_file_path, tv_env.file_format)?;
                }
            }
        }
        Ok(())
    })
}

/// Low‑level: process the next observed entry.
///
/// This is used internally by `TestVector::{expose_value, expose_mut_value, check_value}`.
pub fn process_next_entry<O>(
    entry_type: TestVectorEntryType,
    description: Option<String>,
    name: Option<String>,
    observed_value: &O,
    code_location: Option<String>,
    test_vec_set_code_location: Option<String>,
    serializer: &DynSerializer<O>,
    deserializer: Option<&DynDeserializer<O>>,
    offload: bool,
) -> anyhow::Result<Option<O>> {
    let value = serializer(observed_value)?;
    let observed_entry = TestVectorEntry {
        entry_type,
        description,
        name,
        value,
        code_location,
        test_vec_set_code_location,
        offload,
    };

    TestVecEnv::with_global(|tv_env| {
        let entry_index = tv_env.recorded_tv_data.entries.len();
        let loaded_entry = tv_env.loaded_tv_data.entries.get(entry_index).cloned();
        tv_env.recorded_tv_data.entries.push(observed_entry.clone());
        match tv_env.test_mode {
            TestMode::Init => {
                // init mode ignores (doesn't check) all entries (passes it through to be stored)
                // Entry types of type const are however deserialized and returned anyway
                // This is done to have exact same behaviour as check mode, where consts are loaded and replaced
                match observed_entry.entry_type {
                    TestVectorEntryType::Const => Ok(Some(
                        deserializer.expect("Deserializer was required but not provided")(
                            &observed_entry.value,
                        )
                        .with_context(|| {
                            "Failed to deserialize constant value right after serializing it. \
                        There probably is a bug in the TestVectorMomento implementation"
                        })?,
                    )),
                    TestVectorEntryType::Output => {
                        // Nothing will be outputted if the entry type is output (as there is nothing to be replaced
                        Ok(None)
                    }
                }
            }
            TestMode::Check => {
                let Some(loaded_entry) = loaded_entry else {
                    bail!(
                        "Observed value does not exist in loaded test vector: \n observed: {:?}",
                        observed_entry
                    )
                };
                let diff = || {
                    format!(
                        "\n\
                                     loaded name: {:?}\n\
                                   observed name: {:?}\n\
                                    loaded value: {:?}\n\
                                  observed value: {:?}\n\
                                    loaded entry_type: {:?}\n\
                                  observed entry_type: {:?}\n",
                        loaded_entry.name,
                        observed_entry.name,
                        loaded_entry.value,
                        observed_entry.value,
                        loaded_entry.entry_type,
                        observed_entry.entry_type
                    )
                };
                // check entry types
                match observed_entry.entry_type {
                    TestVectorEntryType::Const | TestVectorEntryType::Output => {
                        if loaded_entry.name != observed_entry.name {
                            bail!(
                                "Observed value does not match the loaded test vectors name:{}",
                                diff()
                            )
                        }
                        if loaded_entry.entry_type != observed_entry.entry_type {
                            bail!(
                                "Observed value does not match the loaded test vectors type:{}",
                                diff()
                            )
                        }
                    }
                }

                // check the value if it is output
                match loaded_entry.entry_type {
                    TestVectorEntryType::Const => {}
                    TestVectorEntryType::Output => {
                        if loaded_entry.value != observed_entry.value {
                            bail!(
                                "Observed value does not match the loaded test vectors value:{}",
                                diff()
                            )
                        }
                    }
                };

                // Deserialize const values
                match loaded_entry.entry_type {
                    TestVectorEntryType::Const => deserializer
                        .expect("Deserializer was required but not provided")(
                        &loaded_entry.value
                    )
                    .map(|v| Some(v)),
                    TestVectorEntryType::Output => Ok(None),
                }
            }
        }
    })
}

fn is_false(v: &bool) -> bool {
    !*v
}

fn append_suffix_to_filename(path: &PathBuf, suffix: &str) -> PathBuf {
    let mut path = path.clone();
    if let Some(file_name) = path.file_name().map(|f| f.to_string_lossy()) {
        let new_file_name = format!("{}{}", file_name, suffix);
        path.set_file_name(new_file_name);
    }
    path
}

#[cfg(feature = "zstd-offload")]
fn decompress(data: Vec<u8>) -> anyhow::Result<Vec<u8>> {
    let cursor = std::io::Cursor::new(data);
    let decompressed = zstd::decode_all(cursor)?;
    Ok(decompressed)
}

#[cfg(feature = "zstd-offload")]
fn compress(data: Vec<u8>) -> anyhow::Result<Vec<u8>> {
    let cursor = std::io::Cursor::new(data);
    let compressed = zstd::encode_all(cursor, 15)?;
    Ok(compressed)
}

fn is_null(value: &serde_json::Value) -> bool {
    matches!(value, serde_json::Value::Null)
}

fn default_null() -> serde_json::Value {
    serde_json::Value::Null
}
