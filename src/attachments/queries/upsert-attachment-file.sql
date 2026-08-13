-- single write path for every resolver (live handler, drain, oversized-retry).
-- The WHERE guard makes 'stored' terminal: a racing drain can never downgrade a row
-- the live path already stored (and vice versa); duplicate 'stored' writes are no-ops.
INSERT INTO attachment_files (attachment_id, status, path, size_bytes, content_type, stored_at)
VALUES ($1, $2, $3, $4, $5, $6)
ON CONFLICT (attachment_id) DO UPDATE
SET status = EXCLUDED.status,
    path = EXCLUDED.path,
    size_bytes = EXCLUDED.size_bytes,
    content_type = EXCLUDED.content_type,
    stored_at = EXCLUDED.stored_at
WHERE attachment_files.status <> 'stored';
