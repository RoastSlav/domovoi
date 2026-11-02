# Troubleshooting

## The browser cannot reach the development UI

Start the example hub from the repository root with `cargo run -p domovoi-hub --example
dev_hub`, then start the Vite server in `web/` with `npm ci` and `npm run dev`. Open the setup
link printed by the hub, replacing port `7420` with `5173` as described in [Quick
start](quick-start.md).

## I cannot enroll an agent or validate a config file

That is expected in the current pre-alpha preview. The production `hub`, `agent` and `check
config` commands are not wired into the `domovoi` binary yet. The configuration examples and
protocol reference describe runtime-library behavior, not a supported installation workflow.

## I need monitoring now

Keep your existing monitoring in place. The preview is intended for local UI and development
work only; it has no supported release, migration or upgrade path.
