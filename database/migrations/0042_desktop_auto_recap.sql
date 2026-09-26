-- Automatic task recaps remain disabled until the user selects this preference.
ALTER TABLE desktop_settings
ADD COLUMN auto_recap INTEGER NOT NULL DEFAULT 0 CHECK (auto_recap IN (0, 1));
