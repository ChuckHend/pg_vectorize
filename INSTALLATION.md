# Installing pg_vectorize

To install the pg_vectorize extension into an existing Postgres cluster, run the following on the same host as Postgres:

1. Install [Rust](https://www.rust-lang.org/tools/install)

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

2. Install [pgrx](https://github.com/pgcentralfoundation/pgrx)

```bash
cargo install cargo-pgrx --version 0.13.1 --locked

cargo pgrx init --pg17 $(which pg_config)
```

3. Clone the repo and install dependencies

```bash
git clone https://github.com/ChuckHend/pg_vectorize.git

cd pg_vectorize/extension

# install dependencies
make setup PGRX_PG_CONFIG=$(which pg_config)
```

4. Compile and install pg_vectorize

```bash
cargo pgrx install
```

5. Update `shared_preload_libraries`

Set these values in your postgresql.conf file:

```
shared_preload_libraries = 'pg_cron, vectorize'
```

or run this SQL command then restart Postgres:

```sql
ALTER SYSTEM SET shared_preload_libraries TO 'pg_cron', 'vectorize';
```

6. Enable the extension

connect to your Postgres instance with something like `psql -U postgres`. Then, run:

```sql
CREATE EXTENSION vectorize CASCADE;
```
