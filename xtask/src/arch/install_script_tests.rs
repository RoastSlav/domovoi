//! Tests for the installer line parser and INSTALL-1.

use std::path::Path;

use super::*;

const GOOD: &str = r#"#!/bin/sh
STATE_DIR="${STATE_DIR:-/var/lib/domovoi-agent}"
# BEGIN created-set
CREATED_PATHS="/etc/domovoi \
  $STATE_DIR
  /var/lib/domovoi-helper"
# END created-set

make_dir() { mkdir -p "$1"; }

agent_unit() {
    cat <<EOF
[Service]
ExecStart=/usr/local/bin/domovoi agent run
# systemctl enable --now inside a unit text is not a command
make_dir /never/counted
EOF
}

make_dir /etc/domovoi
make_dir "$STATE_DIR"
install -d -m 0700 /var/lib/domovoi-helper
say "next: systemctl enable --now domovoi-agent"
systemctl daemon-reload
systemctl enable --now domovoi-helper.socket
"#;

#[test]
fn parser_reads_the_created_set_and_skips_heredoc_bodies_and_prose() {
    let script = parse(GOOD);
    assert_eq!(
        script.created.as_deref(),
        Some(
            &[
                "/etc/domovoi".to_string(),
                "/var/lib/domovoi-agent".to_string(),
                "/var/lib/domovoi-helper".to_string()
            ][..]
        )
    );
    assert_eq!(script.first_enable, Some(25));
    assert_eq!(script.creation["/etc/domovoi"], 20);
    assert_eq!(script.creation["/var/lib/domovoi-agent"], 21);
    assert_eq!(script.creation["/var/lib/domovoi-helper"], 22);
    assert!(check(&script, true).is_empty());
}

#[test]
fn installer_creates_every_created_path_before_enabling_sockets() {
    // Failing script 1: a path is created only after the socket starts.
    let late = GOOD.replace("install -d -m 0700 /var/lib/domovoi-helper\n", "")
        + "install -d -m 0700 /var/lib/domovoi-helper\n";
    let found = check(&parse(&late), true);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].rule, "INSTALL-1");
    assert!(
        found[0].message.contains("after the first systemctl"),
        "{found:?}"
    );

    // Failing script 2: a path in the set is never created.
    let never = GOOD.replace("install -d -m 0700 /var/lib/domovoi-helper\n", "");
    let found = check(&parse(&never), true);
    assert!(
        found.iter().any(|v| v
            .message
            .contains("no line creates `/var/lib/domovoi-helper`")),
        "{found:?}"
    );

    // Failing script 3: no created-set block; a violation only when it is required.
    let none = "#!/bin/sh\nsystemctl enable --now domovoi-helper.socket\n";
    assert_eq!(check(&parse(none), true).len(), 1);
    assert!(check(&parse(none), false).is_empty());

    // The real installer.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root");
    let text = std::fs::read_to_string(root.join("install.sh")).expect("install.sh reads");
    let found = check(&parse(&text), super::super::units::REQUIRE_CREATED_SET);
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn a_loop_over_the_created_set_creates_every_path() {
    let script = "\
# BEGIN created-set
CREATED_PATHS=\"/etc/a /etc/b\"
# END created-set
for dir in $CREATED_PATHS; do
    make_dir \"$dir\"
done
systemctl start x.socket
";
    let parsed = parse(script);
    assert_eq!(parsed.creation.len(), 2);
    assert!(check(&parsed, true).is_empty());
}

#[test]
fn only_a_command_line_counts_as_enabling() {
    let script = "\
# BEGIN created-set
CREATED_PATHS=\"/etc/a\"
# END created-set
echo \"systemctl start nothing\"
if systemctl is-enabled x; then :; fi
cmd && systemctl enable y
mkdir -p /etc/a
";
    let parsed = parse(script);
    assert_eq!(parsed.first_enable, Some(6));
    assert_eq!(check(&parsed, true).len(), 1);
}

#[test]
fn here_strings_and_shifts_do_not_open_a_heredoc() {
    assert!(heredoc_openers("x=$((1<<2))").is_empty());
    assert!(heredoc_openers("read a <<< \"$b\"").is_empty());
    assert_eq!(heredoc_openers("cat <<-'END' > f"), ["END"]);
    assert_eq!(heredoc_openers("cat <<EOF"), ["EOF"]);
}
