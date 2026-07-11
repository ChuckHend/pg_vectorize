-- Baseline: reflects the vectorize.job table as already deployed prior to
-- adopting sqlx migrations. Uses IF NOT EXISTS so this is a no-op against
-- any database where vectorize-core already created this table by hand.
CREATE TABLE IF NOT EXISTS vectorize.job
(
    id UUID DEFAULT gen_random_uuid() PRIMARY KEY,
    job_name TEXT NOT NULL UNIQUE,
    src_schema TEXT NOT NULL,
    src_table TEXT NOT NULL,
    src_columns TEXT[] NOT NULL,
    primary_key TEXT NOT NULL,
    update_time_col TEXT NOT NULL,
    model TEXT NOT NULL,
    params JSONB
);
