CREATE TABLE clients (
    identity VARBINARY(255) PRIMARY KEY,
    password BLOB NOT NULL,
    last_auth_ms BIGINT NULL,
    CONSTRAINT clients_password_nonempty CHECK (OCTET_LENGTH(password) > 0),
    CONSTRAINT clients_auth_time_positive CHECK (last_auth_ms IS NULL OR last_auth_ms > 0)
) ENGINE=InnoDB;
