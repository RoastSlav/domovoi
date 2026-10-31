//! Invariant gates for skill invocations (INV-2, ADR 0004). Never weaken these.

use std::collections::BTreeMap;
use std::path::Path;

use domovoi_core::SkillManifest;
use domovoi_runner::exec::SHELL_NAMES;
use domovoi_runner::skill_run::build_spec;
use proptest::prelude::*;

/// One generated param: (required, kind is integer, supplied value).
type Param = (bool, bool, Option<String>);

fn param() -> impl Strategy<Value = Param> {
    (any::<bool>(), any::<bool>()).prop_flat_map(|(required, integer)| {
        let value: BoxedStrategy<String> = if integer {
            (0i64..=1000).prop_map(|n| n.to_string()).boxed()
        } else {
            "[ -~]{0,24}".boxed()
        };
        let value = if required {
            value.prop_map(Some).boxed()
        } else {
            proptest::option::of(value).boxed()
        };
        value.prop_map(move |v| (required, integer, v))
    })
}

fn manifest_text(params: &[Param]) -> String {
    let mut text = String::from(
        "name = \"generated\"\ndescription = \"d\"\nrisk = \"write\"\nentry = \"run\"\n\
         timeout = \"30s\"\nmax_output_bytes = 4096\n",
    );
    for (i, (required, integer, _)) in params.iter().enumerate() {
        let kind = if *integer {
            "{ type = \"integer\", min = 0, max = 1000 }"
        } else {
            "{ type = \"string\", pattern = '^[ -~]{0,24}$', max_len = 24 }"
        };
        text.push_str(&format!(
            "\n[[params]]\nname = \"p{i}\"\ndescription = \"p\"\nrequired = {required}\nkind = {kind}\n"
        ));
    }
    text
}

proptest! {
    #[test]
    fn argv_never_contains_shell_invocation(params in proptest::collection::vec(param(), 0..8)) {
        let manifest: SkillManifest = toml::from_str(&manifest_text(&params)).unwrap();
        manifest.validate().unwrap();
        let supplied: BTreeMap<String, String> = params
            .iter()
            .enumerate()
            .filter_map(|(i, (_, _, v))| v.clone().map(|v| (format!("p{i}"), v)))
            .collect();
        let args = manifest.validate_args(&supplied).unwrap();

        let dir = Path::new("/srv/domovoi/skills/generated");
        let entry = dir.join("run");
        let spec = build_spec(&manifest, &entry, dir, &args);

        // argv[0] is the entry inside the skill folder, never a shell.
        prop_assert_eq!(&spec.program, entry.to_str().unwrap());
        prop_assert!(Path::new(&spec.program).starts_with(dir));
        let file = Path::new(&spec.program).file_name().unwrap().to_str().unwrap();
        prop_assert!(!SHELL_NAMES.contains(&file));
        // One argv element per declared param, carrying the value verbatim.
        prop_assert_eq!(spec.args.len(), params.len());
        for (i, (_, _, value)) in params.iter().enumerate() {
            prop_assert_eq!(&spec.args[i], &value.clone().unwrap_or_default());
        }
        // Parameters reach the environment only under the reserved prefix.
        for key in spec.env.keys() {
            prop_assert!(key == "DOMOVOI_SKILL" || key.starts_with("DOMOVOI_PARAM_"));
        }
    }
}
