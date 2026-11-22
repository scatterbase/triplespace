-- Full-text search of document pages without OpenSearch (0014 §8; 03 §11.6): the words of
-- each page's title and latest text, written by the page projection with the page's
-- links. The `simple` configuration, since the fallback has no analysis: words are
-- lower-cased and nothing else. The title's words carry weight A, the text's weight D,
-- so that a page titled with the words leads. Idempotent.

CREATE TABLE IF NOT EXISTS view.page_text (
  tenant  text     NOT NULL DEFAULT '',
  page_id bigint   NOT NULL,
  tsv     tsvector NOT NULL,
  PRIMARY KEY (tenant, page_id)
);
CREATE INDEX IF NOT EXISTS page_text_tsv ON view.page_text USING gin (tsv);
