BEGIN IMMEDIATE;

CREATE TABLE tarot_sessions_new (
  id TEXT PRIMARY KEY NOT NULL,
  question TEXT NOT NULL,
  spread_id TEXT NOT NULL CHECK (spread_id = 'three_present_obstacle_tendency'),
  deck_version TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN (
    'REVEALED', 'READING_PENDING', 'READING_STREAMING',
    'READING_READY', 'READING_FAILED_RETRYABLE', 'READING_CANCELLED',
    'FOLLOWUP_PENDING', 'FOLLOWUP_STREAMING', 'ARCHIVED'
  )),
  created_at TEXT NOT NULL,
  revealed_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  reading_stance TEXT
);

INSERT INTO tarot_sessions_new
  (id, question, spread_id, deck_version, state, created_at, revealed_at, updated_at, reading_stance)
SELECT id, question, spread_id, deck_version, state, created_at, revealed_at, updated_at, reading_stance
FROM tarot_sessions;

UPDATE tarot_sessions_new
SET reading_stance = (
  SELECT json_extract(content_json, '$.stance')
  FROM tarot_turns
  WHERE tarot_turns.session_id = tarot_sessions_new.id
    AND tarot_turns.role = 'assistant'
    AND tarot_turns.status = 'COMPLETE'
  ORDER BY turn_order
  LIMIT 1
)
WHERE state = 'READING_READY' AND reading_stance IS NULL;

DROP TABLE tarot_sessions;
ALTER TABLE tarot_sessions_new RENAME TO tarot_sessions;
CREATE INDEX ix_tarot_sessions_created_at ON tarot_sessions(created_at DESC);

PRAGMA user_version = 2;
COMMIT;
