-- The provider's username on an external identity (ADR 0102 §1, §3, §5; 07 §2.7, §2.10).
--
-- Under strict names (`login.names = provider`) an account's name is derived from the
-- username its login's external identity has with the provider, `{label}-{username}`. The
-- username is learnt from the provider's profile each time the identity authenticates and
-- kept here, so that the derived name of every account can be computed without asking the
-- provider: `logins check` and enacting the mode compare names against it (0102 A1). NULL
-- for an identity that has not authenticated since this column existed. Idempotent.

ALTER TABLE private.binding ADD COLUMN IF NOT EXISTS username text;
