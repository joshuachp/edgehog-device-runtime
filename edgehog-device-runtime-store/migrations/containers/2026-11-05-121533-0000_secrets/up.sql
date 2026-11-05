CREATE TABLE IF NOT EXISTS secrets (
    id BLOB NOT NULL PRIMARY KEY,
    status SMALLINT NOT NULL,
    content BLOB NOT NULL
);

CREATE TABLE IF NOT EXISTS secret_refs (
    id BLOB NOT NULL PRIMARY KEY,
    status SMALLINT NOT NULL,
    secret_id BLOB REFERENCES secrets (
        id
    ) ON DELETE CASCADE ON UPDATE CASCADE,
    target TEXT NOT NULL,
    uid BIGINT,
    gid BIGINT,
    mode INTEGER
);

CREATE TABLE IF NOT EXISTS secret_ref_missing_secrets (
    secret_ref_id BLOB PRIMARY KEY NOT NULL REFERENCES secret_refs (
        id
    ) ON DELETE CASCADE ON UPDATE CASCADE,
    secret_id BLOB NOT NULL
);

CREATE TABLE IF NOT EXISTS container_secret_refs (
    container_id BLOB NOT NULL REFERENCES containers (
        id
    ) ON DELETE CASCADE ON UPDATE CASCADE,
    secret_ref_id BLOB NOT NULL REFERENCES secret_refs (
        id
    ) ON DELETE CASCADE ON UPDATE CASCADE,
    PRIMARY KEY (container_id, secret_ref_id)
);

CREATE TABLE IF NOT EXISTS container_missing_secret_refs (
    container_id BLOB NOT NULL REFERENCES containers (
        id
    ) ON DELETE CASCADE ON UPDATE CASCADE,
    secret_ref_id BLOB NOT NULL,
    PRIMARY KEY (container_id, secret_ref_id)
);
