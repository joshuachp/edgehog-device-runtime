INSERT OR IGNORE INTO containers (
    id,
    local_id,
    image_id,
    status,
    network_mode,
    hostname,
    restart_policy,
    privileged
) VALUES (?, ?, ?, ?, ?, ?, ?, ?);
