-- Account registration and challenges (ADR 0087 §3, §4, §6, A1).
--
-- `private.challenge_spent` holds a solved challenge until it expires, so a solution is
-- accepted once on every replica of an API pool where there is no shared cache (§3).
--
-- `private.rate_event` holds one row per act that the `createaccount` rate-limit class
-- and the two `badlogin` counters count, where there is no shared cache (§4, A1): read as
-- the rows inside a window, and swept once older than the longest window, a day. The key
-- is an address key (an IPv4 address or an IPv6 /64) or, for the per-account `badlogin`
-- counter, the account's lower-cased name.
--
-- `private.account_request` holds a request for an account while it waits for review
-- (§6), and a rejected one for 7 days so its requester learns the outcome at login. An
-- approved request is deleted as its account is created. Nothing here is the person's to
-- take elsewhere, and none of it enters the log.
-- Idempotent: every statement tolerates having run before.

CREATE TABLE IF NOT EXISTS private.challenge_spent (
  tenant  text        NOT NULL,
  mac     text        NOT NULL,                  -- the challenge's MAC, which names it
  expires timestamptz NOT NULL,
  PRIMARY KEY (tenant, mac)
);
CREATE INDEX IF NOT EXISTS challenge_spent_expires ON private.challenge_spent (expires);
COMMENT ON TABLE private.challenge_spent IS 'portability: never-leaves';

CREATE TABLE IF NOT EXISTS private.rate_event (
  tenant text        NOT NULL,
  class  text        NOT NULL,                   -- `createaccount`, `badlogin-address`, `badlogin-account`
  key    text        NOT NULL,                   -- an address key, or a lower-cased account name
  at     timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS rate_event_key ON private.rate_event (tenant, class, key, at);
CREATE INDEX IF NOT EXISTS rate_event_at ON private.rate_event (at);
COMMENT ON TABLE private.rate_event IS 'portability: never-leaves';

CREATE TABLE IF NOT EXISTS private.account_request (
  id          text        NOT NULL PRIMARY KEY,   -- random, as session IDs are (no sequence in `private`)
  tenant      text        NOT NULL,
  name        text        NOT NULL,              -- the chosen name, normalized
  name_key    text        NOT NULL,              -- lower-cased, for the name-taken check
  credential  jsonb       NOT NULL,              -- {"issuer": "password", "hash": PHC}, or {"issuer", "subject"}
  statement   text,                              -- up to 1,000 characters (account.request_statement)
  address_key text,                              -- seen only with ts-viewip
  filed       timestamptz NOT NULL DEFAULT now(),
  expires     timestamptz NOT NULL,              -- account.request_expiry after filing
  decision    text CHECK (decision IN ('rejected')),
  reviewer    text,                              -- the reviewer's actor key
  decided     timestamptz,
  reason      text
);
CREATE UNIQUE INDEX IF NOT EXISTS account_request_name ON private.account_request (tenant, name_key);
CREATE INDEX IF NOT EXISTS account_request_open ON private.account_request (tenant, filed) WHERE decision IS NULL;
COMMENT ON TABLE private.account_request IS 'portability: never-leaves';
