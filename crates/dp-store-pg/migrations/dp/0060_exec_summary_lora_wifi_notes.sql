-- 0060_exec_summary_lora_wifi_notes.sql
--
-- ES feedback #3: LoRa and WiFi were only reachable as checkboxes in
-- the Requirements `protocols` set, which can't carry the detail the
-- user needs (region/band/security). Promote them to first-class
-- free-text fields, plus a general free-text notes field on the
-- section.
--
-- All nullable so the §4.3 sparse-PATCH autosave keeps working — a
-- partial save only ever touches the fields the user changed.

ALTER TABLE dp_project_exec_summary
    ADD COLUMN lora      text NULL,   -- e.g. "AU915, SF7–SF12"
    ADD COLUMN wifi      text NULL,   -- e.g. "2.4 GHz b/g/n, WPA2"
    ADD COLUMN req_notes text NULL;   -- general free-text on Requirements
