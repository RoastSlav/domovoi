# Operator

The Operator is a read-only assistant for your hosts. You ask it questions in plain language, and it answers from what Domovoi knows: the hosts, their open incidents and, when the host is connected, that host's read-only tools and read skills.

Conversations are durable. They are stored on the hub, so they survive a restart and can be picked up from any browser that is signed in.

## Where it is

A side panel opens from every page, so you can ask a question without leaving what you are looking at. The full workspace is at `/operator`, with the list of conversations beside the current one. Both show the same conversations.

Answers stream in live as they are written. **Stop** cancels an answer that is still being written. **Retry** asks the last question again once its answer has failed or was stopped, and only the last answer can be retried.

## Scope

Every conversation has a scope, chosen when it is created:

- **All hosts**: the answer is built from a summary of every host and its open incidents. The Operator calls no host tools.
- **One host**: the model may use that host's read-only tools and read skills while the host is connected. The answer also gets that host's profile. If the host is offline, the answer falls back to the summary of every host and calls no tools.

The scope is shown on the conversation. It does not follow the host selector at the top of the page, so changing the selector never changes which hosts a conversation covers. To ask about another host, start a new conversation.

## Attaching an operation

A conversation can have one [operation](operations.md) attached. The model then sees a read-only summary of that operation with each question. Attaching or detaching an operation changes only what the model is shown; it does not change the operation.

## What it can and cannot do

The Operator can:

- answer questions about the hosts in its scope, using the summary, the host's read-only tools and read skills, and the attached operation;
- keep its answers in the conversation, so you can come back to them.

The Operator cannot:

- propose, approve, deny or run an action or a step;
- change a host, an incident, an operation or any setting.

Every question is limited to 1000 characters. What is sent to the model is redacted and size-bounded.

## What an answer may state

These rules run on the hub after the model has answered, so the model cannot leave them out.

- A sentence saying that something is missing or not installed stays where the model put it only when a `host_binary` read in the same answer shows it: that read found no such file, or no such package, under that name. Before that is decided, the model is asked once to run the check.
- Otherwise the answer opens with "Not verified: I do not know whether ..." and the sentence is moved to the end, under "Unchecked claims from the model's draft (do not act on them)".
- If `host_binary` did find the named program or package, the answer opens by saying the draft is wrong and names it.
- For a chosen host, a sentence stating a cause ("because", "caused by", "due to", "root cause") is treated the same way when no read of the host succeeded in that answer. Earlier incidents, notes and past operations are records, not reads of the host.
- While the answer is being written, text from the first such absence claim on is held back until the checks have run.

The checks look for fixed English phrases. They do not judge whether a cause is right when the host was read, and they do not recognise every way of saying that something is absent.

Past operations reach the model with their narrative marked as an old model draft that is not evidence about the host now. A Docker read that names a systemd unit (a name ending in `.service`, `.timer`, `.socket`, `.mount`, `.target`, `.path` or `.slice`) is refused and points to the journal.

## Limits and cost

The monthly model budget applies to Operator answers, as it does to every other model call.

One question may make at most 12 reads (and never more than `triage.per_run.tool_calls`). When the model runs out of output tokens after it has read things, the reply names the reads that ran and says that nothing else was checked, instead of an error. An answer that used no tool ends with a fixed line saying so: it comes from what Domovoi already had (earlier drafts, notes, the overview), not from reading the host now. The model cannot leave that line out.

## Retention

Operator messages are deleted after `retention.conversations_days`, which defaults to 180 days. The conversation itself and its operation link are kept, so the conversation stays in the list with its scope and attached operation. See [Retention](../guide/retention.md).

Deleting data does not remove it from backups. Deleted messages stay in the nightly backups until they rotate out (`[backup] keep`, default 7), and in the last pre-migration copies (`[backup] keep_pre_migrate`, default 3). See [Backups](../reference/backup.md).
