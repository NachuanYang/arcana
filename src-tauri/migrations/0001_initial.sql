CREATE TABLE tarot_sessions (
  id TEXT PRIMARY KEY NOT NULL,
  question TEXT NOT NULL,
  spread_id TEXT NOT NULL CHECK (spread_id = 'three_present_obstacle_tendency'),
  deck_version TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN (
    'REVEALED', 'READING_PENDING', 'READING_STREAMING',
    'READING_READY', 'READING_FAILED_RETRYABLE', 'READING_CANCELLED', 'ARCHIVED'
  )),
  created_at TEXT NOT NULL,
  revealed_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  reading_stance TEXT
);

CREATE TABLE tarot_drawn_cards (
  session_id TEXT NOT NULL REFERENCES tarot_sessions(id) ON DELETE CASCADE,
  slot TEXT NOT NULL CHECK (slot IN ('present', 'obstacle', 'tendency')),
  card_id TEXT NOT NULL,
  orientation TEXT NOT NULL CHECK (orientation IN ('upright', 'reversed')),
  visual_order INTEGER NOT NULL CHECK (visual_order BETWEEN 0 AND 2),
  card_snapshot_json TEXT NOT NULL CHECK (json_valid(card_snapshot_json)),
  PRIMARY KEY (session_id, slot),
  UNIQUE (session_id, card_id),
  UNIQUE (session_id, visual_order)
);

CREATE TABLE tarot_turns (
  id TEXT PRIMARY KEY NOT NULL,
  session_id TEXT NOT NULL REFERENCES tarot_sessions(id) ON DELETE CASCADE,
  turn_order INTEGER NOT NULL CHECK (turn_order > 0),
  role TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
  request_id TEXT,
  content_json TEXT NOT NULL CHECK (json_valid(content_json)),
  status TEXT NOT NULL CHECK (status IN ('PENDING', 'STREAMING', 'COMPLETE', 'FAILED_RETRYABLE', 'CANCELLED')),
  model_id TEXT,
  created_at TEXT NOT NULL,
  UNIQUE (session_id, turn_order),
  UNIQUE (session_id, request_id, role)
);

CREATE TABLE app_settings (
  key TEXT PRIMARY KEY NOT NULL,
  value_json TEXT NOT NULL CHECK (json_valid(value_json))
);

CREATE INDEX ix_tarot_sessions_created_at ON tarot_sessions(created_at DESC);
CREATE INDEX ix_tarot_turns_session ON tarot_turns(session_id, turn_order);
