-- Some preview profiles recorded the historical 2025-08-05 migration before
-- that migration began creating welcome_panes. A newer application then sees
-- the version as applied but cannot persist workspace state. Repair those
-- profiles with a new, forward-only migration; this is a no-op on fresh or
-- already-correct databases.
CREATE TABLE IF NOT EXISTS welcome_panes (
  id INTEGER PRIMARY KEY NOT NULL,
  kind TEXT NOT NULL DEFAULT 'welcome' CHECK (kind = 'welcome'),
  startup_directory TEXT,
  FOREIGN KEY (id, kind) REFERENCES pane_leaves (pane_node_id, kind)
);
