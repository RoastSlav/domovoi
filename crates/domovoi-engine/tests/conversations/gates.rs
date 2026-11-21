//! Gate tests: the Operator only reads (INV-1, INV-9). Never delete or weaken these.

use std::path::Path;

/// Names that would let conversation code approve, authorize or run something.
const FORBIDDEN: &[&str] = &[
    "HumanApproval",
    "Authorization",
    "ActionRequest",
    "ActionHandle",
    "OperationHandle",
    "AuthorizedExecution",
    "AuthorizedStep",
    "execute_step",
    "PlanHash",
    "ProcessExecutor",
    "AgentLink",
    "HandoffRunner",
    "run_skill",
    "run_capability",
    "start_skill",
    "PolicyPermit",
];

fn scan(dir: &Path, files: &mut usize, offenders: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan(&path, files, offenders);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            *files += 1;
            let text = std::fs::read_to_string(&path).unwrap();
            for (number, line) in text.lines().enumerate() {
                if line.trim_start().starts_with("//") {
                    continue;
                }
                for name in FORBIDDEN {
                    if line.contains(name) {
                        offenders.push(format!("{}:{}: {name}", path.display(), number + 1));
                    }
                }
            }
        }
    }
}

#[test]
fn operator_has_no_executor() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/conversations");
    let mut files = 0;
    let mut offenders = Vec::new();
    scan(&root, &mut files, &mut offenders);
    assert!(files >= 8, "the scan found only {files} files");
    assert!(
        offenders.is_empty(),
        "executor names in conversations: {offenders:?}"
    );
}
