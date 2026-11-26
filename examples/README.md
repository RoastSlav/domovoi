# Examples

Release archives ship this folder next to the binary (`/usr/local/share/domovoi/examples/` after `install.sh`). Nothing here is needed to run Domovoi: a new hub is set up in its web UI and an agent writes its own `agent.toml` when it enrolls.

| File | What it is for |
| --- | --- |
| `hub.toml` | A short, commented starting point for a hub that is managed as code. Everything in it is commented out. Anything you set is locked in the web UI. |
| `hub.full.toml` | Every `hub.toml` section and key with its default or an example value. Copy the parts you need; the [configuration reference](https://RoastSlav.github.io/domovoi/reference/configuration.html) describes each one. |
| `agent.toml` | Every `agent.toml` section and key. Use it to see what an agent can be told; `domovoi agent enroll` writes the small file an agent actually needs. |
| `skills/` | Eleven starter skills: eight that only read, `restart_container` and `restart_unit` that change something, and `vacuum_journal`, which cannot be undone. Each folder has a `skill.toml`, an executable `run` and a `README.md`. |

Secrets are never written in these files. A secret is `{ env = "NAME" }` or `{ file = "/path" }`, and the example names are placeholders for variables you set yourself.

## Check a file

```sh
domovoi check config hub.toml --kind hub        # a hub file; every problem is reported with its TOML path
domovoi check config agent.toml --kind agent    # an agent file
domovoi check skills ./skills                   # a skills folder, with one line per skill
```

`--kind` is needed for `hub.toml` because it has no active table to guess from. Secrets are resolved when a file is checked, so `hub.full.toml` and `agent.toml` report every environment variable they name that is not set. The files here are loaded without error or warning by a test, and the skills load through the runner's own manifest validation, so they stay valid as the product changes.

## Use the starter skills

Copy the folders you want into the agent's skills directory (`/etc/domovoi/skills` by default), keep the tree owned by root, validate it and restart the agent:

```sh
sudo cp -r skills/system_overview skills/top_processes /etc/domovoi/skills/
sudo domovoi check skills /etc/domovoi/skills
sudo systemctl restart domovoi-agent
```

The [skills guide](https://RoastSlav.github.io/domovoi/guide/skills.html) describes every starter skill and how to write your own.
