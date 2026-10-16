use serde_json::Value;
use sha1::{Digest as Sha1Digest, Sha1};
use sha2::{Digest as Sha2Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use wisp_bio::docking::prep::pdbqt::{parse_pdbqt, parse_receptor_pdbqt, write_pdbqt};
use wisp_bio::docking::{
    prepare_ligand_pdbqt, prepare_receptor_pdbqt, DockingEngine, VinaEngine, VINARDO,
};

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("scripts/fixtures/docking")
}

fn fixture(relative: &str) -> String {
    fs::read_to_string(fixture_root().join(relative)).unwrap()
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn git_blob_oid(bytes: &[u8]) -> String {
    let mut digest = Sha1::new();
    digest.update(format!("blob {}\0", bytes.len()).as_bytes());
    digest.update(bytes);
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[test]
fn manifest_pins_oracle_provenance_and_every_fixture_checksum() {
    let root = fixture_root();
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(root.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["schema_version"], 1);
    assert_eq!(manifest["oracle"]["name"], "AutoDock Vina");
    assert_eq!(manifest["oracle"]["version"], "1.2.7");
    assert_eq!(
        manifest["oracle"]["git_commit"],
        "8eb40404f4f45608acb3b01427587ac049f27c1f"
    );
    assert_eq!(manifest["oracle"]["license"], "Apache-2.0");
    assert!(manifest["oracle"]["binary_policy"]
        .as_str()
        .unwrap()
        .contains("does not distribute or download"));

    let tolerances = manifest["tolerances"].as_object().unwrap();
    assert_eq!(tolerances.len(), 2);
    assert!(tolerances.contains_key("vina"));
    assert!(tolerances.contains_key("vinardo"));

    for entry in manifest["fixtures"].as_array().unwrap() {
        let relative = entry["path"].as_str().unwrap();
        assert!(!relative.contains('\\'));
        assert!(!Path::new(relative).is_absolute());
        assert!(!Path::new(relative)
            .components()
            .any(|part| part.as_os_str() == ".."));
        let bytes = fs::read(root.join(relative)).unwrap();
        assert_eq!(
            bytes.len() as u64,
            entry["bytes"].as_u64().unwrap(),
            "{relative}"
        );
        assert_eq!(
            sha256(&bytes),
            entry["sha256"].as_str().unwrap(),
            "{relative}"
        );
        assert!(!entry["license"].as_str().unwrap().is_empty());
        assert!(!entry["redistribution"].as_str().unwrap().is_empty());
        if entry["origin"] == "official_upstream" && entry["source"]["git_commit"].is_string() {
            let expected_blob = entry["source"]["git_blob"]
                .as_str()
                .expect("official Git fixture must record git_blob");
            assert_eq!(git_blob_oid(&bytes), expected_blob, "{relative}");
        }
    }

    for case in manifest["scoring_cases"].as_array().unwrap() {
        match case["oracle_status"].as_str().unwrap() {
            "not_generated" => assert!(case["expected"].is_null()),
            "generated" => assert!(case["expected"].is_object()),
            other => panic!("unexpected oracle status {other}"),
        }
    }
}

#[test]
fn official_vina_ligand_round_trip_preserves_wire_identity_and_topology() {
    let input = fixture("official/autodock-vina-v1.2.7/1iep_ligand.pdbqt");
    let parsed = parse_pdbqt(&input).unwrap();
    assert_eq!(parsed.atoms.len(), 40);
    assert_eq!(parsed.branches.len(), 7);
    assert_eq!(parsed.torsdof, 7);
    assert_eq!(
        parsed
            .atoms
            .iter()
            .filter(|atom| atom.ad_type.pdbqt_symbol() == "HD")
            .count(),
        3
    );

    let written = write_pdbqt(&parsed).unwrap();
    let reparsed = parse_pdbqt(&written).unwrap();
    assert_eq!(reparsed, parsed);

    let prepared = prepare_ligand_pdbqt(&input).unwrap();
    assert_eq!(prepared.source_atoms().len(), 40);
    assert_eq!(prepared.atoms().len(), 37);
    assert_eq!(prepared.branches().len(), 7);
    assert_eq!(prepared.num_tors(), 7.0);
    assert_eq!(prepared.torsdof(), 7);
    assert_eq!(
        prepared
            .source_to_scored()
            .iter()
            .filter(|slot| slot.is_none())
            .count(),
        3
    );
}

#[test]
fn official_vina_rigid_receptor_uses_strict_pdbqt_types() {
    let input = fixture("official/autodock-vina-v1.2.7/1iep_receptor.pdbqt");
    let parsed = parse_receptor_pdbqt(&input).unwrap();
    assert_eq!(parsed.atoms.len(), 2702);
    assert_eq!(
        parsed
            .atoms
            .iter()
            .filter(|atom| atom.ad_type.pdbqt_symbol() == "HD")
            .count(),
        473
    );

    let prepared = prepare_receptor_pdbqt(&input).unwrap();
    assert_eq!(prepared.atoms().len(), 2229);
    assert!(prepared.atoms().iter().all(|atom| atom.xs_type.is_some()));
    assert!(prepared
        .atoms()
        .iter()
        .all(|atom| atom.element != wisp_bio::docking::prep::atom_type::Element::H));
}

#[test]
fn project_parser_fixtures_lock_order_hydrogen_identity_and_failures() {
    let nested = fixture("project/parser/branch_order_nested.pdbqt");
    let parsed = parse_pdbqt(&nested).unwrap();
    let serials = parsed
        .atoms
        .iter()
        .map(|atom| atom.raw.serial)
        .collect::<Vec<_>>();
    assert_eq!(serials, vec![10, 30, 20, 25, 40]);
    assert_eq!(parse_pdbqt(&write_pdbqt(&parsed).unwrap()).unwrap(), parsed);

    let hydrogens = fixture("project/parser/h_hd_aliases.pdbqt");
    let parsed = parse_pdbqt(&hydrogens).unwrap();
    assert_eq!(parsed.atoms[1].ad_type.pdbqt_symbol(), "H");
    assert_eq!(parsed.atoms[2].ad_type.pdbqt_symbol(), "HD");
    let prepared = prepare_ligand_pdbqt(&hydrogens).unwrap();
    assert_eq!(prepared.source_atoms()[1].xs_type, None);
    assert_eq!(prepared.source_atoms()[2].xs_type, None);
    assert!(prepared.atoms()[0].is_donor());

    let malformed = fixture("project/parser/malformed_unclosed_branch.pdbqt");
    assert!(parse_pdbqt(&malformed)
        .unwrap_err()
        .contains("unclosed BRANCH"));
}

#[test]
fn all_generated_scoring_cases_match_the_vina_1_2_7_oracle() {
    let root = fixture_root();
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(root.join("manifest.json")).unwrap()).unwrap();
    let fixtures = manifest["fixtures"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            (
                entry["id"].as_str().unwrap(),
                entry["path"].as_str().unwrap(),
            )
        })
        .collect::<std::collections::HashMap<_, _>>();

    for case in manifest["scoring_cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        assert_eq!(case["oracle_status"], "generated", "{id}");
        assert_eq!(case["oracle_method"], "vina.Vina.score()", "{id}");
        assert_eq!(case["oracle_version"], "1.2.7", "{id}");
        let scoring = case["scoring"].as_str().unwrap();
        let tolerance = &manifest["tolerances"][scoring];
        let affinity_tolerance = tolerance["affinity_kcal_mol_abs"].as_f64().unwrap();
        let component_tolerance = tolerance["component_kcal_mol_abs"].as_f64().unwrap();
        let receptor_path = fixtures[case["receptor_fixture"].as_str().unwrap()];
        let ligand_path = fixtures[case["ligand_fixture"].as_str().unwrap()];
        let receptor = prepare_receptor_pdbqt(&fixture(receptor_path)).unwrap();
        let ligand = prepare_ligand_pdbqt(&fixture(ligand_path)).unwrap();
        let positions = ligand
            .atoms()
            .iter()
            .map(|atom| atom.position)
            .collect::<Vec<_>>();
        let engine = match scoring {
            "vina" => VinaEngine::classic(),
            "vinardo" => VinaEngine::new(VINARDO).unwrap(),
            other => panic!("unsupported scoring mode {other}"),
        };
        let actual = engine.score(&receptor, &ligand, &positions).unwrap();
        let expected = &case["expected"];
        assert_close(
            id,
            "estimated affinity",
            actual.reported_affinity,
            expected["estimated_free_energy_of_binding_kcal_mol"]
                .as_f64()
                .unwrap(),
            affinity_tolerance,
        );
        assert_close(
            id,
            "intermolecular energy",
            actual.raw_inter,
            expected["final_intermolecular_energy_kcal_mol"]
                .as_f64()
                .unwrap(),
            component_tolerance,
        );
        assert_close(
            id,
            "ligand-receptor energy",
            actual.raw_inter,
            expected["ligand_receptor_energy_kcal_mol"]
                .as_f64()
                .unwrap(),
            component_tolerance,
        );
        assert_close(
            id,
            "ligand internal energy",
            actual.raw_intra,
            expected["ligand_internal_energy_kcal_mol"]
                .as_f64()
                .unwrap(),
            component_tolerance,
        );
        assert_close(
            id,
            "final total internal energy",
            actual.raw_intra,
            expected["final_total_internal_energy_kcal_mol"]
                .as_f64()
                .unwrap(),
            component_tolerance,
        );
        assert_close(
            id,
            "torsional contribution",
            actual.reported_affinity - actual.raw_inter,
            expected["torsional_free_energy_kcal_mol"].as_f64().unwrap(),
            component_tolerance,
        );
        assert_close(
            id,
            "unbound system energy",
            actual.raw_intra,
            expected["unbound_system_energy_kcal_mol"].as_f64().unwrap(),
            component_tolerance,
        );

        let oracle: Value = serde_json::from_str(
            &fs::read_to_string(root.join(case["oracle_output_path"].as_str().unwrap())).unwrap(),
        )
        .unwrap();
        assert_eq!(oracle["case_id"], id);
        assert_eq!(
            oracle["oracle"]["source_commit"],
            manifest["oracle"]["git_commit"]
        );
        let raw = oracle["api_result"]["raw_vector_kcal_mol"]
            .as_array()
            .unwrap();
        assert_eq!(raw.len(), 8);
        assert_eq!(
            raw[2], 0.0,
            "{id} flex_inter must be zero for rigid receptor"
        );
        assert_eq!(
            raw[3], 0.0,
            "{id} other_inter must be zero for rigid receptor"
        );
        assert_eq!(
            raw[4], 0.0,
            "{id} flex_intra must be zero for rigid receptor"
        );
    }
}

fn assert_close(case: &str, field: &str, actual: f64, expected: f64, tolerance: f64) {
    let delta = (actual - expected).abs();
    assert!(
        delta <= tolerance,
        "{case} {field}: actual={actual}, expected={expected}, delta={delta}, tolerance={tolerance}"
    );
}
