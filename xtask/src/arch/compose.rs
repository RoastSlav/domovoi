//! UNIT-8, compose clause: no compose volume mounts `docker.sock` read-write.

use super::Violation;
use super::identity_rules::violation;

/// UNIT-8, compose clause: no `volumes:` entry that names `docker.sock` is read-write.
/// `files` are `(path, text)` pairs; only the short `source:target[:options]` form is read.
pub fn check_compose(files: &[(String, String)]) -> Vec<Violation> {
    let mut out = Vec::new();
    for (path, text) in files {
        let mut volumes_indent: Option<usize> = None;
        for (index, line) in text.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let indent = line.len() - line.trim_start().len();
            if let Some(base) = volumes_indent {
                if indent <= base && !trimmed.starts_with('-') {
                    volumes_indent = None;
                } else if let Some(item) = trimmed.strip_prefix('-')
                    && let Some(problem) = writable_socket(item)
                {
                    out.push(violation(
                        "UNIT-8",
                        &format!("{path}:{}", index + 1),
                        &problem,
                    ));
                }
            }
            if volumes_indent.is_none() && trimmed == "volumes:" {
                volumes_indent = Some(indent);
            }
        }
    }
    out
}

/// A description of the problem when a volume entry binds `docker.sock` without `ro`.
fn writable_socket(item: &str) -> Option<String> {
    let item = item.split(" #").next().unwrap_or(item);
    let item = item.trim().trim_matches(['"', '\'']);
    if !item.contains("docker.sock") {
        return None;
    }
    let read_only = item
        .split(':')
        .skip(2)
        .any(|options| options.split(',').any(|o| o == "ro"));
    (!read_only).then(|| format!("the volume `{item}` binds docker.sock read-write"))
}
