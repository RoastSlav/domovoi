# Control room, attention and search

Three pages answer three questions: is anything wrong and what is Domovoi doing (**Control room**), what waits for me (**Attention**), and where did I see that (**Search**). They follow the host selector, except Search, which always covers every host.

## The Control room

The Control room is the first page. It has four tiles, each a link to the list behind it:

| Tile                 | What it counts                                                                                                                                                                                                                                                               |
| -------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Needs you            | The attention centre's items, as one number. It sums incident groups that need action, operations waiting for approval, operations waiting for an answer, paused operations (including those with an unknown result) and offline or incompatible hosts. It does not count block items (see below). |
| Running now          | Operations that are running. It opens the Operations page filtered to running.                                                                                                                                                                                               |
| Waiting for approval | Operations with a step waiting for approval. Nothing runs until you decide.                                                                                                                                                                                       |
| Hosts online         | Hosts whose agent is connected now, out of the hosts that have or should have an agent. The hub's own machine is not counted.                                                                                                                                                |

Under the tiles, **Needs you** shows the first five items that are not approvals, **Running now** lists running operations, **Waiting for approval** shows the first three approval cards, and **Hosts** shows only the hosts that need a look, the worst first. A failed source is named in a message with a Retry button; a failure never reads as "nothing needs you".

## What needs you, and in what order

The attention centre and the "Needs you" list read the same items in one fixed order. The list is sorted in this order, and each position belongs to an urgency heading.

| Order | Item                                              | Heading         |
| ----- | ------------------------------------------------- | --------------- |
| 1     | An operation whose result is unknown              | Act now         |
| 2     | Blocks lost on a host (see [Blocking a network source](containment.md)) | Act now |
| 3     | An incompatible host                              | Act now         |
| 4     | A critical incident                               | Act now         |
| 5     | An offline host                                   | Act now         |
| 6     | An approval, the longest waiting first            | Waiting for you |
| 7     | A question from an operation                      | Waiting for you |
| 8     | A paused operation                                | Waiting for you |
| 9     | A host that is expected to be incompatible        | Waiting for you |
| 10    | A warning incident, or blocking is off on a host  | Worth a look    |
| 11    | Any other incident                                | Worth a look    |

Within a position, the most recent item comes first.

- **Act now** means something may have happened that you have not confirmed, or a part of your setup cannot do its job. An unknown result may have run on the host: check the host before continuing.
- **Waiting for you** means Domovoi has stopped until you decide, answer or resume.
- **Worth a look** means nothing is blocked, but an incident is open.

Only the first page of each source is read. When a source has more, the page says so and points to Incidents or Operations.

## Approvals are decided in one card

An approval looks and behaves the same everywhere: the card on the operation page, in the Control room and in the attention centre. Approve and Deny live inside the card, and approving asks for a confirmation first. In the attention centre Approve is the primary button; in the Control room it is secondary, because the page is a summary. Rows that are not approvals link to the place where the decision is made.

## Incompatible is not offline

An **offline** host is one whose agent is not connected. An **incompatible** host is one whose agent speaks a protocol version the hub cannot use: it may be connected, but the hub will not send it work. The fix differs (update the agent, not the network), so the pages never show an incompatible host as offline. A host that is _expected_ to be incompatible, for example during an upgrade you started, ranks lower and sits under "Waiting for you". An incompatible host still counts in the hub's `hosts_offline`, and so in the "Needs you" total.

## Search

**Search** looks in six places for one text of at least two characters:

- **Incidents**, searched by the hub, with a link to the Incidents page.
- **Active operations**, the operations that are not finished. The browser matches the goal and the host name against the newest 50 operations of each state, so it says so when a state has more.
- **Past operations**, searched by the hub across every host. These are historical evidence, not authorization: a past fix is a hint about what worked, never permission to run it again.
- **Notes**, searched by the hub, with the matching excerpt.
- **Facts**, searched by the hub. Facts have no page of their own yet, so they are shown, not linked.
- **Hosts**, matched by name in the browser.

Each group shows five rows and a **Show all** button for the rest of what was loaded. A page states what it searched and who does the matching. A source that fails is named, with Retry, and the other groups still show their results; a failure is never shown as "nothing found".
