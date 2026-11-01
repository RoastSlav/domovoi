# Binary and systemd

Coming in v0.1: the verified one-line installer, hardened systemd units for the hub and the agent, and first-run configuration.

## After installing

You do not need to edit any file. Start the hub, then open the setup link it prints once:

```sh
systemctl enable --now domovoi-hub
journalctl -u domovoi-hub | grep setup
```

Create the admin account. The guided first run then sets up the hub name and address, Discord, a model, the daily digest, and your first host. Every step except the first can be skipped and done later on the Settings page.

To manage the hub from files instead, see [Advanced: config as code](config-as-code.md).
