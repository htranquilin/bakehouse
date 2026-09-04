use serde::{Deserialize, Serialize};

/// The default, digest-pinned SQL Server image (verified under Rosetta — spike/findings.md).
pub const DEFAULT_IMAGE: &str = "mcr.microsoft.com/mssql/server:2022-CU26-ubuntu-22.04@sha256:ba4c8329f48fb8f02e1416be6a930ebfd71268caee78aa985f3af4315e457c89";
pub const DEFAULT_MEMORY_MB: u32 = 4096;
pub const DEFAULT_SQL_MEMORY_MB: u32 = 3072;

/// SQL Server versions offered at instance creation. Digest-pinned (never
/// `:latest` — see spike/findings.md). 2025 must be CU1+ — the RTM requires
/// AVX, which Rosetta-for-Linux doesn't expose.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlVersionInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub image: &'static str,
    pub recommended: bool,
}

pub const SQL_VERSIONS: &[SqlVersionInfo] = &[
    SqlVersionInfo {
        id: "2025",
        label: "SQL Server 2025 (CU8)",
        image: "mcr.microsoft.com/mssql/server:2025-CU8-ubuntu-24.04@sha256:4bab24f36c1ecd48e85f7d37df26e6bf301641d84c3fe652f9a0dcc947d512e1",
        recommended: false,
    },
    SqlVersionInfo {
        id: "2022",
        label: "SQL Server 2022 (CU26)",
        image: DEFAULT_IMAGE,
        recommended: true,
    },
    SqlVersionInfo {
        id: "2019",
        label: "SQL Server 2019 (CU32)",
        image: "mcr.microsoft.com/mssql/server:2019-CU32-ubuntu-20.04@sha256:7a879e9af3557e81a3b3ad14acd071e3638788387f394d27a9d3404b28e954ce",
        recommended: false,
    },
    SqlVersionInfo {
        id: "2017",
        label: "SQL Server 2017 (CU31)",
        image: "mcr.microsoft.com/mssql/server:2017-CU31-ubuntu-18.04@sha256:7d194c54e34cb63bca083542369485c8f4141596805611e84d8c8bab2339eede",
        recommended: false,
    },
];

pub fn image_for_version(id: &str) -> Option<&'static str> {
    SQL_VERSIONS.iter().find(|v| v.id == id).map(|v| v.image)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    pub id: String,
    pub name: String,
    pub image: String,
    pub memory_mb: u32,
    pub sql_memory_mb: u32,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl Instance {
    pub fn new(name: String, memory_mb: u32, image: String) -> Self {
        let id = format!("bh-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
        Self {
            id,
            name,
            image,
            memory_mb,
            sql_memory_mb: DEFAULT_SQL_MEMORY_MB.min(memory_mb.saturating_sub(1024)),
            created_at: chrono::Utc::now(),
        }
    }

    /// Named volume holding /var/opt/mssql (data survives container --rm).
    pub fn volume(&self) -> String {
        format!("{}-data", self.id)
    }
}

/// Lifecycle state, held in memory by the manager (never persisted).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum InstanceState {
    Stopped,
    Pulling,
    Starting,
    WaitingForSql,
    Running { ip: String },
    Stopping,
    Failed { stage: String, detail: String },
}

impl InstanceState {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Pulling => "pulling",
            Self::Starting => "starting",
            Self::WaitingForSql => "waitingForSql",
            Self::Running { .. } => "running",
            Self::Stopping => "stopping",
            Self::Failed { .. } => "failed",
        }
    }
}
