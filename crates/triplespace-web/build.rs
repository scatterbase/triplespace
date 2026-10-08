//! Records this binary's build and its part of the component manifest for
//! Special:Version (ADR 0077 §4, §15): `$OUT_DIR/version.json`.

include!("../../xtask/src/build_version.rs");

fn main() {
    write_version_json("triplespace-web");
}
