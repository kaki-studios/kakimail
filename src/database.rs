use crate::smtp_common::Mail;
use anyhow::{anyhow, Context, Result};
use libsql_client::{client::GenericClient, DatabaseClient, Statement, Value};

pub struct Client {
    db: GenericClient,
}

impl Client {
    /// Creates a new database client.
    /// If the LIBSQL_CLIENT_URL environment variable is not set, a local database will be used.
    /// It's also possible to use a remote database by setting the LIBSQL_CLIENT_URL environment variable.
    /// The `mail` table will be automatically created if it does not exist.
    pub async fn new() -> Result<Self> {
        if std::env::var("LIBSQL_CLIENT_URL").is_err() {
            let mut db_path = std::env::temp_dir();
            db_path.push("kakimail.db");
            let db_path = db_path.display();
            tracing::warn!("LIBSQL_CLIENT_URL not set, using a default local database: {db_path}");
            std::env::set_var("LIBSQL_CLIENT_URL", format!("file://{db_path}"));
        }
        let db = libsql_client::new_client().await?;

        //USERS TABLE, just in case kakimail-website didn't create it already
        db.batch([
            "PRAGMA foreign_keys = ON",
            "CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY, name TEXT UNIQUE, password TEXT);",
            "CREATE INDEX IF NOT EXISTS users_name ON users(name);",
            "CREATE INDEX IF NOT EXISTS users_id ON users(id);",
            //TESTING PURPOSES ONLY
            "INSERT OR IGNORE INTO users VALUES (0, 'test', 'nothashed')"
        ])
        .await.map_err(|e| {
                tracing::error!("1. {:?}", e);
                e
            })?;

        //MAILBOX TABLE
        db.batch([
            "PRAGMA foreign_keys = ON",
            "CREATE TABLE IF NOT EXISTS mailboxes (mid integer primary key not null, user_id integer not null, flags integer, FOREIGN KEY(user_id) REFERENCES users(id));",
            "CREATE INDEX IF NOT EXISTS mailbox_foreign_key ON mailboxes(user_id);",
            "CREATE INDEX IF NOT EXISTS mailbox_id ON mailboxes(mid);",
            //TESTING PURPOSES ONLY
            "INSERT OR IGNORE INTO mailboxes VALUES (0, 0, 0)"
        ])
        .await.map_err(|e| {
                tracing::error!("2. {:?}", e);
                e
            })?;

        //MAIL TABLE
        db.batch([
            "CREATE TABLE IF NOT EXISTS mail (uid integer unique not null, date text, sender text, recipients text, data text, 
             mailbox_id integer not null, flags integer, FOREIGN KEY(mailbox_id) REFERENCES mailboxes(id), PRIMARY KEY(uid));",
            "CREATE INDEX IF NOT EXISTS mail_date ON mail(date);",
            "CREATE INDEX IF NOT EXISTS mail_uid ON mail(uid);",
            "CREATE INDEX IF NOT EXISTS mail_flags ON mail(flags);",
            "CREATE INDEX IF NOT EXISTS mail_foreign_key ON mail(mailbox_id);",
        ])
        .await.map_err(|e| {
                tracing::error!("3. {:?}", e);
                e
            })?;
        Ok(Self { db })
    }
    pub async fn biggest_uid(&self) -> Result<i64> {
        let count: i64 = i64::try_from(
            self.db
                .execute(Statement::new("SELECT uid FROM mail"))
                .await?
                .rows
                .last()
                .context("No rows returned from SELECT uid query")?
                .values
                .first()
                .context("No values returned from a SELECT uid query")?,
        )
        .unwrap_or(0);
        Ok(count)
    }

    /// Replicates received mail to the database
    pub async fn replicate(&self, mail: Mail, mailbox_id: i32) -> Result<()> {
        let now = chrono::offset::Utc::now()
            .format("%Y-%m-%d %H:%M:%S%.3f")
            .to_string();
        let next_uid = self
            .biggest_uid()
            .await
            .map_err(|e| tracing::info!("first mail, no previous mail: {:?}", e))
            //so that it will become 0 in the db
            .unwrap_or(-1)
            + 1;

        self.db
            .execute(Statement::with_args(
                "INSERT INTO mail VALUES (?, ?, ?, ?, ?, ?, ?)",
                libsql_client::args!(
                    next_uid as i32,
                    now,
                    mail.from,
                    mail.to.join(", "),
                    mail.data,
                    mailbox_id,
                    0
                ),
            ))
            .await
            .map(|_| ())
    }

    /// Cleans up old mail
    #[allow(unused)]
    pub async fn delete_old_mail(&self) -> Result<()> {
        let now = chrono::offset::Utc::now();
        let a_week_ago = now - chrono::Duration::days(7);
        let a_week_ago = &a_week_ago.format("%Y-%m-%d %H:%M:%S%.3f").to_string();
        tracing::trace!("Deleting old mail from before {a_week_ago}");
        let count = self.mail_count().await.unwrap_or(0);
        tracing::debug!("Found {count} old mail");

        self.db
            .execute(Statement::with_args(
                "DELETE FROM mail WHERE date < ?",
                libsql_client::args!(a_week_ago),
            ))
            .await
            .ok();
        Ok(())
    }
    pub async fn mail_count(&self) -> Result<i64> {
        i64::try_from(
            self.db
                .execute(Statement::new("SELECT COUNT(*) FROM mail"))
                .await?
                .rows
                .first()
                .context("No rows returned from a COUNT(*) query")?
                .values
                .first()
                .context("No values returned from a COUNT(*) query")?,
        )
        .map_err(|e| anyhow!(e))
    }
    ///used with plain auth
    ///if user doesn't exist or the password is incorrect, returns None
    ///otherwise returns the users id
    pub async fn check_user(&self, username: &str, password: &str) -> Option<i32> {
        let values = self
            .db
            .execute(Statement::with_args(
                "SELECT _rowid_, password FROM users WHERE name = ?",
                libsql_client::args!(username),
            ))
            .await
            .ok()?;
        //fighting with the compiler
        let mut values = values.rows.first()?.values.iter();
        let id = i32::try_from(values.next()?).ok()?;
        let Value::Text { value: hash } = values.next()? else {
            return None;
        };

        if !bcrypt::verify(password, hash).ok()? {
            Option::None
        } else {
            Some(id)
        }
    }
}

pub enum IMAPFlag {
    Answered,
    Flagged,
    Deleted,
    Seen,
    Draft,
}

pub fn update_flags(flag: IMAPFlag, operation: bool) -> Result<()> {
    Ok(())
}
