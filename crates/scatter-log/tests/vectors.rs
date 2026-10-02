//! Checks the crate against `docs/api/vectors/log-v1.json`, whose derived values an
//! independent Python implementation (`check.py` beside it) produces.

use scatter_log::body::{Body, Part};
use scatter_log::cbor::{self, Value};
use scatter_log::hash::{self, hex};
use scatter_log::header::Header;
use scatter_log::record::Record;

fn vectors() -> serde_json::Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/api/vectors/log-v1.json"
    );
    serde_json::from_str(&std::fs::read_to_string(path).expect("vectors file")).expect("json")
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

fn s<'a>(v: &'a serde_json::Value, k: &str) -> &'a str {
    v[k].as_str().expect(k)
}

#[test]
fn cbor_vectors() {
    let doc = vectors();
    assert_eq!(doc["format"], "scatter-log/1");
    for case in doc["cbor"].as_array().expect("cbor") {
        let name = s(case, "name");
        let value = Value::from_json(&case["json"]);
        let expected = unhex(s(&case["expect"], "hex"));
        let encoded = cbor::encode(&value).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(hex(&encoded), hex(&expected), "{name}: encoding");
        let decoded = cbor::decode(&expected).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(decoded, value, "{name}: decoding");
        assert_eq!(
            decoded.to_json().expect("json"),
            case["json"],
            "{name}: back to JSON"
        );
    }
}

fn header_of(h: &serde_json::Value) -> Header {
    let opt = |k: &str| h[k].as_u64();
    Header {
        version: h["version"].as_u64().expect("version"),
        partition: h["partition"].as_u64().expect("partition"),
        offset: h["offset"].as_u64().expect("offset"),
        appended_at: h["appended_at"].as_u64().expect("appended_at"),
        payload_type: s(h, "payload_type").to_owned(),
        key: h["key"].as_str().map(str::to_owned),
        commitment: [0; 32],
        revid: opt("revid"),
        logid: opt("logid"),
        page_id: opt("page_id"),
    }
}

#[test]
fn record_vectors() {
    let doc = vectors();
    for case in doc["records"].as_array().expect("records") {
        let name = s(case, "name");
        let expect = &case["expect"];
        let parts: Vec<Part> = case["parts"]
            .as_array()
            .expect("parts")
            .iter()
            .map(|p| Part::Present {
                salt: unhex(s(p, "salt")).try_into().expect("16-byte salt"),
                bytes: cbor::encode(&Value::from_json(&p["json"])).expect("part encodes"),
            })
            .collect();
        for (i, p) in parts.iter().enumerate() {
            assert_eq!(
                hex(p.bytes().expect("present")),
                expect["part_cbor"][i].as_str().expect("hex"),
                "{name}: part {i} bytes"
            );
            assert_eq!(
                hex(&p.leaf(u8::try_from(i).expect("u8"))),
                expect["part_leaves"][i].as_str().expect("hex"),
                "{name}: part {i} leaf"
            );
        }
        let body = Body::new(parts).expect("body");
        assert_eq!(
            hex(&body.commitment()),
            s(expect, "commitment"),
            "{name}: commitment"
        );
        assert_eq!(
            hex(&body.content_hash().expect("content")),
            s(expect, "content_hash"),
            "{name}: content hash"
        );
        let preimage = hash::signature_preimage(
            body.content().bytes().expect("content"),
            body.comment().bytes().expect("comment"),
        );
        assert_eq!(
            hex(&preimage),
            s(expect, "signature_preimage"),
            "{name}: signature preimage"
        );

        let mut record = Record::seal(header_of(&case["header"]), body);
        assert_eq!(
            hex(&record.header().encode()),
            s(expect, "header_cbor"),
            "{name}: header"
        );
        assert_eq!(hex(&record.leaf()), s(expect, "leaf"), "{name}: leaf");
        assert_eq!(
            hex(&record.encode()),
            s(expect, "record_cbor"),
            "{name}: record"
        );
        let back = Record::decode(&unhex(s(expect, "record_cbor"))).expect("decodes");
        assert_eq!(back, record, "{name}: round trip");

        for i in case["erase"].as_array().expect("erase") {
            let i = usize::try_from(i.as_u64().expect("index")).expect("usize");
            record.body_mut().erase(i).expect("erasable");
        }
        assert_eq!(
            hex(&record.encode()),
            s(expect, "record_cbor_erased"),
            "{name}: erased record"
        );
        let back = Record::decode(&unhex(s(expect, "record_cbor_erased"))).expect("decodes");
        assert_eq!(
            back.leaf(),
            record.leaf(),
            "{name}: the leaf survives erasure"
        );
        assert_eq!(back.header().commitment, record.body().commitment());
    }
}
