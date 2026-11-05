-- Push tokens for Android (D60 §5): FCM registration tokens, stored as
-- their text's bytes, 100 to 1,024 bytes (the unique index can't hold much
-- more), always 'production'. APNs tokens keep their limits. Never edit
-- this file once merged.

ALTER TABLE push_tokens DROP CONSTRAINT push_tokens_app_check;
ALTER TABLE push_tokens DROP CONSTRAINT push_tokens_token_check;

ALTER TABLE push_tokens ADD CONSTRAINT push_tokens_app_check
    CHECK (app IN ('ios', 'macos', 'android'));
ALTER TABLE push_tokens ADD CONSTRAINT push_tokens_token_check
    CHECK ((app = 'android' AND octet_length(token) BETWEEN 100 AND 1024)
        OR (app <> 'android' AND octet_length(token) BETWEEN 32 AND 100));
ALTER TABLE push_tokens ADD CONSTRAINT push_tokens_android_production_check
    CHECK (app <> 'android' OR environment = 'production');
