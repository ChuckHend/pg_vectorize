-- Run with: psql -X -v ON_ERROR_STOP=1 -f extension/tests/chunk_table.sql
-- Requires vectorize and its usual preload dependencies in the test database.
BEGIN;
SET LOCAL statement_timeout = '30s';
CREATE SCHEMA chunk_table_regression;
CREATE TABLE chunk_table_regression.docs (id integer, body text);

-- Empty input still creates an empty output table.
SELECT vectorize.chunk_table(
    'chunk_table_regression.docs', 'body', 'id', 100,
    'chunk_table_regression.empty_output'
);
DO $$ BEGIN
    IF (SELECT count(*) FROM chunk_table_regression.empty_output) <> 0 THEN
        RAISE EXCEPTION 'empty input produced chunks';
    END IF;
END $$;

-- A single row keeps its identifier and starts at chunk index zero.
INSERT INTO chunk_table_regression.docs VALUES (11, 'Short');
SELECT vectorize.chunk_table(
    'chunk_table_regression.docs', 'body', 'id', 100,
    'chunk_table_regression.single_output'
);
DO $$ BEGIN
    IF (SELECT count(*) FROM chunk_table_regression.single_output) <> 1
       OR NOT EXISTS (
           SELECT FROM chunk_table_regression.single_output
           WHERE original_id = 11 AND chunk_index = 0 AND chunk = 'Short'
       ) THEN
        RAISE EXCEPTION 'single input row was not preserved';
    END IF;
END $$;

-- All rows inserted in this transaction must be visible and processed.
INSERT INTO chunk_table_regression.docs VALUES (23, 'Second'), (37, 'Third');
SELECT vectorize.chunk_table(
    'chunk_table_regression.docs', 'body', 'id', 100,
    'chunk_table_regression.multi_output'
);
DO $$ BEGIN
    IF (SELECT count(*) FROM chunk_table_regression.multi_output) <> 3
       OR EXISTS (
           SELECT FROM chunk_table_regression.docs d
           WHERE NOT EXISTS (
               SELECT FROM chunk_table_regression.multi_output c
               WHERE c.original_id = d.id AND c.chunk_index = 0 AND c.chunk = d.body
           )
       ) THEN
        RAISE EXCEPTION 'chunk_table must process every input row';
    END IF;
END $$;

-- Each row has its own chunk indexes; NULL and empty documents remain skipped.
TRUNCATE chunk_table_regression.docs;
INSERT INTO chunk_table_regression.docs VALUES
    (5, 'abcdefghij'), (17, 'klmnopqrst'),
    (28, NULL), (31, ''), (NULL, 'ignored');
SELECT vectorize.chunk_table(
    'chunk_table_regression.docs', 'body', 'id', 4,
    'chunk_table_regression.split_output'
);
DO $$ BEGIN
    IF (SELECT count(*) FROM chunk_table_regression.split_output) <> 6
       OR EXISTS (
           SELECT * FROM (VALUES
               (5, 0, 'abcd'), (5, 1, 'efgh'), (5, 2, 'ij'),
               (17, 0, 'klmn'), (17, 1, 'opqr'), (17, 2, 'st')
           ) AS expected(original_id, chunk_index, chunk)
           EXCEPT ALL
           SELECT original_id, chunk_index, chunk
           FROM chunk_table_regression.split_output
       ) THEN
        RAISE EXCEPTION 'per-row chunks or indexes were not preserved';
    END IF;
END $$;

-- An insertion error rolls back the entire function call, including earlier rows.
TRUNCATE chunk_table_regression.docs;
INSERT INTO chunk_table_regression.docs VALUES (1, 'first'), (2, 'second');
CREATE TABLE chunk_table_regression.reject_output (
    id serial PRIMARY KEY,
    original_id integer CHECK (original_id <> 2),
    chunk_index integer,
    chunk text
);
DO $$ BEGIN
    BEGIN
        PERFORM vectorize.chunk_table(
            'chunk_table_regression.docs', 'body', 'id', 100,
            'chunk_table_regression.reject_output'
        );
        RAISE EXCEPTION 'expected the output constraint to reject a row';
    EXCEPTION WHEN check_violation THEN
        NULL;
    END;
    IF EXISTS (SELECT FROM chunk_table_regression.reject_output) THEN
        RAISE EXCEPTION 'failed chunk_table call left partial output';
    END IF;
END $$;
ROLLBACK;
