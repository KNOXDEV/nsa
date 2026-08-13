-- attachments of one message still needing a download (same pending predicate as
-- pending-attachment-messages.sql; $2 = the configured size cap)
SELECT a.id
FROM attachments a
LEFT JOIN attachment_files f ON f.attachment_id = a.id
WHERE a.message_id = $1
  AND (f.attachment_id IS NULL
       OR (f.status = 'oversized' AND f.size_bytes <= $2));
