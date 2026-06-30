use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use npnp::footprint::build_pcblib_from_payload;
use npnp::pcblib::write_pcblib;
use npnp::schlib::{
    SchlibMetadata, SchlibParameter, build_component_from_payload_with_metadata,
    write_schlib_library,
};
use serde_json::Value;

const STEP_FIXTURE: &[u8] = b"ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\nENDSEC;\nEND-ISO-10303-21;\n";

#[test]
fn fixture_payloads_write_openable_altium_stream_layouts() {
    let output_dir = temp_output_dir("fixture_payloads_write_openable_altium_stream_layouts");
    fs::create_dir_all(&output_dir).expect("create temp output dir");

    let symbol_payload: Value =
        serde_json::from_str(include_str!("fixtures/easyeda_symbol.json")).expect("symbol fixture");
    let footprint_payload: Value =
        serde_json::from_str(include_str!("fixtures/easyeda_footprint.json"))
            .expect("footprint fixture");

    let metadata = SchlibMetadata {
        description: Some("Regression component".to_string()),
        designator: Some("U?".to_string()),
        comment: Some("REGRESSION".to_string()),
        parameters: vec![SchlibParameter {
            name: "Footprint".to_string(),
            value: "REGRESSION_QFN".to_string(),
        }],
        footprint_model_name: Some("REGRESSION_QFN".to_string()),
        footprint_library_file: Some("Regression.PcbLib".to_string()),
    };

    let component =
        build_component_from_payload_with_metadata(&symbol_payload, "REGRESSION_QFN", &metadata)
            .expect("build schematic component from fixture");
    let schlib_path = output_dir.join("Regression.SchLib");
    write_schlib_library(&[component], &schlib_path).expect("write SchLib fixture output");
    assert_cfb_stream_exists(&schlib_path, "/FileHeader");
    assert_cfb_stream_exists(&schlib_path, "/Storage");
    let schlib_data = read_cfb_stream_text(&schlib_path, "/REGRESSION_QFN/Data");
    assert!(schlib_data.contains("|LIBREFERENCE=REGRESSION_QFN|"));
    assert!(schlib_data.contains("|MODELTYPE=PCBLIB|"));
    assert!(schlib_data.contains("|MODELDATAFILEENTITY1=Regression.PcbLib|"));

    let pcblib =
        build_pcblib_from_payload(&footprint_payload, "REGRESSION_QFN", Some(STEP_FIXTURE))
            .expect("build PCB library from fixture");
    let pcb_component = pcblib
        .components
        .first()
        .expect("PCB fixture should produce a component");
    assert_eq!(pcb_component.pads.len(), 2);
    assert_eq!(pcb_component.tracks.len(), 1);
    assert_eq!(pcb_component.arcs.len(), 1);
    assert_eq!(pcb_component.regions.len(), 3);
    assert_eq!(pcb_component.bodies.len(), 1);
    assert_eq!(pcblib.models.len(), 1);

    let pcblib_path = output_dir.join("Regression.PcbLib");
    write_pcblib(&pcblib, &pcblib_path).expect("write PcbLib fixture output");
    assert_cfb_stream_exists(&pcblib_path, "/FileHeader");
    assert_cfb_stream_exists(&pcblib_path, "/Library/Data");
    assert_cfb_stream_exists(&pcblib_path, "/REGRESSION_QFN/Data");
    assert_cfb_stream_exists(&pcblib_path, "/Library/Models/0");

    fs::remove_dir_all(output_dir).ok();
}

fn temp_output_dir(test_name: &str) -> std::path::PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("npnp_{test_name}_{timestamp}"))
}

fn assert_cfb_stream_exists(path: &Path, stream_path: &str) {
    let file = File::open(path).expect("open compound file");
    let mut compound = cfb::CompoundFile::open(file).expect("open compound file structure");
    compound
        .open_stream(stream_path)
        .unwrap_or_else(|err| panic!("expected stream {stream_path}: {err}"));
}

fn read_cfb_stream_text(path: &Path, stream_path: &str) -> String {
    let file = File::open(path).expect("open compound file");
    let mut compound = cfb::CompoundFile::open(file).expect("open compound file structure");
    let mut stream = compound.open_stream(stream_path).expect("open stream");
    let mut data = Vec::new();
    stream.read_to_end(&mut data).expect("read stream");
    String::from_utf8_lossy(&data).into_owned()
}
