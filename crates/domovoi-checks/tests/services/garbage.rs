//! Every service parser survives arbitrary input.

use std::collections::BTreeMap;

use domovoi_checks::security::{
    Proto, parse_accepted, parse_jail_status, parse_port_range, parse_proc_net, parse_sudo_failure,
};
use domovoi_checks::smart::{evaluate, parse_report, parse_scan};
use domovoi_checks::units::{parse_failed_json, parse_failed_plain, parse_show};
use proptest::prelude::*;

proptest! {
    #[test]
    fn service_parsers_never_panic_on_garbage_input(input in ".{0,400}") {
        let _ = parse_failed_json(&input);
        let _ = parse_failed_plain(&input);
        let _ = parse_show(&input);
        let _ = parse_scan(&input);
        if let Some(report) = parse_report(&input) {
            let _ = evaluate("/dev/sda", &report, &mut BTreeMap::new());
        }
        let _ = parse_accepted(&input);
        let _ = parse_sudo_failure(&input);
        let _ = parse_jail_status(&input);
        let _ = parse_proc_net(&input, Proto::Tcp);
        let _ = parse_proc_net(&input, Proto::Udp);
        let _ = parse_port_range(&input);
    }

    #[test]
    fn proc_net_lines_with_random_hex_never_panic(
        local in "[0-9A-Fa-f:]{0,40}",
        remote in "[0-9A-Fa-f:]{0,40}",
        state in "[0-9A-F]{0,3}",
    ) {
        let text = format!("header\n 0: {local} {remote} {state} rest\n");
        let _ = parse_proc_net(&text, Proto::Tcp);
        let _ = parse_proc_net(&text, Proto::Udp);
    }

    #[test]
    fn smart_reports_with_random_counters_never_panic(
        values in proptest::collection::vec(any::<u64>(), 6),
        warning in any::<u64>(),
        temp in any::<i64>(),
    ) {
        let table: Vec<String> = [5, 187, 197, 198, 199, 9]
            .iter()
            .zip(&values)
            .map(|(id, v)| format!(r#"{{"id":{id},"raw":{{"value":{v}}}}}"#))
            .collect();
        let json = format!(
            r#"{{"ata_smart_attributes":{{"table":[{}]}},"nvme_smart_health_information_log":{{"critical_warning":{warning},"temperature":{temp},"media_errors":{}}}}}"#,
            table.join(","),
            values[0]
        );
        let report = parse_report(&json).unwrap();
        let mut baselines = BTreeMap::new();
        let _ = evaluate("/dev/sda", &report, &mut baselines);
        let _ = evaluate("/dev/sda", &report, &mut baselines);
    }
}
