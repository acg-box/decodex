CREATE TABLE desktop_settings (
	singleton        INTEGER PRIMARY KEY CHECK (singleton = 1),
	show_in_menu_bar INTEGER NOT NULL CHECK (show_in_menu_bar IN (0, 1)),
	revision         INTEGER NOT NULL CHECK (revision > 0)
) STRICT;

INSERT INTO desktop_settings (singleton, show_in_menu_bar, revision)
VALUES (1, 1, 1);
