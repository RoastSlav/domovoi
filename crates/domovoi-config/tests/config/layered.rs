//! Settings layers: defaults, UI, file, environment (ADR 0017).

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::io::Write;

    use domovoi_config::{
        ConfigIssue, MapEnv, NoStoredSecrets, Source, StoredSecrets, load_hub_layered,
    };
    use secrecy::{ExposeSecret, SecretString};

    struct Stored(BTreeMap<&'static str, &'static str>);

    impl StoredSecrets for Stored {
        fn get(&self, path: &str) -> Option<SecretString> {
            self.0
                .get(path)
                .map(|v| SecretString::from((*v).to_owned()))
        }
    }

    fn table(text: &str) -> toml::Table {
        text.parse().unwrap()
    }

    fn file(text: &str) -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        write!(file, "{text}").unwrap();
        file
    }

    fn warning_at(issues: &[ConfigIssue], path: &str) -> bool {
        issues.iter().any(|i| !i.is_error() && i.path == path)
    }

    #[test]
    fn layers_merge_defaults_ui_file_env_in_order() {
        let ui = table(
            "[hub]\nname = \"from-ui\"\ntimezone = \"Europe/Sofia\"\n[triage]\nconcurrency = 3\nper_hour_runs = 9\n",
        );
        let toml = file("[triage]\nconcurrency = 4\ncache_days = 2\n");
        let env = MapEnv::new().with("DOMOVOI_TRIAGE__CACHE_DAYS", "5");
        let layered = load_hub_layered(Some(toml.path()), &ui, &NoStoredSecrets, &env).unwrap();
        let c = &layered.config;
        assert_eq!(c.hub.name, "from-ui", "UI over default");
        assert_eq!(c.hub.timezone.name(), "Europe/Sofia");
        assert_eq!(
            c.triage.per_hour_runs, 9,
            "UI value kept where the file is silent"
        );
        assert_eq!(c.triage.concurrency, 4, "file over UI");
        assert_eq!(c.triage.cache_days, 5, "environment over file");
        assert_eq!(c.incidents.confirm_runs, 2, "default where nothing is set");
    }

    #[test]
    fn provenance_names_the_layer_for_each_field() {
        let ui = table("[hub]\nname = \"ui\"\n[notify.digest]\nenabled = false\n");
        let toml = file(
            "[notify.digest]\ntime = \"08:00\"\n[[probes]]\nhost = \"nas\"\nkind = \"tcp\"\ntarget = \"nas:22\"\n",
        );
        let env = MapEnv::new().with("DOMOVOI_TRIAGE__CONCURRENCY", "3");
        let p = load_hub_layered(Some(toml.path()), &ui, &NoStoredSecrets, &env)
            .unwrap()
            .provenance;
        assert_eq!(p.source_of("hub.name"), Source::Ui);
        assert_eq!(p.source_of("notify.digest.enabled"), Source::Ui);
        assert_eq!(p.source_of("notify.digest.time"), Source::File);
        assert_eq!(p.source_of("probes"), Source::File);
        assert_eq!(p.source_of("probes.0.target"), Source::File);
        assert_eq!(
            p.source_of("triage.concurrency"),
            Source::Env("DOMOVOI_TRIAGE__CONCURRENCY".to_owned())
        );
        assert_eq!(p.source_of("hub.timezone"), Source::Default);
    }

    #[test]
    fn bootstrap_keys_in_stored_document_are_ignored_with_warning() {
        let ui = table(
            "[hub]\ndata_dir = \"/tmp/elsewhere\"\nname = \"x\"\n[web]\nbind = \"0.0.0.0:1\"\n[agent_server]\nbind = \"0.0.0.0:2\"\n",
        );
        let layered = load_hub_layered(None, &ui, &NoStoredSecrets, &MapEnv::new()).unwrap();
        assert_eq!(
            layered.config.hub.data_dir,
            std::path::PathBuf::from("/var/lib/domovoi")
        );
        assert_eq!(layered.config.web.bind.to_string(), "127.0.0.1:7420");
        assert_eq!(layered.config.agent_server.bind.to_string(), "0.0.0.0:7421");
        for key in ["hub.data_dir", "web.bind", "agent_server.bind"] {
            assert!(
                warning_at(&layered.issues, key),
                "{key}: {:#?}",
                layered.issues
            );
            assert_eq!(layered.provenance.source_of(key), Source::Default);
        }
        assert_eq!(layered.config.hub.name, "x");
    }

    #[test]
    fn stored_secret_resolves_and_debug_never_shows_it() {
        let ui = table(
            "[llm.primary]\nkind = \"anthropic\"\nmodel = \"m\"\napi_key = { stored = true }\n",
        );
        let stored = Stored(BTreeMap::from([("llm.primary.api_key", "sk-stored-value")]));
        let layered = load_hub_layered(None, &ui, &stored, &MapEnv::new()).unwrap();
        let key = layered
            .config
            .llm
            .primary
            .as_ref()
            .unwrap()
            .api_key
            .as_ref()
            .unwrap();
        assert_eq!(key.value().expose_secret(), "sk-stored-value");
        let debug = format!("{:?}", layered.config);
        assert!(!debug.contains("sk-stored-value"));
        assert!(debug.contains("Stored"));

        let toml = file("[heartbeat]\nurl = { stored = true }\n");
        let err = load_hub_layered(
            Some(toml.path()),
            &toml::Table::new(),
            &stored,
            &MapEnv::new(),
        )
        .unwrap_err();
        assert!(
            err.issues
                .iter()
                .any(|i| i.is_error() && i.path == "heartbeat.url"),
            "a file may not use stored secrets: {:#?}",
            err.issues
        );
        assert!(
            domovoi_config::parse::<domovoi_config::HubConfig>(
                "[heartbeat]\nurl = { stored = true }\n",
                &MapEnv::new()
            )
            .is_err()
        );
    }

    #[test]
    fn missing_stored_secret_resolves_unset_with_warning() {
        let ui = table(
            "[llm.primary]\nkind = \"openai_compatible\"\nmodel = \"m\"\nbase_url = \"http://127.0.0.1:1/v1\"\napi_key = { stored = true }\n\
             [heartbeat]\nurl = { stored = true }\n\
             [webhooks.alerts]\nkind = \"generic\"\nauth = \"hmac\"\nsecret = { stored = true }\n",
        );
        let layered = load_hub_layered(None, &ui, &NoStoredSecrets, &MapEnv::new()).unwrap();
        let c = &layered.config;
        assert!(
            c.llm.primary.as_ref().unwrap().api_key.is_none(),
            "optional secret is unset"
        );
        assert!(
            c.heartbeat.is_none(),
            "heartbeat cannot run without its URL"
        );
        assert!(
            c.webhooks.is_empty(),
            "a webhook never runs with an empty secret"
        );
        for path in [
            "llm.primary.api_key",
            "heartbeat.url",
            "webhooks.alerts.secret",
        ] {
            assert!(
                warning_at(&layered.issues, path),
                "{path}: {:#?}",
                layered.issues
            );
        }
    }
}
