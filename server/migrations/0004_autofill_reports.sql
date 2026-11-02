-- "Autofill didn't work here" reports (D38). Anonymous: no account, device
-- or IP address. Only the site's origin and the report's structure, which
-- the server checks against the v1 schema before storing; kept 90 days.
-- Never edit this file once merged.

CREATE TABLE autofill_reports (
    id          bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    received_at timestamptz NOT NULL DEFAULT now(),
    site        text NOT NULL CHECK (length(site) <= 300),
    report      jsonb NOT NULL CHECK (pg_column_size(report) <= 65536)
);

CREATE INDEX autofill_reports_received_at ON autofill_reports (received_at);
