//! Persistent local storage for accounts, proxies and check history.

use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};

use rand::{rngs::OsRng, RngCore};
use rusqlite::{params, Connection, OptionalExtension};
use url::Url;

use crate::{parse_proxy, Account, CheckStatus};

/// Result of importing a text file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImportSummary {
    /// Newly inserted rows.
    pub imported: usize,
    /// Rows already present in the database.
    pub duplicates: usize,
    /// Non-empty rows that could not be parsed.
    pub invalid: usize,
}

/// Account and its persisted check state.
#[derive(Clone)]
pub struct StoredAccount {
    /// Stable local database ID.
    pub id: i64,
    /// Credentials used by the read-only IMAP client.
    pub account: Account,
    /// Last persisted check state.
    pub status: CheckStatus,
    /// Unix timestamp of the latest completed check.
    pub last_checked_at: Option<i64>,
    /// Proxy used by the latest check.
    pub proxy_id: Option<i64>,
}

/// Stored proxy. The URL contains credentials and must not be logged.
#[derive(Clone)]
pub struct ProxyRecord {
    /// Stable local database ID.
    pub id: i64,
    /// Full proxy URL, including optional credentials.
    pub url: String,
    /// Unix timestamp of the latest use.
    pub last_used_at: Option<i64>,
}

impl ProxyRecord {
    /// Returns an endpoint label without user information or passwords.
    #[must_use]
    pub fn endpoint(&self) -> String {
        Url::parse(&self.url).map_or_else(
            |_| "invalid proxy".into(),
            |url| {
                format!(
                    "{}://{}:{}",
                    url.scheme(),
                    url.host_str().unwrap_or("unknown"),
                    url.port_or_known_default().unwrap_or_default()
                )
            },
        )
    }
}

/// Aggregated result of one batch run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunSummary {
    /// Stable local database ID.
    pub id: i64,
    /// Unix start timestamp.
    pub started_at: i64,
    /// Unix finish timestamp; `None` after an application crash.
    pub finished_at: Option<i64>,
    /// Number of selected accounts.
    pub total: usize,
    /// Successful checks.
    pub valid: usize,
    /// Rejected credentials.
    pub invalid: usize,
    /// Transport and protocol errors.
    pub errors: usize,
}

/// Persistent settings shared by the desktop UI and embedded local API.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceSettings {
    /// Maximum number of simultaneous IMAP checks.
    pub concurrency: usize,
    /// Per-operation IMAP timeout in seconds.
    pub timeout_seconds: u64,
    /// Whether the loopback HTTP API should be started.
    pub api_enabled: bool,
    /// Loopback TCP port used by the HTTP API.
    pub api_port: u16,
    /// Bearer token required by all non-health API routes.
    pub api_token: String,
}

/// Thread-safe SQLite store. Every operation uses a short-lived lock.
pub struct Store {
    connection: Mutex<Connection>,
    check_active: AtomicBool,
}

impl Store {
    /// Opens or creates a database and applies the current schema.
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let connection = Connection::open(path)?;
        Self::from_connection(connection)
    }

    /// Creates an isolated in-memory store.
    ///
    /// # Errors
    ///
    /// Fails if SQLite cannot create the database or schema.
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(connection: Connection) -> rusqlite::Result<Self> {
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             PRAGMA busy_timeout = 5000;
             CREATE TABLE IF NOT EXISTS accounts (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 email TEXT NOT NULL COLLATE NOCASE UNIQUE,
                 password TEXT NOT NULL,
                 host TEXT,
                 port INTEGER NOT NULL DEFAULT 993,
                 starttls INTEGER NOT NULL DEFAULT 0,
                 status TEXT NOT NULL DEFAULT 'idle',
                 messages INTEGER,
                 last_error TEXT,
                 last_checked_at INTEGER,
                 proxy_id INTEGER REFERENCES proxies(id) ON DELETE SET NULL,
                 created_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS proxies (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 url TEXT NOT NULL UNIQUE,
                 last_used_at INTEGER,
                 created_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS runs (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 started_at INTEGER NOT NULL,
                 finished_at INTEGER
             );
             CREATE TABLE IF NOT EXISTS run_accounts (
                 run_id INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
                 account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                 proxy_id INTEGER REFERENCES proxies(id) ON DELETE SET NULL,
                 status TEXT NOT NULL,
                 messages INTEGER,
                 error TEXT,
                 checked_at INTEGER NOT NULL,
                 PRIMARY KEY (run_id, account_id)
             );
             CREATE INDEX IF NOT EXISTS run_accounts_account
                 ON run_accounts(account_id, checked_at DESC);",
        )?;
        let account_columns = {
            let mut statement = connection.prepare("PRAGMA table_info(accounts)")?;
            let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        for (name, definition) in [
            ("host", "host TEXT"),
            ("port", "port INTEGER NOT NULL DEFAULT 993"),
            ("starttls", "starttls INTEGER NOT NULL DEFAULT 0"),
        ] {
            if !account_columns.iter().any(|column| column == name) {
                connection.execute(&format!("ALTER TABLE accounts ADD COLUMN {definition}"), [])?;
            }
        }
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS workspace_settings (
                 id INTEGER PRIMARY KEY CHECK (id = 1),
                 concurrency INTEGER NOT NULL,
                 timeout_seconds INTEGER NOT NULL,
                 api_enabled INTEGER NOT NULL,
                 api_port INTEGER NOT NULL,
                 api_token TEXT NOT NULL
             );",
        )?;
        let has_settings = connection
            .query_row("SELECT 1 FROM workspace_settings WHERE id = 1", [], |_| {
                Ok(())
            })
            .optional()?
            .is_some();
        if !has_settings {
            connection.execute(
                "INSERT INTO workspace_settings
                 (id, concurrency, timeout_seconds, api_enabled, api_port, api_token)
                 VALUES (1, 8, 30, 0, 8765, ?1)",
                params![generate_api_token()],
            )?;
        }
        Ok(Self {
            connection: Mutex::new(connection),
            check_active: AtomicBool::new(false),
        })
    }

    /// Loads persistent workspace settings. A token is generated at database creation.
    pub fn workspace_settings(&self) -> rusqlite::Result<WorkspaceSettings> {
        let connection = self.connection.lock().expect("store mutex poisoned");
        connection.query_row(
            "SELECT concurrency, timeout_seconds, api_enabled, api_port, api_token
             FROM workspace_settings WHERE id = 1",
            [],
            |row| {
                Ok(WorkspaceSettings {
                    concurrency: row.get::<_, i64>(0)?.max(1) as usize,
                    timeout_seconds: row.get::<_, i64>(1)?.max(1) as u64,
                    api_enabled: row.get(2)?,
                    api_port: row.get::<_, i64>(3).and_then(|port| {
                        u16::try_from(port)
                            .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(3, port))
                    })?,
                    api_token: row.get(4)?,
                })
            },
        )
    }

    /// Saves workspace settings, generating a token if the supplied value is empty.
    pub fn save_workspace_settings(&self, settings: &WorkspaceSettings) -> rusqlite::Result<()> {
        let connection = self.connection.lock().expect("store mutex poisoned");
        let token = if settings.api_token.is_empty() {
            generate_api_token()
        } else {
            settings.api_token.clone()
        };
        connection.execute(
            "INSERT INTO workspace_settings
             (id, concurrency, timeout_seconds, api_enabled, api_port, api_token)
             VALUES (1, ?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET
                 concurrency = excluded.concurrency,
                 timeout_seconds = excluded.timeout_seconds,
                 api_enabled = excluded.api_enabled,
                 api_port = excluded.api_port,
                 api_token = excluded.api_token",
            params![
                settings.concurrency.max(1) as i64,
                settings.timeout_seconds.max(1) as i64,
                settings.api_enabled,
                settings.api_port,
                token
            ],
        )?;
        Ok(())
    }

    /// Imports `email:password` rows in one transaction.
    pub fn import_accounts(&self, input: &str) -> rusqlite::Result<ImportSummary> {
        let mut summary = ImportSummary::default();
        let mut connection = self.connection.lock().expect("store mutex poisoned");
        let transaction = connection.transaction()?;
        let now = unix_time();
        for row in input.lines().map(str::trim) {
            if row.is_empty() || row.starts_with('#') {
                continue;
            }
            let Ok(account) = Account::parse(row) else {
                summary.invalid += 1;
                continue;
            };
            let changed = transaction.execute(
                "INSERT OR IGNORE INTO accounts
                 (email, password, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
                params![account.email, account.password, now],
            )?;
            if changed == 0 {
                summary.duplicates += 1;
            } else {
                summary.imported += 1;
            }
        }
        transaction.commit()?;
        Ok(summary)
    }

    /// Saves one account for direct IMAP access, resetting stale check results.
    pub fn save_account(&self, account: &Account) -> rusqlite::Result<i64> {
        self.save_connection(account, None)
    }

    /// Atomically saves one account and its optional proxy connection.
    pub fn save_connection(
        &self,
        account: &Account,
        proxy_url: Option<&str>,
    ) -> rusqlite::Result<i64> {
        let proxy_url = proxy_url
            .map(|url| normalize_proxy(url).ok_or(rusqlite::Error::InvalidQuery))
            .transpose()?;
        let mut connection = self.connection.lock().expect("store mutex poisoned");
        self.ensure_idle()?;
        let transaction = connection.transaction()?;
        let now = unix_time();
        let proxy_id = proxy_url
            .map(|url| {
                transaction.execute(
                    "INSERT OR IGNORE INTO proxies (url, created_at) VALUES (?1, ?2)",
                    params![url, now],
                )?;
                transaction.query_row(
                    "SELECT id FROM proxies WHERE url = ?1",
                    params![url],
                    |row| row.get::<_, i64>(0),
                )
            })
            .transpose()?;
        transaction.execute(
            "INSERT INTO accounts
             (email, password, host, port, starttls, proxy_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
             ON CONFLICT(email) DO UPDATE SET
                 password = excluded.password,
                 host = excluded.host,
                 port = excluded.port,
                 starttls = excluded.starttls,
                 status = 'idle',
                 messages = NULL,
                 last_error = NULL,
                 last_checked_at = NULL,
                 proxy_id = excluded.proxy_id,
                 updated_at = excluded.updated_at",
            params![
                account.email,
                account.password,
                account.host,
                account.port,
                account.starttls,
                proxy_id,
                now
            ],
        )?;
        let id = transaction.query_row(
            "SELECT id FROM accounts WHERE email = ?1 COLLATE NOCASE",
            params![account.email],
            |row| row.get(0),
        )?;
        transaction.commit()?;
        Ok(id)
    }

    /// Imports and validates one proxy URL or `host:port:user:password` row per line.
    pub fn import_proxies(&self, input: &str) -> rusqlite::Result<ImportSummary> {
        let mut summary = ImportSummary::default();
        let mut connection = self.connection.lock().expect("store mutex poisoned");
        let transaction = connection.transaction()?;
        let now = unix_time();
        for row in input.lines().map(str::trim) {
            if row.is_empty() || row.starts_with('#') {
                continue;
            }
            let Some(url) = normalize_proxy(row) else {
                summary.invalid += 1;
                continue;
            };
            let changed = transaction.execute(
                "INSERT OR IGNORE INTO proxies (url, created_at) VALUES (?1, ?2)",
                params![url, now],
            )?;
            if changed == 0 {
                summary.duplicates += 1;
            } else {
                summary.imported += 1;
            }
        }
        transaction.commit()?;
        Ok(summary)
    }

    /// Loads accounts in stable import order.
    pub fn accounts(&self) -> rusqlite::Result<Vec<StoredAccount>> {
        let connection = self.connection.lock().expect("store mutex poisoned");
        let mut statement = connection.prepare(
            "SELECT id, email, password, host, port, starttls, status, messages, last_error,
                    last_checked_at, proxy_id FROM accounts ORDER BY id",
        )?;
        let rows = statement.query_map([], |row| {
            let status = decode_status(
                &row.get::<_, String>(6)?,
                row.get::<_, Option<u32>>(7)?,
                row.get::<_, Option<String>>(8)?,
            );
            Ok(StoredAccount {
                id: row.get(0)?,
                account: Account {
                    email: row.get(1)?,
                    password: row.get(2)?,
                    host: row.get(3)?,
                    port: row.get(4)?,
                    starttls: row.get(5)?,
                },
                status,
                last_checked_at: row.get(9)?,
                proxy_id: row.get(10)?,
            })
        })?;
        rows.collect()
    }

    /// Loads all imported proxies.
    pub fn proxies(&self) -> rusqlite::Result<Vec<ProxyRecord>> {
        let connection = self.connection.lock().expect("store mutex poisoned");
        let mut statement =
            connection.prepare("SELECT id, url, last_used_at FROM proxies ORDER BY id")?;
        let rows = statement.query_map([], |row| {
            Ok(ProxyRecord {
                id: row.get(0)?,
                url: row.get(1)?,
                last_used_at: row.get(2)?,
            })
        })?;
        rows.collect()
    }

    /// Starts a persistent batch run.
    pub fn begin_run(&self) -> rusqlite::Result<i64> {
        let connection = self.connection.lock().expect("store mutex poisoned");
        self.check_active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| rusqlite::Error::InvalidQuery)?;
        if let Err(error) = connection.execute(
            "INSERT INTO runs (started_at) VALUES (?1)",
            params![unix_time()],
        ) {
            self.check_active.store(false, Ordering::Release);
            return Err(error);
        }
        Ok(connection.last_insert_rowid())
    }

    /// Atomically records one result and updates the account's last-check fields.
    pub fn record_check(
        &self,
        run_id: i64,
        account_id: i64,
        proxy_id: Option<i64>,
        status: &CheckStatus,
    ) -> rusqlite::Result<()> {
        let (code, messages, error) = encode_status(status);
        let now = unix_time();
        let mut connection = self.connection.lock().expect("store mutex poisoned");
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT OR REPLACE INTO run_accounts
             (run_id, account_id, proxy_id, status, messages, error, checked_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![run_id, account_id, proxy_id, code, messages, error, now],
        )?;
        transaction.execute(
            "UPDATE accounts SET status = ?2, messages = ?3, last_error = ?4,
             last_checked_at = ?5, proxy_id = ?6, updated_at = ?5 WHERE id = ?1",
            params![account_id, code, messages, error, now, proxy_id],
        )?;
        if let Some(proxy_id) = proxy_id {
            transaction.execute(
                "UPDATE proxies SET last_used_at = ?2 WHERE id = ?1",
                params![proxy_id, now],
            )?;
        }
        transaction.commit()
    }

    /// Closes a run and computes its counters from stored per-account results.
    pub fn finish_run(&self, run_id: i64) -> rusqlite::Result<()> {
        let result = self
            .connection
            .lock()
            .expect("store mutex poisoned")
            .execute(
                "UPDATE runs SET finished_at = ?2 WHERE id = ?1",
                params![run_id, unix_time()],
            )
            .map(|_| ());
        self.check_active.store(false, Ordering::Release);
        result
    }

    fn ensure_idle(&self) -> rusqlite::Result<()> {
        if self.check_active.load(Ordering::Acquire) {
            Err(rusqlite::Error::InvalidQuery)
        } else {
            Ok(())
        }
    }

    /// Lists newest batch runs first.
    pub fn runs(&self, limit: usize) -> rusqlite::Result<Vec<RunSummary>> {
        let connection = self.connection.lock().expect("store mutex poisoned");
        let mut statement = connection.prepare(
            "SELECT r.id, r.started_at, r.finished_at,
                    COUNT(a.account_id),
                    COALESCE(SUM(a.status = 'valid'), 0),
                    COALESCE(SUM(a.status = 'invalid'), 0),
                    COALESCE(SUM(a.status = 'error'), 0)
             FROM runs r LEFT JOIN run_accounts a ON a.run_id = r.id
             GROUP BY r.id ORDER BY r.id DESC LIMIT ?1",
        )?;
        let rows = statement.query_map(params![limit as i64], |row| {
            Ok(RunSummary {
                id: row.get(0)?,
                started_at: row.get(1)?,
                finished_at: row.get(2)?,
                total: row.get(3)?,
                valid: row.get(4)?,
                invalid: row.get(5)?,
                errors: row.get(6)?,
            })
        })?;
        rows.collect()
    }

    /// Resets current account verdicts without deleting check history.
    pub fn reset_results(&self) -> rusqlite::Result<()> {
        let connection = self.connection.lock().expect("store mutex poisoned");
        self.ensure_idle()?;
        connection
            .execute(
                "UPDATE accounts SET status = 'idle', messages = NULL, last_error = NULL",
                [],
            )
            .map(|_| ())
    }

    /// Deletes one account and its run membership, preserving aggregate runs.
    pub fn delete_account(&self, account_id: i64) -> rusqlite::Result<()> {
        let connection = self.connection.lock().expect("store mutex poisoned");
        self.ensure_idle()?;
        connection
            .execute("DELETE FROM accounts WHERE id = ?1", params![account_id])
            .map(|_| ())
    }

    /// Deletes one proxy; historical account rows keep a NULL proxy reference.
    pub fn delete_proxy(&self, proxy_id: i64) -> rusqlite::Result<()> {
        let connection = self.connection.lock().expect("store mutex poisoned");
        self.ensure_idle()?;
        connection
            .execute("DELETE FROM proxies WHERE id = ?1", params![proxy_id])
            .map(|_| ())
    }

    /// Deletes every account and its per-run membership.
    pub fn clear_accounts(&self) -> rusqlite::Result<usize> {
        let connection = self.connection.lock().expect("store mutex poisoned");
        self.ensure_idle()?;
        connection.execute("DELETE FROM accounts", [])
    }

    /// Deletes every proxy, leaving historical rows without proxy references.
    pub fn clear_proxies(&self) -> rusqlite::Result<usize> {
        let connection = self.connection.lock().expect("store mutex poisoned");
        self.ensure_idle()?;
        connection.execute("DELETE FROM proxies", [])
    }

    /// Deletes all runs and their per-account results without changing current accounts.
    pub fn clear_history(&self) -> rusqlite::Result<usize> {
        let connection = self.connection.lock().expect("store mutex poisoned");
        self.ensure_idle()?;
        connection.execute("DELETE FROM runs", [])
    }
}

fn generate_api_token() -> String {
    let mut bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn normalize_proxy(row: &str) -> Option<String> {
    let url = if row.contains("://") {
        row.to_owned()
    } else {
        let (host, port, user, password) = row.split_once(':').and_then(|(host, rest)| {
            let (port, rest) = rest.split_once(':')?;
            let (user, password) = rest.split_once(':')?;
            Some((host, port, user, password))
        })?;
        if host.is_empty() || port.parse::<u16>().is_err() {
            return None;
        }
        format!("http://{user}:{password}@{host}:{port}")
    };
    parse_proxy(&url).ok().map(|_| url)
}

fn encode_status(status: &CheckStatus) -> (&'static str, Option<u32>, Option<&str>) {
    match status {
        CheckStatus::Idle => ("idle", None, None),
        CheckStatus::Checking => ("checking", None, None),
        CheckStatus::Valid { messages } => ("valid", Some(*messages), None),
        CheckStatus::Invalid { reason } => ("invalid", None, Some(reason)),
        CheckStatus::Error { reason } => ("error", None, Some(reason)),
    }
}

fn decode_status(code: &str, messages: Option<u32>, error: Option<String>) -> CheckStatus {
    match code {
        "valid" => CheckStatus::Valid {
            messages: messages.unwrap_or_default(),
        },
        "invalid" => CheckStatus::Invalid {
            reason: error.unwrap_or_default(),
        },
        "error" => CheckStatus::Error {
            reason: error.unwrap_or_default(),
        },
        "checking" => CheckStatus::Checking,
        _ => CheckStatus::Idle,
    }
}

fn unix_time() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_database() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "fuckthunder-gui-{}-{}.sqlite",
            std::process::id(),
            generate_api_token()
        ))
    }

    fn remove_database(path: &Path) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }

    #[test]
    fn manual_starttls_endpoint_survives_reopening_the_database() {
        let path = temporary_database();
        let account = Account::new(
            "person@example.com",
            "secret",
            Some("mail.example.com"),
            1143,
            true,
        )
        .unwrap();
        let id = Store::open(&path).unwrap().save_account(&account).unwrap();

        let store = Store::open(&path).unwrap();
        let stored = store.accounts().unwrap().remove(0);
        assert_eq!(stored.id, id);
        assert_eq!(stored.account.imap_host(), "mail.example.com");
        assert_eq!(stored.account.imap_port(), 1143);
        assert!(stored.account.uses_starttls());
        assert_eq!(stored.account.password, "secret");
        drop(store);
        remove_database(&path);
    }

    #[test]
    fn case_insensitive_update_preserves_identity_and_history() {
        let store = Store::open_in_memory().unwrap();
        store
            .import_accounts("person@example.com:old-secret")
            .unwrap();
        let original = store.accounts().unwrap().remove(0);
        let run = store.begin_run().unwrap();
        store
            .record_check(run, original.id, None, &CheckStatus::Valid { messages: 7 })
            .unwrap();
        store.finish_run(run).unwrap();
        let replacement = Account::new(
            "PERSON@example.com",
            "new-secret",
            Some("imap.example.com"),
            1993,
            false,
        )
        .unwrap();

        let id = store
            .save_connection(&replacement, Some("http://proxy.example:8080"))
            .unwrap();

        assert_eq!(id, original.id);
        let stored = store.accounts().unwrap().remove(0);
        assert_eq!(stored.account.password, "new-secret");
        assert_eq!(stored.account.imap_host(), "imap.example.com");
        assert_eq!(stored.account.imap_port(), 1993);
        assert_eq!(stored.status, CheckStatus::Idle);
        assert_eq!(stored.last_checked_at, None);
        assert!(stored.proxy_id.is_some());
        let runs = store.runs(1).unwrap();
        assert_eq!(runs[0].valid, 1);
        assert_eq!(runs[0].total, 1);
    }

    #[test]
    fn legacy_accounts_gain_default_connection_settings() {
        let path = temporary_database();
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE accounts (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     email TEXT NOT NULL COLLATE NOCASE UNIQUE,
                     password TEXT NOT NULL,
                     status TEXT NOT NULL DEFAULT 'idle',
                     messages INTEGER,
                     last_error TEXT,
                     last_checked_at INTEGER,
                     proxy_id INTEGER,
                     created_at INTEGER NOT NULL,
                     updated_at INTEGER NOT NULL
                 );
                 INSERT INTO accounts (email, password, created_at, updated_at)
                 VALUES ('legacy@example.com', 'secret', 1, 1);",
            )
            .unwrap();
        drop(connection);

        let store = Store::open(&path).unwrap();
        let stored = store.accounts().unwrap().remove(0);
        assert_eq!(stored.account.email, "legacy@example.com");
        assert_eq!(stored.account.password, "secret");
        assert_eq!(stored.account.imap_host(), "imap.example.com");
        assert_eq!(stored.account.imap_port(), 993);
        assert!(!stored.account.uses_starttls());
        drop(store);
        remove_database(&path);
    }

    #[test]
    fn persists_accounts_proxies_and_run_history() {
        let store = Store::open_in_memory().unwrap();
        let imported = store
            .import_accounts("one@gmx.de:secret\none@gmx.de:duplicate\nbroken")
            .unwrap();
        assert_eq!(imported.imported, 1);
        assert_eq!(imported.duplicates, 1);
        assert_eq!(imported.invalid, 1);
        let proxies = store
            .import_proxies("proxy.example:8080:user:pass\ninvalid")
            .unwrap();
        assert_eq!(proxies.imported, 1);
        assert_eq!(proxies.invalid, 1);

        let account = store.accounts().unwrap().remove(0);
        let proxy = store.proxies().unwrap().remove(0);
        assert_eq!(proxy.endpoint(), "http://proxy.example:8080");
        assert!(!proxy.endpoint().contains("pass"));
        let run = store.begin_run().unwrap();
        store
            .record_check(
                run,
                account.id,
                Some(proxy.id),
                &CheckStatus::Valid { messages: 42 },
            )
            .unwrap();
        store.finish_run(run).unwrap();

        let account = store.accounts().unwrap().remove(0);
        assert_eq!(account.status, CheckStatus::Valid { messages: 42 });
        assert!(account.last_checked_at.is_some());
        assert_eq!(account.proxy_id, Some(proxy.id));
        let runs = store.runs(10).unwrap();
        assert_eq!(runs[0].valid, 1);
        assert_eq!(runs[0].total, 1);
        assert!(runs[0].finished_at.is_some());
    }

    #[test]
    fn persists_workspace_settings_and_generates_only_missing_tokens() {
        let store = Store::open_in_memory().unwrap();
        let initial = store.workspace_settings().unwrap();
        assert_eq!(initial.concurrency, 8);
        assert_eq!(initial.timeout_seconds, 30);
        assert!(!initial.api_enabled);
        assert_eq!(initial.api_port, 8765);
        assert_eq!(initial.api_token.len(), 64);
        store
            .save_workspace_settings(&WorkspaceSettings {
                concurrency: 16,
                timeout_seconds: 45,
                api_enabled: true,
                api_port: 9000,
                api_token: initial.api_token.clone(),
            })
            .unwrap();
        let saved = store.workspace_settings().unwrap();
        assert_eq!(saved.concurrency, 16);
        assert_eq!(saved.timeout_seconds, 45);
        assert!(saved.api_enabled);
        assert_eq!(saved.api_port, 9000);
        assert_eq!(saved.api_token, initial.api_token);
    }

    #[test]
    fn reset_keeps_history_and_delete_cascades_safely() {
        let store = Store::open_in_memory().unwrap();
        store.import_accounts("a@web.de:secret").unwrap();
        store.import_proxies("http://proxy.example:8080").unwrap();
        let account = store.accounts().unwrap().remove(0);
        let proxy = store.proxies().unwrap().remove(0);
        let run = store.begin_run().unwrap();
        assert!(store.save_account(&account.account).is_err());
        store
            .record_check(run, account.id, Some(proxy.id), &CheckStatus::Idle)
            .unwrap();
        assert!(store.reset_results().is_err());
        assert!(store.clear_accounts().is_err());
        assert!(store.begin_run().is_err());
        store.finish_run(run).unwrap();
        store.reset_results().unwrap();
        store.delete_proxy(proxy.id).unwrap();
        store.delete_account(account.id).unwrap();
        assert!(store.accounts().unwrap().is_empty());
        assert!(store.proxies().unwrap().is_empty());
        assert_eq!(store.runs(1).unwrap()[0].total, 0);
    }
}
