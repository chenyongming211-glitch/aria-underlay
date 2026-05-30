use std::fs;
use std::path::Path;

#[test]
fn production_code_does_not_call_with_phase_directly() {
    let roots = ["src/api", "src/worker", "src/tx/recovery.rs"];
    let mut offenders = Vec::new();

    for root in roots {
        collect_with_phase_calls(Path::new(root), &mut offenders);
    }

    assert!(
        offenders.is_empty(),
        "production code must use TxJournalRecord::transition_phase instead of with_phase: {offenders:?}"
    );
}

fn collect_with_phase_calls(path: &Path, offenders: &mut Vec<String>) {
    if path.is_dir() {
        for entry in fs::read_dir(path).expect("read source directory") {
            let entry = entry.expect("read source entry");
            collect_with_phase_calls(&entry.path(), offenders);
        }
        return;
    }

    if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
        return;
    }

    let content = fs::read_to_string(path).expect("read source file");
    for (index, line) in content.lines().enumerate() {
        if line.contains(".with_phase(") {
            offenders.push(format!("{}:{}", path.display(), index + 1));
        }
    }
}
