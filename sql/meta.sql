CREATE TABLE vectorize.job (
    job_id bigserial,
    name TEXT NOT NULL UNIQUE,
    job_type TEXT NOT NULL,
    transformer TEXT NOT NULL,
    search_alg TEXT NOT NULL,
    params jsonb NOT NULL,
    last_completion TIMESTAMP WITH TIME ZONE
);
