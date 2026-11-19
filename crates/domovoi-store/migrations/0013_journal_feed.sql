-- Highest journal seq that retention removed. A stream cursor below it must reset.
CREATE TABLE journal_floor (
    id                 INTEGER PRIMARY KEY CHECK (id = 1),
    pruned_through_seq INTEGER NOT NULL
) STRICT;
INSERT INTO journal_floor (id, pruned_through_seq) VALUES (1, 0);

-- Copy the incident_events rows the dual write has not mirrored. Rows written since the dual write
-- began already have a journal row whose legacy_event_seq names them; rows from before it do not.
-- The shape is exactly the mirror's: kind incident.timeline, payload {kind, summary, detail},
-- actor system:incidents, the original timestamp. Nothing is invented.
INSERT INTO journal (aggregate_type, aggregate_id, kind, payload_v, payload, actor,
                     occurred_at, legacy_event_seq)
SELECT 'incident', e.incident_id, 'incident.timeline', 1,
       json_object('kind', e.kind, 'summary', e.summary,
                   'detail', json(CASE WHEN json_valid(e.detail) THEN e.detail
                                       ELSE json_quote(e.detail) END)),
       'system:incidents', e.at, e.seq
FROM incident_events AS e
WHERE NOT EXISTS (SELECT 1 FROM journal AS j WHERE j.legacy_event_seq = e.seq)
ORDER BY e.at, e.seq;
