use arrow::array::RecordBatch;
use arrow::json::reader::{infer_json_schema_from_iterator, ReaderBuilder};
use serde_json::Value;

use serde::Deserialize;
use std::sync::Arc;

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

/// Ingest JSON array using the Apache Arrow reference implementation.
pub fn parse_with_arrow(data: &str) -> RecordBatch {
    let records: Vec<Value> = serde_json::from_str(data).expect("valid JSON array");
    let schema =
        infer_json_schema_from_iterator(records.iter().map(Ok)).expect("infer Arrow schema");
    let mut decoder = ReaderBuilder::new(Arc::new(schema))
        .build_decoder()
        .expect("build Arrow JSON decoder");
    decoder
        .serialize(records.as_slice())
        .expect("serialize records into Arrow columns");
    decoder
        .flush()
        .expect("flush buffered records")
        .expect("non-empty record batch")
}

/// Generate `n` JSON records as an array string.
pub fn generate_json(n: usize) -> String {
    let rows = (0..n)
        .map(|i| format!("{{\"id\":{},\"value\":\"v{}\"}}", i, i))
        .collect::<Vec<_>>();
    format!("[{}]", rows.join(","))
}
