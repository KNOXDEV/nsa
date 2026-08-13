-- newest-first page of messages that still have at least one attachment to fetch:
-- no attachment_files row yet, or an 'oversized' row whose recorded size now fits
-- under the configured cap ($2). $1 = exclusive keyset cursor (start at i64::MAX).
SELECT DISTINCT m.id, m.channel_id
FROM messages m
JOIN attachments a ON a.message_id = m.id
LEFT JOIN attachment_files f ON f.attachment_id = a.id
WHERE m.id < $1
  AND (f.attachment_id IS NULL
       OR (f.status = 'oversized' AND f.size_bytes <= $2))
ORDER BY m.id DESC
LIMIT 100;
