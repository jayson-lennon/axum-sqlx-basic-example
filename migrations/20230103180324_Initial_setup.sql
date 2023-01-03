-- Add migration script here
CREATE TABLE IF NOT EXISTS hits (
  target TEXT NOT NULL PRIMARY KEY,
  count  BIGINT NOT NULL
);
