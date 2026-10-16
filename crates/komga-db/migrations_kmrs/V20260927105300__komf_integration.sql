CREATE TABLE KOMF_INTEGRATION (
    ID                 INTEGER NOT NULL PRIMARY KEY CHECK (ID = 1),
    URL                TEXT    NOT NULL,
    BASE_URL           TEXT    NOT NULL,
    OWNER_USER_ID      TEXT    NULL,
    API_KEY_ID         TEXT    NULL,
    STATE              TEXT    NOT NULL DEFAULT 'pending',
    LAST_ERROR         TEXT    NULL,
    CREATED_DATE       TEXT    NOT NULL,
    LAST_MODIFIED_DATE TEXT    NOT NULL,
    CHECK (STATE IN ('pending', 'connected', 'error'))
);
