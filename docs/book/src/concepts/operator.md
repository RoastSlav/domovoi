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

## Limits and cost

The monthly model budget applies to Operator answers, as it does to every other model call.

## Retention

Operator messages are deleted after `retention.conversations_days`, which defaults to 180 days. The conversation itself and its operation link are kept, so the conversation stays in the list with its scope and attached operation. See [Retention](../guide/retention.md).

Deleting data does not remove it from backups. Deleted messages stay in the nightly backups until they rotate out (`[backup] keep`, default 7), and in the last pre-migration copies (`[backup] keep_pre_migrate`, default 3). See [Backups](../reference/backup.md).
