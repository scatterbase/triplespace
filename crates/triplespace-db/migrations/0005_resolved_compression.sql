-- The resolved canonical JSON is stored compressed (ADR 0013 §5.1), by the same TOAST
-- method as record bodies (0013 §2): lz4, which decompresses fast enough for the read
-- path and needs no extension. The projection writes the plain bytes.
ALTER TABLE view.entity ALTER COLUMN resolved SET COMPRESSION lz4;
