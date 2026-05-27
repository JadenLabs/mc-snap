use anyhow::Result;

pub async fn run() -> Result<()> {
    super::stop::run().await.ok();
    super::start::run(true).await
}
