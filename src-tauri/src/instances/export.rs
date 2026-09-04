//! Export an instance as a docker-compose.yml + README so it can be recreated
//! on any machine with vanilla Docker (requirement 2.3.1).

use super::model::Instance;
use crate::error::Result;
use std::path::Path;

pub fn write_compose(
    instance: &Instance,
    dest_dir: &Path,
    password: Option<&str>,
) -> Result<Vec<String>> {
    let mut written = vec![];
    let service = instance
        .name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>();

    let password_line = match password {
        Some(_) => "      MSSQL_SA_PASSWORD: ${MSSQL_SA_PASSWORD}".to_string(),
        None => "      MSSQL_SA_PASSWORD: ${MSSQL_SA_PASSWORD:?set a strong sa password}".to_string(),
    };

    let compose = format!(
        r#"# Exported from Bakehouse — instance "{name}"
# Run with: docker compose up -d        (on amd64, or Apple Silicon with Rosetta enabled)
services:
  {service}:
    image: {image}
    platform: linux/amd64
    environment:
      ACCEPT_EULA: "Y"
{password_line}
      MSSQL_MEMORY_LIMIT_MB: "{sql_mem}"
    ports:
      - "1433:1433"
    volumes:
      - {service}-data:/var/opt/mssql
    deploy:
      resources:
        limits:
          memory: {mem}M
    healthcheck:
      test: ["CMD-SHELL", "/opt/mssql-tools18/bin/sqlcmd -C -S localhost -U sa -P \"$$MSSQL_SA_PASSWORD\" -Q 'SELECT 1' || exit 1"]
      interval: 15s
      timeout: 5s
      retries: 10

volumes:
  {service}-data:
"#,
        name = instance.name,
        service = service,
        image = instance.image,
        sql_mem = instance.sql_memory_mb,
        mem = instance.memory_mb,
        password_line = password_line,
    );
    std::fs::create_dir_all(dest_dir)?;
    std::fs::write(dest_dir.join("docker-compose.yml"), compose)?;
    written.push("docker-compose.yml".into());

    if let Some(pw) = password {
        std::fs::write(dest_dir.join(".env"), format!("MSSQL_SA_PASSWORD={pw}\n"))?;
        written.push(".env".into());
    }

    let readme = format!(
        r#"# {name} — exported SQL Server instance

Created by Bakehouse on {date}.

- Image: `{image}` (amd64 only — on Apple Silicon enable Rosetta emulation in your container runtime)
- Data lives in the named volume `{service}-data`; the database files themselves are NOT
  included in this export. Restore your .bak files after the container is up, or back up
  databases from Bakehouse and restore them here.
- Start: `docker compose up -d`, then connect to `localhost:1433` as `sa`.
{env_note}
"#,
        name = instance.name,
        date = chrono::Utc::now().format("%Y-%m-%d"),
        image = instance.image,
        service = service,
        env_note = if password.is_some() {
            "- The sa password is in `.env` — keep that file private."
        } else {
            "- Set MSSQL_SA_PASSWORD in the environment (or a .env file) before starting."
        },
    );
    std::fs::write(dest_dir.join("README.md"), readme)?;
    written.push("README.md".into());
    Ok(written)
}
