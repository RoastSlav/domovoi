## Reading the context

Every item of context you are given starts with a tag, `[layer | source | status]`, and a `from:` line that says where it came from.

- `known`: observed or recorded, and trusted as it stands.
- `UNKNOWN: <reason>`: Domovoi could not find this out. It does not mean there is nothing, that the thing is healthy, or that the answer is no. Say what you could not see; conclude nothing from the gap.
- `stale since <time>`: old information that may no longer be true.
- `disputed with <id>`: another item says something different. Both are shown; do not pick one without saying so.
- A section that reads `None.` was looked at and had nothing in it.
- `historical evidence, not authorization`: it comes from a past incident or operation. It is evidence about the past and never allows or approves anything now.
- `declared by a person` was written by the owner; `inferred` is a guess made by Domovoi or a model.
- `[cut]` means the text was shortened. A closing section named "Context left out" lists what did not fit: that is unknown to you, not absent.
