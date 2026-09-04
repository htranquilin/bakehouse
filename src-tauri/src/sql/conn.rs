//! One TDS connection. This module is the only place that touches tiberius
//! directly — the escape seam if the driver ever has to be replaced.

use crate::error::{AppError, Result};
use crate::results::buffer::ColMeta;
use crate::results::cell::Cell;
use futures_util::TryStreamExt;
use tiberius::{AuthMethod, Client, Config, QueryItem};
use tokio::net::TcpStream;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

pub struct SqlConn {
    client: Client<Compat<TcpStream>>,
    spid: i16,
}

/// Events yielded while streaming a batch's response.
pub enum StreamEvent {
    ResultSet(Vec<ColMeta>),
    Row(Vec<Cell>),
    /// PRINT output / RAISERROR severity <= 10 / STATS messages.
    Info { number: u32, line: u32, message: String },
    /// Server error (severity > 10). The connection survives these.
    ServerError { number: u32, class: u8, line: u32, message: String, procedure: String },
    RowsAffected(u64),
}

/// Matches SSMS/ODBC connection defaults; without these, views created through
/// Bakehouse would get different metadata than views created in SSMS.
const SESSION_OPTIONS: &str = "SET ANSI_NULLS ON;
SET ANSI_PADDING ON;
SET ANSI_WARNINGS ON;
SET ANSI_NULL_DFLT_ON ON;
SET ARITHABORT ON;
SET CONCAT_NULL_YIELDS_NULL ON;
SET QUOTED_IDENTIFIER ON;
SET IMPLICIT_TRANSACTIONS OFF;
SET TEXTSIZE 2147483647;";

impl SqlConn {
    pub async fn connect(
        ip: &str,
        password: &str,
        database: Option<&str>,
    ) -> Result<Self> {
        let mut config = Config::new();
        config.host(ip);
        config.port(1433);
        config.authentication(AuthMethod::sql_server("sa", password));
        // The container's cert is self-signed and per-boot.
        config.trust_cert();
        config.application_name("Bakehouse");
        if let Some(db) = database {
            config.database(db);
        }

        let tcp = TcpStream::connect((ip, 1433))
            .await
            .map_err(|e| AppError::runtime(format!("cannot reach SQL Server at {ip}:1433: {e}"), None))?;
        tcp.set_nodelay(true).ok();
        let mut client = Client::connect(config, tcp.compat_write())
            .await
            .map_err(|e| AppError::runtime(format!("SQL login failed: {e}"), None))?;

        client
            .simple_query(SESSION_OPTIONS)
            .await
            .map_err(sql_err)?
            .into_results()
            .await
            .map_err(sql_err)?;

        let spid_rows = client
            .simple_query("SELECT @@SPID")
            .await
            .map_err(sql_err)?
            .into_first_result()
            .await
            .map_err(sql_err)?;
        let spid: i16 = spid_rows
            .first()
            .and_then(|r| r.get(0))
            .ok_or_else(|| AppError::Internal("no @@SPID returned".into()))?;

        Ok(Self { client, spid })
    }

    pub fn spid(&self) -> i16 {
        self.spid
    }

    /// Stream one T-SQL batch, invoking `on_event` for everything the server
    /// sends. Server errors are events (the connection survives); transport
    /// errors return Err (the connection is poisoned).
    pub async fn execute_streaming(
        &mut self,
        sql: &str,
        mut on_event: impl FnMut(StreamEvent),
    ) -> Result<()> {
        let mut stream = self.client.simple_query(sql).await.map_err(sql_err)?;
        let mut reported_errors: Vec<u32> = vec![];
        loop {
            match stream.try_next().await {
                Ok(Some(item)) => match item {
                    QueryItem::Metadata(meta) => {
                        let cols = meta
                            .columns()
                            .iter()
                            .map(|c| ColMeta {
                                name: c.name().to_string(),
                                sql_type: format!("{:?}", c.column_type()),
                            })
                            .collect();
                        on_event(StreamEvent::ResultSet(cols));
                    }
                    QueryItem::Row(row) => {
                        let cells =
                            row.cells().map(|(_, data)| Cell::from_column_data(data)).collect();
                        on_event(StreamEvent::Row(cells));
                    }
                    QueryItem::Info(info) => on_event(StreamEvent::Info {
                        number: info.number(),
                        line: info.line(),
                        message: info.message().to_string(),
                    }),
                    QueryItem::Error(e) => {
                        reported_errors.push(e.code());
                        on_event(StreamEvent::ServerError {
                            number: e.code(),
                            class: e.class(),
                            line: e.line(),
                            message: e.message().to_string(),
                            procedure: e.procedure().to_string(),
                        });
                    }
                    QueryItem::RowsAffected(n) => on_event(StreamEvent::RowsAffected(n)),
                },
                Ok(None) => return Ok(()),
                // The token stream re-raises the first server error at EOF;
                // we already emitted it as an event, so a clean end.
                Err(tiberius::error::Error::Server(e)) if reported_errors.contains(&e.code()) => {
                    return Ok(())
                }
                Err(tiberius::error::Error::Server(e)) => {
                    on_event(StreamEvent::ServerError {
                        number: e.code(),
                        class: e.class(),
                        line: e.line(),
                        message: e.message().to_string(),
                        procedure: e.procedure().to_string(),
                    });
                    return Ok(());
                }
                Err(e) => return Err(AppError::runtime(format!("connection lost: {e}"), None)),
            }
        }
    }

    /// Utility query: all rows of the first result set.
    pub async fn query_rows(&mut self, sql: &str) -> Result<Vec<Vec<Cell>>> {
        let rows = self
            .client
            .simple_query(sql)
            .await
            .map_err(sql_err)?
            .into_first_result()
            .await
            .map_err(sql_err)?;
        Ok(rows
            .into_iter()
            .map(|r| r.cells().map(|(_, data)| Cell::from_column_data(data)).collect())
            .collect())
    }

    /// Fire-and-drain (utility statements like KILL).
    pub async fn exec_simple(&mut self, sql: &str) -> Result<()> {
        self.client
            .simple_query(sql)
            .await
            .map_err(sql_err)?
            .into_results()
            .await
            .map_err(sql_err)?;
        Ok(())
    }

    /// Current @@TRANCOUNT, used for the open-transaction badge.
    pub async fn trancount(&mut self) -> Result<i32> {
        let rows = self.query_rows("SELECT @@TRANCOUNT").await?;
        Ok(rows
            .first()
            .and_then(|r| r.first())
            .and_then(|c| c.display.parse().ok())
            .unwrap_or(0))
    }

    /// Post-execution probe: @@TRANCOUNT plus the current database (a batch
    /// may have run USE, and the UI dropdown must follow).
    pub async fn probe_state(&mut self) -> Result<(i32, String)> {
        let rows = self.query_rows("SELECT @@TRANCOUNT, DB_NAME()").await?;
        let row = rows.first();
        Ok((
            row.and_then(|r| r.first()).and_then(|c| c.display.parse().ok()).unwrap_or(0),
            row.and_then(|r| r.get(1)).map(|c| c.display.clone()).unwrap_or_default(),
        ))
    }
}

fn sql_err(e: tiberius::error::Error) -> AppError {
    match e {
        tiberius::error::Error::Server(te) => AppError::runtime(
            format!("SQL error {}: {}", te.code(), te.message()),
            Some(format!("line {}, severity {}", te.line(), te.class())),
        ),
        other => AppError::runtime(format!("SQL connection error: {other}"), None),
    }
}
