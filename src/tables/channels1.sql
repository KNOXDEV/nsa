/* we have to alter this table after the fact
   because it circularly references messages
*/
ALTER TABLE channels
    ADD COLUMN IF NOT EXISTS first_message_id bigint REFERENCES messages(id),
    ADD COLUMN IF NOT EXISTS last_message_id bigint REFERENCES messages(id);