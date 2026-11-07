-- Email addresses and the delivery queue (ADR 0100 §1–§5, §8; 03 §4.16; 16 §5.7).
--
-- `private.email` holds a login's addresses: up to `email.max_addresses` rows, each
-- unverified or verified, one verified row the primary. The `login` column is the login's
-- identifier (0089 §2); until instance logins are built it holds the actor key of the
-- account whose session added the address (0100 A1). Only an HMAC of a verification code is
-- stored, with its expiry and the count of wrong entries. A verified address is held by one
-- login across the instance (the partial unique index on the address key). An address that
-- took a hard bounce or a spam complaint is marked `undeliverable` with the reason, and
-- nothing is sent to it until it is verified again (0100 A2).
--
-- The rows belong to the login, which no tenant owns, and leave with no account (0100 §9):
-- they are in no portability class of 0027 §2 and are tagged `never-leaves` so that the
-- bundle of 07 §8.4 cannot read them.
--
-- `ops.delivery` is the delivery queue: every outgoing message, staged complete before it
-- is sent, with attempts, next attempt, last error and expiry; a row is deleted as soon as
-- the transport accepts its message. `kind` is `email` here; the fediverse deliveries of
-- 0022 §7 share the table later. Idempotent: every statement tolerates having run before.

CREATE TABLE IF NOT EXISTS private.email (
  login         text        NOT NULL,            -- the login's identifier (0100 A1: an actor key for now)
  address       text        NOT NULL,            -- as written, the domain in A-labels
  address_key   text        NOT NULL,            -- lower-cased: how two addresses compare
  verified      timestamptz,                     -- when the code was entered; NULL while unverified
  is_primary    boolean     NOT NULL DEFAULT false,
  added         timestamptz NOT NULL DEFAULT now(),
  code_mac      text,                            -- HMAC-SHA256 of the current code, hex; NULL once verified or spent
  code_expires  timestamptz,
  code_wrong    smallint    NOT NULL DEFAULT 0,  -- wrong entries against the current code; five spend it
  code_sent     timestamptz,                     -- when the current code was sent
  undeliverable timestamptz,                     -- a hard bounce or a complaint (0100 A2)
  undeliverable_reason text CHECK (undeliverable_reason IN ('bounce', 'complaint')),
  PRIMARY KEY (login, address_key)
);
CREATE UNIQUE INDEX IF NOT EXISTS email_one_primary ON private.email (login) WHERE is_primary;
CREATE UNIQUE INDEX IF NOT EXISTS email_verified_once ON private.email (address_key) WHERE verified IS NOT NULL;
CREATE INDEX IF NOT EXISTS email_address ON private.email (address_key);
COMMENT ON TABLE private.email IS 'portability: never-leaves';

GRANT SELECT ON private.email TO ts_notify;

CREATE TABLE IF NOT EXISTS ops.delivery (
  id           bigserial   PRIMARY KEY,
  kind         text        NOT NULL DEFAULT 'email' CHECK (kind IN ('email')),
  stream       text        NOT NULL DEFAULT 'transactional' CHECK (stream IN ('transactional', 'notification')),
  recipient    text        NOT NULL,             -- the address
  sender       text        NOT NULL,             -- `mail.from`
  sender_name  text,                             -- the instance's or the tenant's display name
  subject      text        NOT NULL,
  body         text        NOT NULL,             -- plain text, UTF-8, in the recipient's language
  message_id   text        NOT NULL,             -- `{token}@{domain}`, without angle brackets
  enqueued     timestamptz NOT NULL DEFAULT now(),
  next_attempt timestamptz NOT NULL DEFAULT now(),
  attempts     integer     NOT NULL DEFAULT 0,
  last_error   text,
  failed       boolean     NOT NULL DEFAULT false, -- a permanent failure: kept with its error until it expires, never retried
  expires      timestamptz NOT NULL
);
CREATE INDEX IF NOT EXISTS delivery_due ON ops.delivery (next_attempt) WHERE NOT failed;
CREATE INDEX IF NOT EXISTS delivery_expires ON ops.delivery (expires);
