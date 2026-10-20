use serde_json::json;

use super::*;

#[test]
fn a_digest_is_the_sha256_of_the_bytes_and_round_trips_through_its_spelling() {
    let digest = Digest::of(b"abc");
    assert_eq!(
        digest.to_string(),
        "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(digest.to_string().parse::<Digest>(), Ok(digest));
    for malformed in [
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        "sha256:ba78",
        "sha256:zz7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        "sha1:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    ] {
        assert!(malformed.parse::<Digest>().is_err(), "{malformed}");
    }
}

#[test]
fn an_attachment_reads_from_a_record_with_its_four_fields_and_refuses_a_missing_or_extra_one() {
    let digest = Digest::of(b"x");
    let value = json!({
        "$type": ATTACHMENT_TYPE,
        "digest": digest.to_string(),
        "size": 1,
        "media_type": "image/png",
        "location": "/store/x",
    });
    let attachment = Attachment::from_value(&value).unwrap();
    assert_eq!(
        attachment,
        Attachment::new(digest, 1, "image/png".to_owned(), "/store/x".to_owned())
    );
    assert_eq!(serde_json::to_value(&attachment).unwrap(), value);
    let mut missing = value.clone();
    missing.as_object_mut().unwrap().remove("size");
    assert!(Attachment::from_value(&missing).is_err());
    let mut extra = value;
    extra["etag"] = json!("x");
    assert!(Attachment::from_value(&extra).is_err());
}

#[test]
fn a_media_type_follows_the_extension_and_an_unknown_one_is_an_octet_stream() {
    for (name, expected) in [
        ("photo.png", "image/png"),
        ("photo.jpg", "image/jpeg"),
        ("photo.jpeg", "image/jpeg"),
        ("a.gif", "image/gif"),
        ("a.webp", "image/webp"),
        ("a.svg", "image/svg+xml"),
        ("a.pdf", "application/pdf"),
        ("a.zip", "application/zip"),
        ("a.tar.gz", "application/gzip"),
        ("a.json", "application/json"),
        ("a.md", "text/markdown"),
        ("a.txt", "text/plain"),
        ("a.csv", "text/csv"),
        ("a.html", "text/html"),
        ("a.mp4", "video/mp4"),
        ("a.mp3", "audio/mpeg"),
        ("photo.PNG", "application/octet-stream"),
        ("README", "application/octet-stream"),
    ] {
        assert_eq!(media_type(name), expected, "{name}");
    }
}
