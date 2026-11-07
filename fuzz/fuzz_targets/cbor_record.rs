//! The strict CBOR decoder and the record readers over arbitrary bytes (22 §4.9; 0006
//! §2–3, §5; 0033 §15): any bytes must be decoded or refused without a panic, as a
//! header, a body, a record and a bare value; what decodes must re-encode and convert
//! to JSON (or refuse to) without a panic. Nothing else is asserted: the canonical
//! round trip is `crates/scatter-log/tests/properties.rs`.

#![no_main]

use libfuzzer_sys::fuzz_target;
use scatter_log::body::Body;
use scatter_log::cbor;
use scatter_log::header::Header;
use scatter_log::record::Record;

fuzz_target!(|data: &[u8]| {
    if let Ok(value) = cbor::decode(data) {
        let _ = cbor::encode(&value);
        let _ = value.to_json();
    }
    let _ = cbor::decode_prefix(data);
    let _ = Header::decode(data);
    let _ = Body::decode(data);
    let _ = Record::decode(data);
});
