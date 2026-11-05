CREATE TABLE IF NOT EXISTS secrets (
    id BLOB NOT NULL PRIMARY KEY,
    status SMALLINT NOT NULL,
    content BLOB NOT NULL
);

CREATE TABLE IF NOT EXISTS secret_refs (
    id BLOB NOT NULL PRIMARY KEY,
    status SMALLINT NOT NULL,
    secret_id BLOB NOT NULL REFERENCES secrets (
        id
    ) ON DELETE CASCADE ON UPDATE CASCADE,
    container_id BLOB NOT NULL REFERENCES containers (
        id
    ) ON DELETE CASCADE ON UPDATE CASCADE,
    target TEXT NOT NULL,
    uid BIGINT,
    gid BIGINT,
    mode BIGINT
);
