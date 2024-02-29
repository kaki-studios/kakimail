use crate::smtp_common::Mail;
use anyhow::{anyhow, Context, Result};
use libsql_client::{client::GenericClient, DatabaseClient, Statement};

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
        db.batch([
            "CREATE TABLE IF NOT EXISTS mail (uid integer, date text, sender text, recipients text, data text, outgoing bool, flags integer)",
            "CREATE INDEX IF NOT EXISTS mail_date ON mail(date)",
            "CREATE INDEX IF NOT EXISTS mail_uid ON mail(uid)",
            "CREATE INDEX IF NOT EXISTS mail_recipients ON mail(recipients)",
        ])
        .await?;
        Ok(Self { db })
    }
    pub async fn latest_uid(&self) -> Result<i64> {
        let count: i64 = i64::try_from(
            self.db
                .execute(Statement::new("SELECT * FROM mail"))
                .await?
                .rows
                .last()
                .context("No rows returned from a * query")?
                .values
                .first()
                .context("No values returned from a * query")?,
        )
        .unwrap_or(0);
        Ok(count)
    }

    /// Replicates received mail to the database
    pub async fn replicate(&self, mail: Mail, outgoing: bool) -> Result<()> {
        let now = chrono::offset::Utc::now()
            .format("%Y-%m-%d %H:%M:%S%.3f")
            .to_string();
        let sql_bool = if outgoing { 1 } else { 0 };
        let latest_uid = self.latest_uid().await?;

        self.db
            .execute(Statement::with_args(
                "INSERT INTO mail VALUES (?, ?, ?, ?, ?, ?, ?)",
                libsql_client::args!(
                    latest_uid as i32 + 1,
                    now,
                    mail.from,
                    mail.to.join(", "),
                    mail.data,
                    sql_bool,
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
