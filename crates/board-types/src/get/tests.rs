//! A get report's file reads as Board prints it.

use serde_json::{Value, json};

use super::*;

/// The files of `board get /projects/p /raw.bin /docs --json`.
fn files() -> Value {
    let location = |display: &str, path: &str| json!({"display": display, "path": path, "pointers": [], "repository": "/r", "source": "worktree"});
    json!([
        {"location": location("/projects/p", "projects/p.json"), "blob": "350ab0b8b289baeaf217fc2914e660f59ee25faf", "text": "{\"$type\": \"lr/project/v1\"}", "record": {"$type": "lr/project/v1"}},
        {"location": location("/raw.bin", "raw.bin"), "blob": "0a853c868c31f8e9991aa72662ad9eab6eabbc6b", "base64": "/wBh", "code": "board::record::not_a_record"},
        {"location": location("/docs", "docs"), "blob": null, "text": "", "code": "board::record::not_a_record"}
    ])
}

#[test]
fn each_file_reads_with_its_record_and_writes_back_the_same_json() {
    let read: Vec<File<Value>> = serde_json::from_value(files()).unwrap();
    assert_eq!(read[0].record, Some(json!({"$type": "lr/project/v1"})));
    assert_eq!(read[1].content, Content::Base64("/wBh".to_owned()));
    assert_eq!(read[2].blob, None);
    assert_eq!(serde_json::to_value(&read).unwrap(), files());
}

#[test]
fn a_file_without_text_or_base64_is_refused_and_its_twin_with_text_reads() {
    let mut refused = files()[2].clone();
    refused.as_object_mut().unwrap().remove("text");
    assert!(serde_json::from_value::<File>(refused).is_err());
    assert!(serde_json::from_value::<File>(files()[2].clone()).is_ok());
}
