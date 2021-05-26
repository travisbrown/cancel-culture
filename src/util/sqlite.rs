use chrono::{DateTime, TimeZone, Utc};
use rusqlite::types::{FromSql, FromSqlResult, ToSql, ToSqlOutput, Value, ValueRef};
use rusqlite::Result;

pub(crate) struct SQLiteId(pub(crate) u64);

impl ToSql for SQLiteId {
    fn to_sql(&self) -> Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Owned(Value::Integer(self.0 as i64)))
    }
}

pub(crate) struct SQLiteDateTime(pub(crate) DateTime<Utc>);

impl ToSql for SQLiteDateTime {
    fn to_sql(&self) -> Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Owned(Value::Integer(
            self.0.timestamp_millis(),
        )))
    }
}

impl FromSql for SQLiteDateTime {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let ts: i64 = FromSql::column_result(value)?;

        Ok(SQLiteDateTime(Utc.timestamp_millis(ts)))
    }
}
