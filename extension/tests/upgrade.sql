-- test_static seeds this job on the previous extension version.
-- Run immediately after ALTER EXTENSION, before tests recreate the extension.
DO $$
BEGIN
    IF NOT EXISTS (SELECT FROM vectorize._embeddings_static_test_job) THEN
        RAISE EXCEPTION 'upgrade lost the existing embeddings';
    END IF;

    IF (
        SELECT count(*) FROM vectorize.search(
            job_name => 'static_test_job',
            query => 'mobile devices',
            return_columns => ARRAY['product_id'],
            num_results => 3
        )
    ) <> 3 THEN
        RAISE EXCEPTION 'search against the upgraded job did not return three rows';
    END IF;
END $$;
