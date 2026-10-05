use crate::{NativeDatabase, StoreError};
use rusqlite::{params, OptionalExtension};

#[derive(Clone, PartialEq, Eq)]
pub struct SubjectRecord {
    pub subject_id: String,
    pub username: Option<String>,
    pub password_hash: Option<String>,
}

impl NativeDatabase {
    pub fn create_subject_with_credential(
        &self,
        subject: &SubjectRecord,
        credential_hash: &str,
    ) -> Result<(), StoreError> {
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            if let Some(username) = &subject.username {
                let exists: bool = transaction.query_row(
                    "SELECT EXISTS(SELECT 1 FROM subjects WHERE username=?1)",
                    [username],
                    |row| row.get(0),
                )?;
                if exists {
                    return Err(StoreError::Invalid("IDENTITY_USERNAME_EXISTS".into()));
                }
            }
            transaction.execute(
                "INSERT INTO subjects(subject_id,username,password_hash) VALUES (?1,?2,?3)",
                params![subject.subject_id, subject.username, subject.password_hash],
            )?;
            transaction.execute(
                "INSERT INTO credentials(credential_hash,subject_id) VALUES (?1,?2)",
                params![credential_hash, subject.subject_id],
            )?;
            transaction.commit()?;
            Ok(())
        })
    }

    pub fn subject_by_username(&self, username: &str) -> Result<Option<SubjectRecord>, StoreError> {
        self.with_connection(|connection| {
            Ok(connection
                .query_row(
                    "SELECT subject_id,username,password_hash FROM subjects WHERE username=?1",
                    [username],
                    read_subject,
                )
                .optional()?)
        })
    }

    pub fn subject_by_credential_hash(
        &self,
        credential_hash: &str,
    ) -> Result<Option<SubjectRecord>, StoreError> {
        self.with_connection(|connection| {
            Ok(connection.query_row("SELECT s.subject_id,s.username,s.password_hash FROM subjects s JOIN credentials c ON c.subject_id=s.subject_id WHERE c.credential_hash=?1", [credential_hash], read_subject).optional()?)
        })
    }

    pub fn create_credential(
        &self,
        subject_id: &str,
        credential_hash: &str,
    ) -> Result<(), StoreError> {
        self.with_connection(|connection| {
            connection.execute(
                "INSERT INTO credentials(credential_hash,subject_id) VALUES (?1,?2)",
                params![credential_hash, subject_id],
            )?;
            Ok(())
        })
    }

    pub fn revoke_credential(&self, credential_hash: &str) -> Result<bool, StoreError> {
        self.with_connection(|connection| {
            Ok(connection.execute(
                "DELETE FROM credentials WHERE credential_hash=?1",
                [credential_hash],
            )? == 1)
        })
    }
}

fn read_subject(row: &rusqlite::Row<'_>) -> rusqlite::Result<SubjectRecord> {
    Ok(SubjectRecord {
        subject_id: row.get(0)?,
        username: row.get(1)?,
        password_hash: row.get(2)?,
    })
}
