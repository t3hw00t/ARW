use arrow2::{
    array::Array,
    chunk::Chunk,
    datatypes::Schema,
    io::json::read::{
        deserialize_records, infer_records_schema,
        json_deserializer::{parse, Value},
    },
};
use serde::Deserialize;

/// Simple record used for benchmarking ingestion paths.
#[derive(Deserialize)]
pub struct Record {
    pub id: u64,
    pub value: String,
}

/// Ingest JSON array using serde.
pub fn parse_with_serde(data: &str) -> Vec<Record> {
    serde_json::from_str(data).unwrap()
}

/// Ingest JSON array using arrow2.
pub fn parse_with_arrow(data: &str) -> Chunk<Box<dyn Array>> {
    let value: Value = parse(data.as_bytes()).unwrap();
    let schema: Schema = infer_records_schema(&value).unwrap();
    deserialize_records(&value, &schema).unwrap()
}

/// Generate `n` JSON records as an array string.
pub fn generate_json(n: usize) -> String {
    let rows = (0..n)
        .map(|i| format!("{{\"id\":{},\"value\":\"v{}\"}}", i, i))
        .collect::<Vec<_>>();
    format!("[{}]", rows.join(","))
}
