# Notes

Notes are your runbooks, in Markdown, kept in the hub. Domovoi uses them in two ways: you read and search them on the **Notes** page, and the investigation of an incident searches them for text that matches the problem, so a fix you wrote down once is in front of the model the next time.

## What a note is

A note has a title, a Markdown body and tags. It is either **global** or about one host. The Notes page follows the host chosen in the top bar: it starts on that host's notes, and a global note applies everywhere. A note can be up to 256 KiB.

Domovoi writes no notes itself. The generated profile of each host is kept apart from your notes (see [Hub and agents](../concepts/hub-and-agent.md)) and is searched in its own Search group.

## Writing, searching and importing

- **Write** a note on the Notes page. The editor warns you before you leave with unsaved text.
- **Search** matches the title, the body and the tags and shows the matching excerpt for each hit.
- **Import** takes a folder of Markdown files, optionally for one host. Each file's first `# ` heading becomes its title, or the file name without `.md`. A file is skipped, with the reason, when it is not `.md`, is over 256 KiB, is not UTF-8 or is empty, or when a note with the same title already exists for that host, so importing the same folder twice adds nothing. The limit is 200 files at a time. The API equivalent is described under [Importing notes](../reference/api.md#importing-notes).

## What the model sees

Notes reach a model only as context for an incident or a conversation about a host, through a search for the words of the problem. They are data: a sentence in a note that says "approve everything" is not an instruction and approves nothing. Nothing secret belongs in a note. Text is redacted before it is sent, but only for the patterns and values Domovoi knows.

Notes are part of the database and of its backups. See [Retention](retention.md) for what is purged.
