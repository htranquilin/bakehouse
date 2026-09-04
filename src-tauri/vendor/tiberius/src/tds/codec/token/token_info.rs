use crate::SqlReadBytes;

#[allow(dead_code)] // we might want to debug the values
#[derive(Debug)]
pub struct TokenInfo {
    /// info number
    pub(crate) number: u32,
    /// error state
    pub(crate) state: u8,
    /// severity (<10: Info)
    pub(crate) class: u8,
    pub(crate) message: String,
    pub(crate) server: String,
    pub(crate) procedure: String,
    pub(crate) line: u32,
}

impl TokenInfo {
    // Bakehouse patch: public accessors so a client can render info messages
    // (PRINT output, RAISERROR severity <= 10) in a messages pane.
    pub fn number(&self) -> u32 {
        self.number
    }
    pub fn state(&self) -> u8 {
        self.state
    }
    pub fn class(&self) -> u8 {
        self.class
    }
    pub fn message(&self) -> &str {
        &self.message
    }
    pub fn server(&self) -> &str {
        &self.server
    }
    pub fn procedure(&self) -> &str {
        &self.procedure
    }
    pub fn line(&self) -> u32 {
        self.line
    }

    pub(crate) async fn decode<R>(src: &mut R) -> crate::Result<Self>
    where
        R: SqlReadBytes + Unpin,
    {
        let _length = src.read_u16_le().await?;

        let number = src.read_u32_le().await?;
        let state = src.read_u8().await?;
        let class = src.read_u8().await?;
        let message = src.read_us_varchar().await?;
        let server = src.read_b_varchar().await?;
        let procedure = src.read_b_varchar().await?;
        let line = src.read_u32_le().await?;

        Ok(TokenInfo {
            number,
            state,
            class,
            message,
            server,
            procedure,
            line,
        })
    }
}
