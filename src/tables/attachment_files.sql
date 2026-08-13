/* local file store bookkeeping: one row per attachment we've resolved.
   stored    — file on disk at `path` (relative to ATTACHMENTS_DIR)
   oversized — exceeded ATTACHMENTS_MAX_BYTES; size_bytes records the Discord-reported
               size so a raised cap re-admits it automatically
   gone      — message or attachment deleted upstream (REST 404); never retried
   No row = not yet attempted or transient failure; the boot-time drain retries. */
CREATE TABLE IF NOT EXISTS attachment_files (
    attachment_id bigint PRIMARY KEY NOT NULL REFERENCES attachments(id),
    status text NOT NULL CHECK (status IN ('stored', 'oversized', 'gone')),
    path text,
    size_bytes bigint,
    content_type text,
    stored_at timestamp NOT NULL
);
