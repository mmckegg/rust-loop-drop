extern crate serde_json;

use serde_json::Value;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=config/controller.ytx");

    let source = fs::read_to_string("config/controller.ytx").expect("read config/controller.ytx");
    let json: Value = serde_json::from_str(&source).expect("parse config/controller.ytx as json");
    let encoders = json["banks"]["0"]["encoders"]
        .as_object()
        .expect("banks.0.encoders object");

    let mut modes = Vec::new();
    for index in 0..28 {
        let mode = encoders[&index.to_string()]["rotation_feedback"]["mode"]
            .as_u64()
            .unwrap_or(2);
        modes.push(mode);
    }

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let out_file = out_dir.join("controller_modes.rs");
    let body = format!(
        "pub const PHYSICAL_ENCODER_RING_MODES: [u8; 28] = [{}];\n",
        modes.iter()
            .map(|mode| mode.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
    fs::write(out_file, body).expect("write generated controller modes");
}
