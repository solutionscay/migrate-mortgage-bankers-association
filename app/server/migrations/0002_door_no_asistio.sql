-- 0002: "No Asistió" becomes a real registration status (D-91); mail_outbox.status gains 'mocked' (05 EXT-01, I4).
-- SQLite cannot change a CHECK in place, so both tables are rebuilt (ALTER TABLE "other changes" procedure):
-- the runner turns foreign keys off for the migration and runs PRAGMA foreign_key_check before it commits.
-- legacy_alter_table keeps the RENAME from rewriting the views that name registration (they keep working).
-- Data, keys, indexes and the foreign keys of registration_items, registration_guests and payments are kept.
PRAGMA foreign_keys = OFF;
BEGIN;
PRAGMA legacy_alter_table = ON;

CREATE TABLE registration_new (
  uuid TEXT PRIMARY KEY NOT NULL, -- #1 parent side of r4, r5, r7
  person_uuid TEXT NOT NULL REFERENCES people(uuid), -- #8 r1
  event_uuid TEXT REFERENCES events(uuid), -- #21 set by picking an active event (OP-REG-02)
  package_uuid TEXT REFERENCES packages(uuid), -- #30 r29; empty = individual (IND)
  registration_number INTEGER UNIQUE, -- #28 auto-serial
  guest_name_badges TEXT, -- #52 its exit trigger recomputes ticket_qty (BR-REG-03)
  status TEXT NOT NULL DEFAULT 'Nuevo' CHECK (status IN ('Nuevo', 'Registrado', 'Asistió', 'Canceló', 'No Canceló', 'No Asistió')), -- #16 + D-91 door value
  comments TEXT, -- #26
  confirmation_letter_sent INTEGER NOT NULL DEFAULT 0 CHECK (confirmation_letter_sent IN (0, 1)), -- #43 comment: confirm the letter was sent
  confirmation_letter_sent_time TEXT, -- #44
  costeado_por TEXT CHECK (costeado_por IN ('Afiliado', 'Individuo')), -- #56 value list costeado_por; selects the invoice (BR-RPT-01)
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%S','now','localtime')),
  created_by TEXT,
  modified_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%S','now','localtime')),
  modified_by TEXT
);
INSERT INTO registration_new SELECT * FROM registration;
DROP TABLE registration;
ALTER TABLE registration_new RENAME TO registration;
CREATE INDEX ix_registration_event ON registration (event_uuid);
CREATE INDEX ix_registration_person ON registration (person_uuid);

CREATE TABLE mail_outbox_new (
  id INTEGER PRIMARY KEY,
  email_uuid TEXT REFERENCES mail_emails(uuid) ON DELETE SET NULL,
  provider TEXT NOT NULL CHECK (provider IN ('resend', 'mock')),
  from_addr TEXT NOT NULL,
  to_addrs TEXT NOT NULL,
  cc TEXT,
  bcc TEXT,
  subject TEXT NOT NULL,
  html TEXT NOT NULL,
  attachment_file_uuid TEXT REFERENCES stored_files(uuid),
  status TEXT NOT NULL CHECK (status IN ('sent', 'failed', 'mocked')), -- mocked: no RESEND_API_KEY (05 EXT-01)
  provider_message_id TEXT,
  error TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%S','now','localtime')),
  created_by TEXT
);
INSERT INTO mail_outbox_new SELECT * FROM mail_outbox;
DROP TABLE mail_outbox;
ALTER TABLE mail_outbox_new RENAME TO mail_outbox;

PRAGMA legacy_alter_table = OFF;

INSERT INTO value_list_items (list, value, sort_order) VALUES ('registration_status', 'No Asistió', 5);
PRAGMA foreign_key_check;
COMMIT;
PRAGMA foreign_keys = ON;
