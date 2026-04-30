-- Phase D: initial schema for omw-remote.
-- Tables per PRD §10. See specs/byorc-protocol.md §3 + §6.3.

CREATE TABLE IF NOT EXISTS devices (
    id                TEXT PRIMARY KEY,            -- 16 hex chars, first 16 of SHA-256(pubkey)
    name              TEXT NOT NULL,
    public_key        BLOB NOT NULL,               -- 32 bytes Ed25519
    paired_at         TEXT NOT NULL,               -- RFC 3339 UTC
    last_seen         TEXT,
    permissions_json  TEXT NOT NULL,               -- JSON array of capability scopes
    revoked_at        TEXT
);

CREATE TABLE IF NOT EXISTS pairings (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    token_hash          BLOB NOT NULL UNIQUE,      -- SHA-256(pair_token), 32 bytes
    expires_at          TEXT NOT NULL,
    used_at             TEXT,
    used_by_device_id   TEXT REFERENCES devices(id)
);

CREATE INDEX IF NOT EXISTS pairings_token_hash_idx ON pairings(token_hash);

CREATE TABLE IF NOT EXISTS request_log (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    route            TEXT NOT NULL,
    actor_device_id  TEXT,                         -- nullable: pair-redeem has no device yet
    nonce            TEXT,
    ts               TEXT NOT NULL,                -- RFC 3339 UTC
    signature        TEXT,
    body_hash        TEXT,
    accepted         INTEGER NOT NULL,             -- 0 / 1
    reason           TEXT
);

CREATE INDEX IF NOT EXISTS request_log_ts_idx ON request_log(ts);
