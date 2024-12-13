UPDATE containers
SET image_id = ?
WHERE
    containers.id IN (
        SELECT container_missing_images.container_id
        FROM container_missing_images
        WHERE container_missing_images.image_id = ?
    )
