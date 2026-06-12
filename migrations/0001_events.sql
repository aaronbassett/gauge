CREATE TABLE events (
    id           BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    app          TEXT        NOT NULL,
    app_version  TEXT        NOT NULL,
    install_id   UUID        NOT NULL,
    session_id   UUID        NOT NULL,
    os           TEXT        NOT NULL,
    arch         TEXT        NOT NULL,
    event_name   TEXT        NOT NULL,
    time         TIMESTAMPTZ NOT NULL,
    received_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    attributes   JSONB       NOT NULL DEFAULT '{}'::jsonb
);

CREATE INDEX events_app_name_time_idx    ON events (app, event_name, time);
CREATE INDEX events_app_install_time_idx ON events (app, install_id, time);
